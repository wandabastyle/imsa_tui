// Main frontend state container and side effects:
// initial snapshot load, stream subscription, and preference persistence.

import { get, writable } from 'svelte/store';

import {
  fetchDemoState,
  fetchNlsLiveticker,
  fetchPreferences,
  fetchSnapshot,
  openSeriesStream,
  updatePreferences,
} from '$lib/api';
import type { Preferences, Series, SnapshotResponse, ViewMode, TimingEntry } from '$lib/types';
import { ALL_SERIES } from '$lib/types';

interface SearchState {
  query: string;
  matches: number[];
  currentMatch: number;
  inputActive: boolean;
}

export interface AppState {
  snapshots: Partial<Record<Series, SnapshotResponse['snapshot']>>;
  activeSeries: Series;
  favourites: Set<string>;
  viewMode: ViewMode;
  selectedRow: number;
  gapAnchorStableId: string | null;
  showHelp: boolean;
  showSeriesPicker: boolean;
  seriesPickerIndex: number;
  showGroupPicker: boolean;
  groupPickerIndex: number;
  search: SearchState;
  demoEnabled: boolean;
  connectionErrors: string[];
  showNlsLiveticker: boolean;
  showMessages: boolean;
  nlsLiveticker: {
    entries: import('$lib/types').NlsLivetickerEntry[];
    lastUpdateUnixMs: bigint | null;
    lastError: string | null;
  };
}

const initialState: AppState = {
  snapshots: {},
  activeSeries: 'imsa',
  favourites: new Set<string>(),
  viewMode: { kind: 'overall' },
  selectedRow: 0,
  gapAnchorStableId: null,
  showHelp: false,
  showSeriesPicker: false,
  seriesPickerIndex: 0,
  showGroupPicker: false,
  groupPickerIndex: 0,
  search: {
    query: '',
    matches: [],
    currentMatch: 0,
    inputActive: false,
  },
  demoEnabled: false,
  connectionErrors: [],
  showNlsLiveticker: false,
  showMessages: false,
  nlsLiveticker: {
    entries: [],
    lastUpdateUnixMs: null,
    lastError: null,
  },
};

export const appState = writable<AppState>(initialState);

let activeStream: { series: Series; handle: EventSource } | null = null;

export async function initializeAppState(): Promise<void> {
  // Fetch all snapshots concurrently using Promise.allSettled
  const snapshotPromises = ALL_SERIES.map((series) => fetchSnapshot(series));
  const [prefsResult, demoResult, ...snapshotResults] = await Promise.allSettled([
    fetchPreferences(),
    fetchDemoState(),
    ...snapshotPromises,
  ]);

  // Extract values, using defaults on failure
  const prefs =
    prefsResult.status === 'fulfilled'
      ? prefsResult.value
      : { selected_series: 'imsa' as Series, favourites: [] };
  const demo = demoResult.status === 'fulfilled' ? demoResult.value : { enabled: false };

  appState.update((state) => {
    const nextSnapshots: AppState['snapshots'] = { ...state.snapshots };
    const errors: string[] = [...state.connectionErrors];

    for (let i = 0; i < snapshotResults.length; i++) {
      const result = snapshotResults[i];
      const series = ALL_SERIES[i];
      if (result.status === 'fulfilled') {
        nextSnapshots[result.value.series] = result.value.snapshot;
      } else {
        errors.push(`Failed to load ${series}: ${String(result.reason)}`);
      }
    }

    return {
      ...state,
      activeSeries: prefs.selected_series,
      favourites: new Set(prefs.favourites),
      snapshots: nextSnapshots,
      demoEnabled: demo.enabled,
      connectionErrors: errors.slice(-5), // Keep last 5 errors
    };
  });

  connectSeriesStream(prefs.selected_series);
}

export function destroyStreams(): void {
  activeStream?.handle.close();
  activeStream = null;
}

export function switchSeriesStream(series: Series): void {
  connectSeriesStream(series);
}

export function favouriteKey(series: Series, stableId: string): string {
  const normalizedStableId = normalizeStableIdForSeries(series, stableId);
  return `${series}|${normalizedStableId}`;
}

function normalizeStableIdForSeries(series: Series, stableId: string): string {
  if (series === 'imsa') {
    return trimLegacyClassSuffix(stableId, 'fallback');
  }
  if (series === 'nls') {
    return trimLegacyClassSuffix(stableId, 'stnr');
  }
  return stableId;
}

function trimLegacyClassSuffix(stableId: string, expectedPrefix: string): string {
  if (!stableId.startsWith(`${expectedPrefix}:`)) {
    return stableId;
  }
  const parts = stableId.split(':');
  if (parts.length < 3) {
    return stableId;
  }
  return `${parts[0]}:${parts[1]}`;
}

export async function persistPreferences(): Promise<void> {
  const state = get(appState);
  const payload: Preferences = {
    favourites: Array.from(state.favourites).sort(),
    selected_series: state.activeSeries,
  };
  const persisted = await updatePreferences(payload);
  appState.update((current) => ({
    ...current,
    favourites: new Set(persisted.favourites),
    activeSeries: persisted.selected_series,
  }));
}

function connectSeriesStream(series: Series): void {
  if (activeStream?.series === series) {
    return;
  }

  activeStream?.handle.close();

  const handle = openSeriesStream(series, (payload) => {
    appState.update((state) => ({
      ...state,
      snapshots: {
        ...state.snapshots,
        [payload.series]: payload.snapshot,
      },
    }));
  });

  handle.onerror = () => {
    appState.update((state) => ({
      ...state,
      connectionErrors: [...state.connectionErrors.slice(-4), `stream reconnect: ${series}`],
    }));
  };

  activeStream = { series, handle };
}

/**
 * Resolves the selected row by stable_id when entries change.
 * Tries to find the previously selected stable_id; falls back to nearest row index.
 */
export function resolveSelectedRow(
  currentEntries: TimingEntry[],
  previousStableId: string | null,
  previousRow: number,
  previousEntriesLength: number,
): { row: number; stableId: string | null } {
  // If no previous selection, start at 0
  if (!previousStableId || previousEntriesLength === 0) {
    return { row: 0, stableId: currentEntries[0]?.stable_id ?? null };
  }

  // Try to find the stable_id in the new entries
  const newIndex = currentEntries.findIndex((entry) => entry.stable_id === previousStableId);
  if (newIndex >= 0) {
    return { row: newIndex, stableId: previousStableId };
  }

  // Fallback: use the nearest row index
  const clampedRow = Math.min(previousRow, Math.max(0, currentEntries.length - 1));
  return {
    row: clampedRow,
    stableId: currentEntries[clampedRow]?.stable_id ?? null,
  };
}

/**
 * Fetches and updates the NLS liveticker state.
 */
export async function refreshNlsLiveticker(): Promise<void> {
  try {
    const response = await fetchNlsLiveticker();
    appState.update((state) => ({
      ...state,
      nlsLiveticker: {
        entries: response.entries,
        lastUpdateUnixMs: response.last_update_unix_ms ?? null,
        lastError: response.last_error,
      },
    }));
  } catch (error) {
    appState.update((state) => ({
      ...state,
      nlsLiveticker: {
        ...state.nlsLiveticker,
        lastError: error instanceof Error ? error.message : 'Failed to fetch liveticker',
      },
    }));
  }
}
