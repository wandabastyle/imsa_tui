// Main App component
import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react';

import { fetchSessionState, loginWithAccessCode, logoutSession, updateDemoState } from './lib/api';
import { LoadingScreen, ErrorScreen } from './lib/components/app-screens';
import { LoginScreen } from './lib/components/login-screen';
import { MainContent } from './lib/components/main-content';
import { useAppState, useKeyboard, type AppState, type UseAppStateReturn } from './lib/hooks';
import {
  handleGroupPickerKeydown,
  handleMainKeydown,
  handleSearchKeydown,
  handleSeriesPickerKeydown,
} from './lib/keyboard-handlers';
import type { Series, TimingEntry } from './lib/types';
import { classDisplayName, nextViewMode } from './lib/view-utils';

const DEFAULT_SELECTED_ROW = 0;
const INDEX_DECREMENT = -1;
const INDEX_INCREMENT = 1;

const MINIMUM_LENGTH = 0;
const ZERO_LENGTH = 0;
const NO_MATCH = -1;
const MIN_GROUP_COUNT = 1;
const LARGE_FALLBACK_RANK = 9999;
const GROUP_NAME_INDEX = 0;
const GROUP_ENTRIES_INDEX = 1;

interface GroupedSection {
  name: string;
  entries: TimingEntry[];
  start: number;
}

interface UseAppLogicReturn {
  activeEntries: TimingEntry[];
  searchMatches: number[];
  chooseSeries: (series: Series) => Promise<void>;
  cycleView: () => void;
  gapAnchorStableId: string | null;
  groups: [string, TimingEntry[]][];
  groupedSections: GroupedSection[];
  jumpFavourite: () => void;
  jumpSearch: (delta: number) => void;
  markedStableId: string | null;
  pickGroup: (index: number) => void;
  searchCurrentMatch: number;
  selectGroup: (index: number) => void;
  shiftSelection: (delta: number) => void;
  toggleDemoMode: () => Promise<void>;
  toggleFavourite: () => Promise<void>;
}

interface UseAppLogicParams {
  activeSnapshot: UseAppStateReturn['activeSnapshot'];
  destroyStreams: UseAppStateReturn['destroyStreams'];
  favouriteKey: UseAppStateReturn['favouriteKey'];
  initializeAppState: UseAppStateReturn['initializeAppState'];
  persistPreferences: UseAppStateReturn['persistPreferences'];
  setState: UseAppStateReturn['setState'];
  state: AppState;
  switchSeriesStream: UseAppStateReturn['switchSeriesStream'];
}

const useAppLogic = (params: UseAppLogicParams): UseAppLogicReturn => {
  const {
    activeSnapshot,
    destroyStreams,
    favouriteKey,
    initializeAppState,
    persistPreferences,
    setState,
    state,
    switchSeriesStream,
  } = params;

  // Grouped entries logic - ported from Svelte
  const groupedEntries = useCallback((entries: TimingEntry[]): [string, TimingEntry[]][] => {
    const grouped = new Map<string, TimingEntry[]>();
    for (const entry of entries) {
      const group = classDisplayName(entry.class_name);
      if (!grouped.has(group)) {
        grouped.set(group, []);
      }
      grouped.get(group)?.push(entry);
    }

    const groups = [...grouped.entries()];

    // Sort entries within each group by class_rank
    for (const group of groups) {
      group[GROUP_ENTRIES_INDEX].sort((firstEntry, secondEntry) => {
        const firstRank = Number(firstEntry.class_rank || LARGE_FALLBACK_RANK);
        const secondRank = Number(secondEntry.class_rank || LARGE_FALLBACK_RANK);
        return firstRank - secondRank;
      });
    }

    // Match TUI behavior: order groups by best overall position in class
    groups.sort((groupA, groupB) => {
      let aBest = Number.MAX_SAFE_INTEGER;
      for (const entry of groupA[GROUP_ENTRIES_INDEX]) {
        if (entry.position < aBest) {
          aBest = entry.position;
        }
      }
      let bBest = Number.MAX_SAFE_INTEGER;
      for (const entry of groupB[GROUP_ENTRIES_INDEX]) {
        if (entry.position < bBest) {
          bBest = entry.position;
        }
      }
      if (aBest !== bBest) {
        return aBest - bBest;
      }
      return groupA[GROUP_NAME_INDEX].localeCompare(groupB[GROUP_NAME_INDEX]);
    });

    return groups;
  }, []);

  const groups = useMemo((): [string, TimingEntry[]][] => {
    const entries: TimingEntry[] = activeSnapshot?.entries ?? [];
    return groupedEntries(entries);
  }, [activeSnapshot?.entries, groupedEntries]);

  // Calculate grouped sections with start index
  const groupedSections = useMemo((): GroupedSection[] => {
    let start = 0;
    return groups.map(([name, groupEntries]) => {
      const section = { entries: groupEntries, name, start };
      start += groupEntries.length;
      return section;
    });
  }, [groups]);

  // ViewEntries for different view modes
  const viewEntries = useMemo((): TimingEntry[] => {
    const entries: TimingEntry[] = activeSnapshot?.entries ?? [];
    if (state.viewMode.kind === 'overall') {
      return entries;
    }
    if (state.viewMode.kind === 'grouped') {
      return groups.flatMap(([, groupEntries]) => groupEntries);
    }
    if (state.viewMode.kind === 'class') {
      return groups[state.viewMode.index]?.[MIN_GROUP_COUNT] ?? [];
    }
    // Favourites
    return entries.filter((entry: TimingEntry): boolean => {
      const key: string = favouriteKey(state.activeSeries, entry.stable_id);
      return state.favourites.has(key);
    });
  }, [
    activeSnapshot?.entries,
    favouriteKey,
    state.activeSeries,
    state.favourites,
    state.viewMode,
    groups,
  ]);

  // Use viewEntries as activeEntries for backwards compatibility
  const activeEntries = viewEntries;

  const searchMatches = ((): number[] => {
    if (state.search.query === '') {
      return [];
    }
    const query = state.search.query.toLowerCase().trim();
    if (!query) {
      return [];
    }
    return viewEntries
      .map((entry: TimingEntry, index: number): number => {
        if (
          entry.car_number.toLowerCase().includes(query) ||
          entry.driver.toLowerCase().includes(query) ||
          entry.vehicle.toLowerCase().includes(query) ||
          entry.team.toLowerCase().includes(query)
        ) {
          return index;
        }
        return NO_MATCH;
      })
      .filter((idx: number): boolean => idx >= MINIMUM_LENGTH);
  })();

  // MarkedStableId for search highlight
  const searchCurrentMatch =
    searchMatches.length === MINIMUM_LENGTH
      ? MINIMUM_LENGTH
      : Math.min(state.search.currentMatch, searchMatches.length - INDEX_INCREMENT);
  const markedStableId =
    searchMatches.length === MINIMUM_LENGTH
      ? null
      : (viewEntries[searchMatches[searchCurrentMatch]]?.stable_id ?? null);

  // Gap anchor cleanup: clear when anchor leaves current view
  useEffect(() => {
    const anchorId = state.gapAnchorStableId;
    if (anchorId !== null && !viewEntries.some((entry) => entry.stable_id === anchorId)) {
      setState((prev: AppState) => ({ ...prev, gapAnchorStableId: null }));
    }
  }, [viewEntries, state.gapAnchorStableId, setState]);

  const chooseSeries = useCallback(
    async (series: Series): Promise<void> => {
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
      await persistPreferences(series);
    },
    [persistPreferences, setState, switchSeriesStream],
  );

  const cycleView = useCallback((): void => {
    setState((prev: AppState) => ({
      ...prev,
      gapAnchorStableId: null,
      selectedRow: DEFAULT_SELECTED_ROW,
      viewMode: nextViewMode(prev.viewMode, groups.length),
    }));
  }, [groups.length, setState]);

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
  }, [
    activeEntries,
    favouriteKey,
    setState,
    state.activeSeries,
    state.favourites,
    state.selectedRow,
  ]);

  const jumpSearch = useCallback(
    (delta: number): void => {
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
    },
    [searchMatches, setState],
  );

  const pickGroup = useCallback(
    (index: number): void => {
      setState((prev: AppState) => ({
        ...prev,
        gapAnchorStableId: null,
        selectedRow: DEFAULT_SELECTED_ROW,
        showGroupPicker: false,
        viewMode: { index, kind: 'class' },
      }));
    },
    [setState],
  );

  const shiftSelection = useCallback(
    (delta: number): void => {
      setState((prev: AppState) => {
        const max = Math.max(activeEntries.length + INDEX_DECREMENT, DEFAULT_SELECTED_ROW);
        const next = Math.max(DEFAULT_SELECTED_ROW, Math.min(max, prev.selectedRow + delta));
        return { ...prev, selectedRow: next };
      });
    },
    [activeEntries.length, setState],
  );

  const toggleDemoMode = useCallback(async (): Promise<void> => {
    const nextEnabled = !state.demoEnabled;
    const result = await updateDemoState(nextEnabled);
    setState((prev: AppState) => ({ ...prev, demoEnabled: result.enabled }));
    destroyStreams();
    await initializeAppState();
  }, [destroyStreams, initializeAppState, setState, state.demoEnabled]);

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
    // Persist after state update
    await persistPreferences();
  }, [
    activeEntries,
    favouriteKey,
    persistPreferences,
    setState,
    state.activeSeries,
    state.selectedRow,
  ]);

  return {
    activeEntries,
    chooseSeries,
    cycleView,
    gapAnchorStableId: state.gapAnchorStableId,
    groupedSections,
    groups,
    jumpFavourite,
    jumpSearch,
    markedStableId,
    pickGroup,
    searchCurrentMatch,
    searchMatches,
    selectGroup: pickGroup,
    shiftSelection,
    toggleDemoMode,
    toggleFavourite,
  };
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
  } = useAppState();

  const [authChecking, setAuthChecking] = useState<boolean>(true);
  const [authenticated, setAuthenticated] = useState<boolean>(false);
  const [loading, setLoading] = useState<boolean>(true);
  const [loadError, setLoadError] = useState<string>('');
  const [loginCode, setLoginCode] = useState<string>('');
  const [loginError, setLoginError] = useState<string>('');
  const seriesPickerIndexRef = useRef(state.seriesPickerIndex);

  useEffect(() => {
    seriesPickerIndexRef.current = state.seriesPickerIndex;
  }, [state.seriesPickerIndex]);

  const logic = useAppLogic({
    activeSnapshot,
    destroyStreams,
    favouriteKey,
    initializeAppState,
    persistPreferences,
    setState,
    state,
    switchSeriesStream,
  });

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
    const initResult: Promise<void> = initializeAppState();
    await initResult;
  }, [initializeAppState, loginCode]);

  const onGroupPickerKeydown = useCallback(
    (event: KeyboardEvent): void => {
      handleGroupPickerKeydown(event, {
        groupPickerIndex: state.groupPickerIndex,
        groupsLength: logic.groups.length,
        selectGroup: logic.pickGroup,
        setState,
      });
    },
    [logic.pickGroup, logic.groups.length, setState, state.groupPickerIndex],
  );

  const onMainKeydown = useCallback(
    (event: KeyboardEvent): void => {
      handleMainKeydown(event, {
        activeEntriesLength: logic.activeEntries.length,
        cycleView: logic.cycleView,
        jumpFavourite: logic.jumpFavourite,
        jumpSearch: logic.jumpSearch,
        refreshNlsLiveticker,
        setState,
        shiftSelection: logic.shiftSelection,
        showHelp: state.showHelp,
        showNlsLiveticker: state.showNlsLiveticker,
        toggleDemoMode: logic.toggleDemoMode,
        toggleFavourite: logic.toggleFavourite,
      });
    },
    [logic, refreshNlsLiveticker, setState, state.showHelp, state.showNlsLiveticker],
  );

  const onSearchKeydown = useCallback(
    (event: KeyboardEvent): void => {
      handleSearchKeydown(event, { searchMatches: logic.searchMatches, setState });
    },
    [logic.searchMatches, setState],
  );

  const onSeriesPickerKeydown = useCallback(
    (event: KeyboardEvent): void => {
      handleSeriesPickerKeydown(event, {
        chooseSeries: logic.chooseSeries,
        getSeriesPickerIndex: (): number => seriesPickerIndexRef.current,
        setState,
      });
    },
    [logic.chooseSeries, setState],
  );

  const handleKeydown = useCallback(
    (event: KeyboardEvent): void => {
      if (!authenticated) {
        return;
      }
      if (state.search.inputActive) {
        onSearchKeydown(event);
        return;
      }
      if (state.showSeriesPicker) {
        onSeriesPickerKeydown(event);
        return;
      }
      if (state.showGroupPicker) {
        onGroupPickerKeydown(event);
        return;
      }
      onMainKeydown(event);
    },
    [
      authenticated,
      onGroupPickerKeydown,
      onMainKeydown,
      onSearchKeydown,
      onSeriesPickerKeydown,
      state.search.inputActive,
      state.showGroupPicker,
      state.showSeriesPicker,
    ],
  );

  useKeyboard(handleKeydown);

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
  }, []);

  if (authChecking) {
    return <LoadingScreen message="Checking session..." />;
  }

  if (!authenticated) {
    return (
      <LoginScreen
        loginCode={loginCode}
        loginError={loginError}
        setLoginCode={setLoginCode}
        onSubmit={() => {
          void submitLogin();
        }}
      />
    );
  }

  if (loading) {
    return <LoadingScreen />;
  }
  if (loadError) {
    return <ErrorScreen error={loadError} />;
  }

  const favCount = [...state.favourites].filter((favourite) =>
    favourite.startsWith(`${state.activeSeries}|`),
  ).length;

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
    ? `Search: ${state.search.query}${state.search.inputActive ? '_' : ''} (${logic.searchMatches.length === MINIMUM_LENGTH ? MINIMUM_LENGTH : logic.searchCurrentMatch + INDEX_INCREMENT}/${logic.searchMatches.length})`
    : '';

  return (
    <MainContent
      state={state}
      activeSnapshot={activeSnapshot ?? null}
      activeEntries={logic.activeEntries}
      activeSeries={state.activeSeries}
      viewModeLabel={viewModeLabel}
      searchLabel={searchLabel}
      demoLabel={state.demoEnabled ? '| DEMO' : ''}
      favCount={favCount}
      groups={logic.groups}
      groupedSections={logic.groupedSections}
      groupPickerIndex={state.groupPickerIndex}
      gapAnchorStableId={logic.gapAnchorStableId}
      markedStableId={logic.markedStableId}
      searchMatches={logic.searchMatches}
      searchCurrentMatch={logic.searchCurrentMatch}
      onCloseGroupPicker={() => {
        setState((prev: AppState) => ({ ...prev, showGroupPicker: false }));
      }}
      onCloseHelp={() => {
        setState((prev: AppState) => ({ ...prev, showHelp: false }));
      }}
      onCloseMessages={() => {
        setState((prev: AppState) => ({ ...prev, showMessages: false }));
      }}
      onCloseNlsLiveticker={() => {
        setState((prev: AppState) => ({ ...prev, showNlsLiveticker: false }));
      }}
      onCloseSeriesPicker={() => {
        setState((prev: AppState) => ({ ...prev, showSeriesPicker: false }));
      }}
      onPickGroup={logic.pickGroup}
      onPickSeries={(series: Series) => {
        void logic.chooseSeries(series);
      }}
      onSignOut={() => {
        void logoutSession().then(() => {
          destroyStreams();
          setState((prev: AppState) => ({
            ...prev,
            gapAnchorStableId: null,
            search: { currentMatch: 0, inputActive: false, matches: [], query: '' },
            selectedRow: 0,
            showGroupPicker: false,
            showHelp: false,
            showSeriesPicker: false,
            snapshots: {},
          }));
        });
      }}
    />
  );
};
