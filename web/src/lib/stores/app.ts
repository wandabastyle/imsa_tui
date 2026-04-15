// Main frontend state container and side effects:
// initial snapshot load, stream subscription, and preference persistence.

import { get, writable } from 'svelte/store';

import {
  fetchDemoState,
  fetchPreferences,
  fetchSnapshot,
  openSeriesStream,
  updatePreferences
} from '$lib/api';
import type { Preferences, Series, SnapshotResponse, ViewMode } from '$lib/types';
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
    inputActive: false
  },
  demoEnabled: false,
  connectionErrors: []
};

export const appState = writable<AppState>(initialState);

let streamHandles: EventSource[] = [];

export async function initializeAppState(): Promise<void> {
  const [prefs, demo, ...snapshots] = await Promise.all([
    fetchPreferences(),
    fetchDemoState(),
    ...ALL_SERIES.map((series) => fetchSnapshot(series))
  ]);

  appState.update((state) => {
    const nextSnapshots: AppState['snapshots'] = {};
    for (const snapshot of snapshots) {
      nextSnapshots[snapshot.series] = snapshot.snapshot;
    }
    return {
      ...state,
      activeSeries: prefs.selected_series,
      favourites: new Set(prefs.favourites),
      snapshots: nextSnapshots,
      demoEnabled: demo.enabled
    };
  });

  // One stream per series keeps data warm, even when user switches tabs/views.
  streamHandles = ALL_SERIES.map((series) =>
    openSeriesStream(series, (payload) => {
      appState.update((state) => ({
        ...state,
        snapshots: {
          ...state.snapshots,
          [payload.series]: payload.snapshot
        }
      }));
    })
  );

  for (const [index, handle] of streamHandles.entries()) {
    handle.onerror = () => {
      appState.update((state) => ({
        ...state,
        connectionErrors: [
          ...state.connectionErrors.slice(-4),
          `stream reconnect: ${ALL_SERIES[index]}`
        ]
      }));
    };
  }
}

export function destroyStreams(): void {
  for (const handle of streamHandles) {
    handle.close();
  }
  streamHandles = [];
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
    selected_series: state.activeSeries
  };
  const persisted = await updatePreferences(payload);
  appState.update((current) => ({
    ...current,
    favourites: new Set(persisted.favourites),
    activeSeries: persisted.selected_series
  }));
}
