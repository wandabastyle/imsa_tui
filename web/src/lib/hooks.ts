// React hooks for app state management
import { useCallback, useEffect, useMemo, useState, type JSX } from 'react';

import {
  fetchDemoState,
  fetchNlsLiveticker,
  fetchPreferences,
  fetchSnapshot,
  openSeriesStream,
  updatePreferences,
} from './api';
import {
  ALL_SERIES,
  type NlsLivetickerEntry,
  type Preferences,
  type Series,
  type SnapshotResponse,
  type TimingEntry,
} from './types';

interface SearchState {
  currentMatch: number;
  inputActive: boolean;
  matches: number[];
  query: string;
}

interface NlsLivetickerState {
  entries: NlsLivetickerEntry[];
  lastError: string | null;
  lastUpdateUnixMs: bigint | null;
}

type ViewMode =
  | { kind: 'class'; index: number }
  | { kind: 'favourites' }
  | { kind: 'grouped' }
  | { kind: 'overall' };

export interface AppState {
  activeSeries: Series;
  connectionErrors: string[];
  demoEnabled: boolean;
  favourites: Set<string>;
  gapAnchorStableId: string | null;
  groupPickerIndex: number;
  groups: string[];
  nlsLiveticker: NlsLivetickerState;
  search: SearchState;
  selectedRow: number;
  seriesPickerIndex: number;
  showGroupPicker: boolean;
  showHelp: boolean;
  showMessages: boolean;
  showNlsLiveticker: boolean;
  showSeriesPicker: boolean;
  snapshots: Partial<Record<Series, SnapshotResponse['snapshot']>>;
  viewMode: ViewMode;
}

const INITIAL_STATE: AppState = {
  activeSeries: 'imsa',
  connectionErrors: [],
  demoEnabled: false,
  favourites: new Set(),
  gapAnchorStableId: null,
  groupPickerIndex: 0,
  groups: [],
  nlsLiveticker: {
    entries: [],
    lastError: null,
    lastUpdateUnixMs: null,
  },
  search: {
    currentMatch: 0,
    inputActive: false,
    matches: [],
    query: '',
  },
  selectedRow: 0,
  seriesPickerIndex: 0,
  showGroupPicker: false,
  showHelp: false,
  showMessages: false,
  showNlsLiveticker: false,
  showSeriesPicker: false,
  snapshots: {},
  viewMode: { kind: 'overall' },
};

// Constants for magic numbers
const INITIAL_ROW = 0;
const SLICE_START = 0;
const SLICE_END_NEGATIVE_FIVE = -5;
const SLICE_END_NEGATIVE_FOUR = -4;
const INDEX_NOT_FOUND = -1;
const TRIM_PARTS_MIN_LENGTH = 3;
const TRIM_PARTS_FIRST = 0;
const TRIM_PARTS_SECOND = 1;
const CONNECT_STREAM_INDEX_INCREMENT = 1;
const FIRST_ENTRY_INDEX = 0;
const ZERO_ENTRIES = 0;

export interface UseAppStateReturn {
  activeSnapshot: SnapshotResponse['snapshot'] | undefined;
  destroyStreams: () => void;
  favouriteKey: (series: Series, stableId: string) => string;
  initializeAppState: () => Promise<void>;
  persistPreferences: () => Promise<void>;
  refreshNlsLiveticker: () => Promise<void>;
  resolveSelectedRow: (
    currentEntries: TimingEntry[],
    previousStableId: string | null,
    previousRow: number,
    previousEntriesLength: number,
  ) => { row: number; stableId: string | null };
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  switchSeriesStream: (series: Series) => void;
  updateState: <Key extends keyof AppState>(key: Key, value: AppState[Key]) => void;
}

// Helper function defined before use
const trimLegacyClassSuffix = function trimLegacyClassSuffix(
  stableId: string,
  expectedPrefix: string,
): string {
  if (!stableId.startsWith(`${expectedPrefix}:`)) {
    return stableId;
  }
  const parts = stableId.split(':');
  if (parts.length < TRIM_PARTS_MIN_LENGTH) {
    return stableId;
  }
  return `${parts[TRIM_PARTS_FIRST]}:${parts[TRIM_PARTS_SECOND]}`;
};

const normalizeStableId = function normalizeStableId(series: Series, stableId: string): string {
  if (series === 'imsa') {
    return trimLegacyClassSuffix(stableId, 'fallback');
  }
  if (series === 'nls') {
    return trimLegacyClassSuffix(stableId, 'stnr');
  }
  return stableId;
};

// JSX is used for type annotations - do not remove
export type { JSX };

export const useAppState = function useAppState(): UseAppStateReturn {
  const [state, setState] = useState<AppState>(INITIAL_STATE);
  const [activeStream, setActiveStream] = useState<{ series: Series; handle: EventSource } | null>(
    null,
  );

  const updateState = useCallback(
    <Key extends keyof AppState>(key: Key, value: AppState[Key]): void => {
      setState((previous: AppState) => ({ ...previous, [key]: value }));
    },
    [],
  );

  const initializeAppState = useCallback(async (): Promise<void> => {
    const snapshotPromises: Promise<SnapshotResponse>[] = ALL_SERIES.map(
      async (seriesItem: Series): Promise<SnapshotResponse> => {
        const snapshot = await fetchSnapshot(seriesItem);
        return snapshot;
      },
    );
    const [prefsResult, demoResult, ...snapshotResults] = await Promise.allSettled([
      fetchPreferences(),
      fetchDemoState(),
      ...snapshotPromises,
    ]);

    const prefs: { favourites: string[]; selected_series: Series } =
      prefsResult.status === 'fulfilled'
        ? prefsResult.value
        : { favourites: [], selected_series: 'imsa' as Series };
    const demo: { enabled: boolean } =
      demoResult.status === 'fulfilled' ? demoResult.value : { enabled: false };

    const nextSnapshots: AppState['snapshots'] = { ...state.snapshots };
    const errors: string[] = [...state.connectionErrors];

    for (
      let index = SLICE_START;
      index < snapshotResults.length;
      index += CONNECT_STREAM_INDEX_INCREMENT
    ) {
      const result = snapshotResults[index];
      const seriesItem = ALL_SERIES[index];
      if (result.status === 'fulfilled') {
        nextSnapshots[result.value.series] = result.value.snapshot;
      } else {
        errors.push(`Failed to load ${seriesItem}: ${String(result.reason)}`);
      }
    }

    setState((previous: AppState) => ({
      ...previous,
      activeSeries: prefs.selected_series,
      connectionErrors: errors.slice(SLICE_END_NEGATIVE_FIVE),
      demoEnabled: demo.enabled,
      favourites: new Set(prefs.favourites),
      snapshots: nextSnapshots,
    }));

    // Connect stream
    if (activeStream?.series !== prefs.selected_series) {
      activeStream?.handle.close();
      const handle: EventSource = openSeriesStream(
        prefs.selected_series,
        (payload: SnapshotResponse): void => {
          setState((previous: AppState) => ({
            ...previous,
            snapshots: {
              ...previous.snapshots,
              [payload.series]: payload.snapshot,
            },
          }));
        },
      );

      // Use addEventListener instead of onerror
      const handleError = (): void => {
        setState((previous: AppState) => ({
          ...previous,
          connectionErrors: [
            ...previous.connectionErrors.slice(SLICE_END_NEGATIVE_FOUR),
            `stream reconnect: ${prefs.selected_series}`,
          ],
        }));
      };
      handle.addEventListener('error', handleError);

      setActiveStream({ handle, series: prefs.selected_series });
    }
  }, [activeStream, state.connectionErrors, state.snapshots]);

  const destroyStreams = useCallback((): void => {
    if (activeStream !== null) {
      activeStream.handle.close();
    }
    setActiveStream(null);
  }, [activeStream]);

  const switchSeriesStream = useCallback(
    (series: Series): void => {
      if (activeStream?.series === series) {
        return;
      }
      activeStream?.handle.close();
      const handle = openSeriesStream(series, (payload: SnapshotResponse): void => {
        setState((previous: AppState) => ({
          ...previous,
          snapshots: {
            ...previous.snapshots,
            [payload.series]: payload.snapshot,
          },
        }));
      });

      // Use addEventListener instead of onerror
      const handleError = (): void => {
        setState((previous: AppState) => ({
          ...previous,
          connectionErrors: [
            ...previous.connectionErrors.slice(SLICE_END_NEGATIVE_FOUR),
            `stream reconnect: ${series}`,
          ],
        }));
      };
      handle.addEventListener('error', handleError);

      setActiveStream({ handle, series });
    },
    [activeStream],
  );

  const persistPreferences = useCallback(async (): Promise<void> => {
    const favouritesArray: string[] = [...state.favourites];
    favouritesArray.sort();
    const payload: Preferences = {
      favourites: favouritesArray,
      selected_series: state.activeSeries,
    };
    const persisted = await updatePreferences(payload);
    setState((previous: AppState) => ({
      ...previous,
      activeSeries: persisted.selected_series,
      favourites: new Set(persisted.favourites),
    }));
  }, [state.activeSeries, state.favourites]);

  const refreshNlsLiveticker = useCallback(async (): Promise<void> => {
    try {
      const response = await fetchNlsLiveticker();
      setState((previous: AppState) => ({
        ...previous,
        nlsLiveticker: {
          entries: response.entries,
          lastError: response.last_error,
          lastUpdateUnixMs: response.last_update_unix_ms ?? null,
        },
      }));
    } catch (error: unknown) {
      setState((previous: AppState) => ({
        ...previous,
        nlsLiveticker: {
          ...previous.nlsLiveticker,
          lastError: error instanceof Error ? error.message : 'Failed to fetch liveticker',
        },
      }));
    }
  }, []);

  const favouriteKey = useCallback((series: Series, stableId: string): string => {
    const normalized = normalizeStableId(series, stableId);
    return `${series}|${normalized}`;
  }, []);

  const resolveSelectedRow = useCallback(
    (
      currentEntries: TimingEntry[],
      previousStableId: string | null,
      previousRow: number,
      previousEntriesLength: number,
    ): { row: number; stableId: string | null } => {
      if (previousStableId === null || previousEntriesLength === ZERO_ENTRIES) {
        return { row: INITIAL_ROW, stableId: currentEntries[FIRST_ENTRY_INDEX]?.stable_id ?? null };
      }
      const newIndex = currentEntries.findIndex(
        (entry: TimingEntry) => entry.stable_id === previousStableId,
      );
      if (newIndex !== INDEX_NOT_FOUND) {
        return { row: newIndex, stableId: previousStableId };
      }
      const clampedRow = Math.min(
        previousRow,
        Math.max(INITIAL_ROW, currentEntries.length - CONNECT_STREAM_INDEX_INCREMENT),
      );
      return {
        row: clampedRow,
        stableId: currentEntries[clampedRow]?.stable_id ?? null,
      };
    },
    [],
  );

  const activeSnapshot = useMemo(
    () => state.snapshots[state.activeSeries],
    [state.activeSeries, state.snapshots],
  );

  return {
    activeSnapshot,
    destroyStreams,
    favouriteKey,
    initializeAppState,
    persistPreferences,
    refreshNlsLiveticker,
    resolveSelectedRow,
    setState,
    state,
    switchSeriesStream,
    updateState,
  };
};

export const useKeyboard = function useKeyboard(handler: (event: KeyboardEvent) => void): void {
  useEffect(() => {
    globalThis.addEventListener('keydown', handler);
    return (): void => {
      globalThis.removeEventListener('keydown', handler);
    };
  }, [handler]);
};
