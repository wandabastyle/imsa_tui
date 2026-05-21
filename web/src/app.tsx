// Main App component
import { useCallback, useEffect, useMemo, useState, type JSX } from 'react';

import {
  fetchSessionState,
  loginWithAccessCode,
  updateDemoState,
} from './lib/api';
import { HeaderBar } from './lib/components/header-bar';
import { HelpModal } from './lib/components/help-modal';
import { MessagesModal } from './lib/components/messages-modal';
import { NlsLivetickerModal } from './lib/components/nls-liveticker-modal';
import { SeriesModal } from './lib/components/series-modal';
import { TimingTable } from './lib/components/timing-table';
import { useAppState, useKeyboard, type AppState, type UseAppStateReturn } from './lib/hooks';
import { ALL_SERIES, type Series, type TimingEntry, type ViewMode } from './lib/types';

import './app.css';

const SLICE_START_INDEX = 0;
const DEFAULT_GROUP_PICKER_INDEX = 0;
const DEFAULT_SELECTED_ROW = 0;
const FIRST_MATCH_INDEX = 0;
const INDEX_DECREMENT = -1;
const INDEX_INCREMENT = 1;
const JUMP_BACKWARD = -1;
const JUMP_FORWARD = 1;
const KEY_LENGTH_SINGLE = 1;
const MINIMUM_GROUP_COUNT = 0;
const MINIMUM_LENGTH = 0;
const PAGE_JUMP_SIZE = 10;
const SLICE_REMOVE_LAST = 1;
const ZERO_LENGTH = 0;

// Helper functions (hoisted before App component for no-use-before-define rule)
const classDisplayName = (value: string): string => {
  const normalized: string = value.replaceAll(' ', '').replaceAll('_', '').toUpperCase();
  if (normalized === 'GTDPRO') {
    return 'GTD PRO';
  }
  return value.trim() || '-';
};

const getGroups = (entries: TimingEntry[]): string[] => {
  const grouped = new Map<string, TimingEntry[]>();
  for (const entry of entries) {
    const group: string = classDisplayName(entry.class_name);
    if (!grouped.has(group)) {
      grouped.set(group, []);
    }
    const groupEntries: TimingEntry[] | undefined = grouped.get(group);
    if (groupEntries !== undefined) {
      groupEntries.push(entry);
    }
  }
  // oxlint doesn't support ES2024's toSorted yet
  // eslint-disable-next-line unicorn/no-array-sort
  return [...grouped.keys()].sort();
};

const nextViewMode = (current: ViewMode, groupCount: number): ViewMode => {
  if (groupCount === MINIMUM_GROUP_COUNT) {
    if (current.kind === 'overall') {
      return { kind: 'grouped' };
    }
    if (current.kind === 'grouped') {
      return { kind: 'favourites' };
    }
    return { kind: 'overall' };
  }
  if (current.kind === 'overall') {
    return { kind: 'grouped' };
  }
  if (current.kind === 'grouped') {
    return { index: 0, kind: 'class' };
  }
  if (current.kind === 'class') {
    return current.index + INDEX_INCREMENT < groupCount
      ? { index: current.index + INDEX_INCREMENT, kind: 'class' }
      : { kind: 'favourites' };
  }
  return { kind: 'overall' };
};

export const App = (): JSX.Element => {
  const {
    activeSnapshot,
    destroyStreams,
    favouriteKey,
    initializeAppState,
    persistPreferences,
    refreshNlsLiveticker,
    setState,
    state,
    switchSeriesStream,
  }: UseAppStateReturn = useAppState();

  const [authChecking, setAuthChecking] = useState<boolean>(true);
  const [authenticated, setAuthenticated] = useState<boolean>(false);
  const [loading, setLoading] = useState<boolean>(true);
  const [loadError, setLoadError] = useState<string>('');
  const [loginCode, setLoginCode] = useState<string>('');
  const [loginError, setLoginError] = useState<string>('');

  // Derived values
  const activeEntries: TimingEntry[] = useMemo((): TimingEntry[] => {
    const entries: TimingEntry[] = activeSnapshot?.entries ?? [];
    if (state.viewMode.kind === 'favourites') {
      return entries.filter((entry: TimingEntry): boolean => {
        const key: string = favouriteKey(state.activeSeries, entry.stable_id);
        return state.favourites.has(key);
      });
    }
    if (state.viewMode.kind === 'class' && state.groups.length > MINIMUM_LENGTH) {
      const group: string = state.groups[state.viewMode.index];
      return entries.filter((entry: TimingEntry): boolean => classDisplayName(entry.class_name) === group);
    }
    return entries;
  }, [
    activeSnapshot?.entries,
    favouriteKey,
    state.activeSeries,
    state.favourites,
    state.groups,
    state.viewMode,
  ]);

  const searchMatches: number[] = useMemo((): number[] => {
    if (state.search.query === '') {
      return [];
    }
    const matches: number[] = [];
    const query: string = state.search.query.toLowerCase();
    for (let index = FIRST_MATCH_INDEX; index < activeEntries.length; index += INDEX_INCREMENT) {
      const entry: TimingEntry = activeEntries[index];
      if (
        entry.car_number.toLowerCase().includes(query) ||
        entry.driver.toLowerCase().includes(query) ||
        entry.vehicle.toLowerCase().includes(query) ||
        entry.team.toLowerCase().includes(query)
      ) {
        matches.push(index);
      }
    }
    return matches;
  }, [activeEntries, state.search.query]);

  // Actions
  const chooseSeries = useCallback(async (series: Series): Promise<void> => {
    setState((prev: AppState) => ({
      ...prev,
      activeSeries: series,
      gapAnchorStableId: null,
      selectedRow: DEFAULT_SELECTED_ROW,
      showGroupPicker: false,
      showSeriesPicker: false,
      viewMode: { kind: 'overall' },
    }));
    switchSeriesStream(series);
    await persistPreferences();
  }, [persistPreferences, setState, switchSeriesStream]);

  const cycleView = useCallback((): void => {
    const groups = getGroups(activeSnapshot?.entries ?? []);
    setState((prev: AppState) => ({
      ...prev,
      gapAnchorStableId: null,
      selectedRow: DEFAULT_SELECTED_ROW,
      viewMode: nextViewMode(prev.viewMode, groups.length),
    }));
  }, [activeSnapshot?.entries, setState]);

  const jumpFavourite = useCallback((): void => {
    if (activeEntries.length === MINIMUM_LENGTH) {
      return;
    }
    const start: number = state.selectedRow;
    for (let offset = INDEX_INCREMENT; offset <= activeEntries.length; offset += INDEX_INCREMENT) {
      const idx: number = (start + offset) % activeEntries.length;
      const entry: TimingEntry = activeEntries[idx];
      const key: string = favouriteKey(state.activeSeries, entry.stable_id);
      if (state.favourites.has(key)) {
        setState((prev: AppState) => ({
          ...prev,
          gapAnchorStableId: entry.stable_id,
          selectedRow: idx,
        }));
        return;
      }
    }
  }, [activeEntries, favouriteKey, setState, state.activeSeries, state.favourites, state.selectedRow]);

  const jumpSearch = useCallback((delta: number): void => {
    if (searchMatches.length === MINIMUM_LENGTH) {
      return;
    }
    setState((prev: AppState) => {
      const start = Math.min(prev.search.currentMatch, searchMatches.length + INDEX_DECREMENT);
      const next = (start + delta + searchMatches.length) % searchMatches.length;
      return {
        ...prev,
        search: { ...prev.search, currentMatch: next },
        selectedRow: searchMatches[next],
      };
    });
  }, [searchMatches, setState]);

  const selectGroup = useCallback((index: number): void => {
    setState((prev: AppState) => ({
      ...prev,
      gapAnchorStableId: null,
      groupPickerIndex: index,
      selectedRow: DEFAULT_SELECTED_ROW,
      showGroupPicker: false,
      viewMode: { index, kind: 'class' },
    }));
  }, [setState]);

  const shiftSelection = useCallback((delta: number): void => {
    setState((prev: AppState) => {
      const max = Math.max(activeEntries.length + INDEX_DECREMENT, DEFAULT_SELECTED_ROW);
      const next = Math.max(DEFAULT_SELECTED_ROW, Math.min(max, prev.selectedRow + delta));
      return { ...prev, selectedRow: next };
    });
  }, [activeEntries.length, setState]);

  const submitLogin = useCallback(async (): Promise<void> => {
    setLoginError('');
    const result = await loginWithAccessCode(loginCode.trim());
    if (!result.ok) {
      setLoginError(
        result.retryAfterSecs !== undefined && result.retryAfterSecs > ZERO_LENGTH
          ? `${result.error ?? 'login blocked'} (retry in ${String(result.retryAfterSecs)}s)`
          : (result.error ?? 'Invalid access code'),
      );
      return;
    }
    setAuthenticated(true);
    setLoginCode('');
    await initializeAppState();
  }, [initializeAppState, loginCode]);

  const toggleDemoMode = useCallback(async (): Promise<void> => {
    const nextEnabled = !state.demoEnabled;
    await updateDemoState(nextEnabled);
    setState((prev: AppState) => ({ ...prev, demoEnabled: nextEnabled }));
  }, [setState, state.demoEnabled]);

  const toggleFavourite = useCallback(async (): Promise<void> => {
    const selected: TimingEntry = activeEntries[state.selectedRow];
    const key = favouriteKey(state.activeSeries, selected.stable_id);
    setState((prev: AppState) => {
      const next = new Set(prev.favourites);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return { ...prev, favourites: next };
    });
    await persistPreferences();
  }, [activeEntries, favouriteKey, persistPreferences, setState, state.activeSeries, state.selectedRow]);

  // Keyboard handlers
  const handleGroupPickerKeydown = useCallback((event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      setState((prev: AppState) => ({ ...prev, showGroupPicker: false }));
      event.preventDefault();
    } else if (event.key === 'ArrowDown' || event.key === 'j') {
      setState((prev: AppState) => ({
        ...prev,
        groupPickerIndex: (prev.groupPickerIndex + INDEX_INCREMENT) % state.groups.length,
      }));
      event.preventDefault();
    } else if (event.key === 'ArrowUp' || event.key === 'k') {
      setState((prev: AppState) => ({
        ...prev,
        groupPickerIndex:
          prev.groupPickerIndex === DEFAULT_GROUP_PICKER_INDEX
            ? state.groups.length + INDEX_DECREMENT
            : prev.groupPickerIndex + INDEX_DECREMENT,
      }));
      event.preventDefault();
    } else if (event.key === 'Enter') {
      selectGroup(state.groupPickerIndex);
      event.preventDefault();
    }
  }, [selectGroup, setState, state.groupPickerIndex, state.groups.length]);

  const handleMainKeydown = useCallback((event: KeyboardEvent): void => {
    switch (event.key) {
      case 'Escape': {
        if (state.showHelp) {
          setState((prev: AppState) => ({ ...prev, showHelp: false }));
          event.preventDefault();
        }
        break;
      }
      case 'h':
      case '?': {
        setState((prev: AppState) => ({ ...prev, showHelp: !prev.showHelp }));
        event.preventDefault();
        break;
      }
      case 'g': {
        cycleView();
        event.preventDefault();
        break;
      }
      case 'G': {
        setState((prev: AppState) => ({
          ...prev,
          groupPickerIndex:
            prev.viewMode.kind === 'class' && prev.groups.length > MINIMUM_LENGTH
              ? Math.min(prev.viewMode.index, prev.groups.length + INDEX_DECREMENT)
              : DEFAULT_GROUP_PICKER_INDEX,
          showGroupPicker: true,
        }));
        event.preventDefault();
        break;
      }
      case 'o': {
        setState((prev: AppState) => ({
          ...prev,
          gapAnchorStableId: null,
          selectedRow: DEFAULT_SELECTED_ROW,
          viewMode: { kind: 'overall' },
        }));
        event.preventDefault();
        break;
      }
      case 't': {
        setState((prev: AppState) => ({
          ...prev,
          seriesPickerIndex: ALL_SERIES.indexOf(prev.activeSeries),
          showSeriesPicker: true,
        }));
        event.preventDefault();
        break;
      }
      case 'ArrowDown':
      case 'j': {
        shiftSelection(JUMP_FORWARD);
        event.preventDefault();
        break;
      }
      case 'ArrowUp':
      case 'k': {
        shiftSelection(JUMP_BACKWARD);
        event.preventDefault();
        break;
      }
      case 'PageDown': {
        shiftSelection(PAGE_JUMP_SIZE);
        event.preventDefault();
        break;
      }
      case 'PageUp': {
        shiftSelection(-PAGE_JUMP_SIZE);
        event.preventDefault();
        break;
      }
      case 'Home': {
        setState((prev: AppState) => ({ ...prev, selectedRow: DEFAULT_SELECTED_ROW }));
        event.preventDefault();
        break;
      }
      case 'End': {
        setState((prev: AppState) => ({
          ...prev,
          selectedRow: Math.max(activeEntries.length + INDEX_DECREMENT, DEFAULT_SELECTED_ROW),
        }));
        event.preventDefault();
        break;
      }
      case ' ': {
        void toggleFavourite();
        event.preventDefault();
        break;
      }
      case 'f': {
        jumpFavourite();
        event.preventDefault();
        break;
      }
      case 's': {
        setState((prev: AppState) => ({
          ...prev,
          search: {
            currentMatch: 0,
            inputActive: true,
            matches: [],
            query: '',
          },
        }));
        event.preventDefault();
        break;
      }
      case 'n': {
        jumpSearch(JUMP_FORWARD);
        event.preventDefault();
        break;
      }
      case 'p': {
        jumpSearch(JUMP_BACKWARD);
        event.preventDefault();
        break;
      }
      case 'd': {
        void toggleDemoMode();
        event.preventDefault();
        break;
      }
      case 'l': {
        setState((prev: AppState) => ({
          ...prev,
          showHelp: false,
          showMessages: false,
          showNlsLiveticker: !prev.showNlsLiveticker,
        }));
        if (!state.showNlsLiveticker) {
          void refreshNlsLiveticker();
        }
        event.preventDefault();
        break;
      }
      case 'm': {
        setState((prev: AppState) => ({
          ...prev,
          showHelp: false,
          showMessages: !prev.showMessages,
          showNlsLiveticker: false,
        }));
        event.preventDefault();
        break;
      }
      default: {
        break;
      }
    }
  }, [activeEntries.length, cycleView, jumpFavourite, jumpSearch, refreshNlsLiveticker, setState, shiftSelection, state.showHelp, state.showNlsLiveticker, toggleDemoMode, toggleFavourite]);

  const handleSearchKeydown = useCallback((event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      setState((prev: AppState) => ({
        ...prev,
        search: { ...prev.search, inputActive: false },
      }));
      event.preventDefault();
    } else if (event.key === 'Enter') {
      setState((prev: AppState) => ({
        ...prev,
        search: { ...prev.search, currentMatch: FIRST_MATCH_INDEX, inputActive: false },
      }));
      if (searchMatches.length > MINIMUM_LENGTH) {
        setState((prev: AppState) => ({ ...prev, selectedRow: searchMatches[FIRST_MATCH_INDEX] }));
      }
      event.preventDefault();
    } else if (event.key === 'Backspace') {
      setState((prev: AppState) => ({
        ...prev,
        search: {
          ...prev.search,
          query: prev.search.query.slice(SLICE_START_INDEX, -SLICE_REMOVE_LAST),
        },
      }));
      event.preventDefault();
    } else if (event.key.length === KEY_LENGTH_SINGLE && !event.ctrlKey && !event.metaKey) {
      setState((prev: AppState) => ({
        ...prev,
        search: {
          ...prev.search,
          query: prev.search.query + event.key,
        },
      }));
      event.preventDefault();
    }
  }, [searchMatches, setState]);

  const handleSeriesPickerKeydown = useCallback((event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      setState((prev: AppState) => ({ ...prev, showSeriesPicker: false }));
      event.preventDefault();
    } else if (event.key === 'ArrowDown' || event.key === 'j') {
      setState((prev: AppState) => ({
        ...prev,
        seriesPickerIndex: (prev.seriesPickerIndex + INDEX_INCREMENT) % ALL_SERIES.length,
      }));
      event.preventDefault();
    } else if (event.key === 'ArrowUp' || event.key === 'k') {
      setState((prev: AppState) => ({
        ...prev,
        seriesPickerIndex:
          prev.seriesPickerIndex === DEFAULT_GROUP_PICKER_INDEX
            ? ALL_SERIES.length + INDEX_DECREMENT
            : prev.seriesPickerIndex + INDEX_DECREMENT,
      }));
      event.preventDefault();
    } else if (event.key === 'Enter') {
      void chooseSeries(ALL_SERIES[state.seriesPickerIndex]);
      event.preventDefault();
    }
  }, [chooseSeries, setState, state.seriesPickerIndex]);

  // Initialize
  useEffect(() => {
    const init = async (): Promise<void> => {
      try {
        const isAuth = await fetchSessionState();
        setAuthenticated(isAuth);
        setAuthChecking(false);
        if (isAuth) {
          await initializeAppState();
        }
        setLoading(false);
      } catch (error) {
        setAuthChecking(false);
        setLoading(false);
        setLoadError(error instanceof Error ? error.message : 'initialization failed');
      }
    };
    void init();

    return (): void => {
      destroyStreams();
    };
  }, [destroyStreams, initializeAppState]);

  // Keyboard handler
  const handleKeydown = useCallback(
    (event: KeyboardEvent): void => {
      if (!authenticated) {
        return;
      }

      if (state.search.inputActive) {
        handleSearchKeydown(event);
        return;
      }

      if (state.showSeriesPicker) {
        handleSeriesPickerKeydown(event);
        return;
      }

      if (state.showGroupPicker) {
        handleGroupPickerKeydown(event);
        return;
      }

      handleMainKeydown(event);
    },
    [authenticated, handleGroupPickerKeydown, handleMainKeydown, handleSearchKeydown, handleSeriesPickerKeydown, state.search.inputActive, state.showGroupPicker, state.showSeriesPicker],
  );

  useKeyboard(handleKeydown);

  // Render helpers
  useMemo(() => getGroups(activeSnapshot?.entries ?? []), [activeSnapshot?.entries]);

  if (authChecking) {
    return <div className="loading">Checking session...</div>;
  }

  if (!authenticated) {
    return (
      <div className="login-container">
        <h1>IMSA Live Timing</h1>
        <input
          onChange={(event): void => { setLoginCode(event.target.value); }}
          placeholder="Enter access code"
          type="text"
          value={loginCode}
        />
        <button onClick={(): void => { void submitLogin(); }} type="button">
          Login
        </button>
        {loginError && <div className="error">{loginError}</div>}
      </div>
    );
  }

  if (loading) {
    return <div className="loading">Loading...</div>;
  }

  if (loadError) {
    return <div className="error">{loadError}</div>;
  }

  const favCount = useMemo(() =>
    [...state.favourites].filter((favourite) =>
      favourite.startsWith(`${state.activeSeries}|`),
    ).length,
  [state.activeSeries, state.favourites]);

  // View mode label with IIFE to avoid nested ternary and init-declarations rule
  const viewModeLabel = ((): string => {
    if (state.viewMode.kind === 'overall') {
      return 'Overall';
    }
    if (state.viewMode.kind === 'grouped') {
      return 'Grouped';
    }
    if (state.viewMode.kind === 'favourites') {
      return 'Favourites';
    }
    return `Class ${state.viewMode.index + INDEX_INCREMENT}`;
  })();

  const searchLabel = state.search.query
    ? `${state.search.query} (${state.search.currentMatch + INDEX_INCREMENT}/${searchMatches.length})`
    : '';

  const demoLabel = state.demoEnabled ? '| DEMO' : '';

  return (
    <div className="app">
      <HeaderBar
        demoLabel={demoLabel}
        errorText={state.connectionErrors[FIRST_MATCH_INDEX] ?? ''}
        favCount={favCount}
        searchCurrentMatch={state.search.currentMatch}
        searchInputActive={state.search.inputActive}
        searchLabel={searchLabel}
        searchMatches={searchMatches.length}
        searchQuery={state.search.query}
        series={state.activeSeries}
        snapshot={activeSnapshot ?? null}
        viewModeLabel={viewModeLabel}
      />

      <TimingTable
        classColors={activeSnapshot?.header.class_colors ?? {}}
        entries={activeEntries}
        selectedRow={state.selectedRow}
        series={state.activeSeries}
      />

      <HelpModal
        onClose={(): void => { setState((prev: AppState) => ({ ...prev, showHelp: false })); }}
        open={state.showHelp}
      />

      <SeriesModal
        onPick={(series): void => { void chooseSeries(series); }}
        open={state.showSeriesPicker}
        selectedIndex={state.seriesPickerIndex}
        selectedSeries={state.activeSeries}
      />

      <MessagesModal
        notices={activeSnapshot?.notices ?? []}
        onClose={(): void => { setState((prev: AppState) => ({ ...prev, showMessages: false })); }}
        open={state.showMessages}
      />

      <NlsLivetickerModal
        entries={state.nlsLiveticker.entries}
        lastError={state.nlsLiveticker.lastError}
        lastUpdateUnixMs={state.nlsLiveticker.lastUpdateUnixMs}
        onClose={(): void => { setState((prev: AppState) => ({ ...prev, showNlsLiveticker: false })); }}
        open={state.showNlsLiveticker}
      />
    </div>
  );
};
