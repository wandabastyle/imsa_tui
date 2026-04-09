<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import HeaderBar from '$lib/components/HeaderBar.svelte';
  import HelpModal from '$lib/components/HelpModal.svelte';
  import GroupModal from '$lib/components/GroupModal.svelte';
  import SeriesModal from '$lib/components/SeriesModal.svelte';
  import TimingTable from '$lib/components/TimingTable.svelte';
  import { appState, destroyStreams, favouriteKey, initializeAppState, persistPreferences } from '$lib/stores/app';
  import { ALL_SERIES, type Series, type TimingEntry, type ViewMode } from '$lib/types';

  let loading = true;
  let loadError = '';

  let cleanupKeys = () => {};

  onMount(async () => {
    try {
      await initializeAppState();
      loading = false;
    } catch (error) {
      loading = false;
      loadError = error instanceof Error ? error.message : 'initialization failed';
      return;
    }

    const handler = (event: KeyboardEvent) => {
      if ($appState.search.inputActive) {
        if (event.key === 'Escape') {
          appState.update((state) => ({ ...state, search: { ...state.search, inputActive: false } }));
          event.preventDefault();
          return;
        }
        if (event.key === 'Enter') {
          appState.update((state) => ({
            ...state,
            search: { ...state.search, inputActive: false, currentMatch: 0 }
          }));
          if (searchMatches.length > 0) {
            appState.update((state) => ({ ...state, selectedRow: searchMatches[0] }));
          }
          event.preventDefault();
          return;
        }
        if (event.key === 'Backspace') {
          appState.update((state) => ({
            ...state,
            search: { ...state.search, query: state.search.query.slice(0, -1) }
          }));
          event.preventDefault();
          return;
        }
        if (event.key.length === 1 && !event.ctrlKey && !event.metaKey) {
          appState.update((state) => ({
            ...state,
            search: { ...state.search, query: `${state.search.query}${event.key}` }
          }));
          event.preventDefault();
          return;
        }
      }

      if ($appState.showSeriesPicker) {
        if (event.key === 'Escape') {
          appState.update((state) => ({ ...state, showSeriesPicker: false }));
          event.preventDefault();
          return;
        }
        if (event.key === 'ArrowDown' || event.key === 'j') {
          appState.update((state) => ({
            ...state,
            seriesPickerIndex: (state.seriesPickerIndex + 1) % ALL_SERIES.length
          }));
          event.preventDefault();
          return;
        }
        if (event.key === 'ArrowUp' || event.key === 'k') {
          appState.update((state) => ({
            ...state,
            seriesPickerIndex:
              state.seriesPickerIndex === 0 ? ALL_SERIES.length - 1 : state.seriesPickerIndex - 1
          }));
          event.preventDefault();
          return;
        }
        if (event.key === 'Enter') {
          void chooseSeries(ALL_SERIES[$appState.seriesPickerIndex]);
          event.preventDefault();
          return;
        }
      }

      if ($appState.showGroupPicker) {
        if (event.key === 'Escape') {
          appState.update((state) => ({ ...state, showGroupPicker: false }));
          event.preventDefault();
          return;
        }
        if (event.key === 'ArrowDown' || event.key === 'j') {
          appState.update((state) => ({
            ...state,
            groupPickerIndex: groups.length === 0 ? 0 : (state.groupPickerIndex + 1) % groups.length
          }));
          event.preventDefault();
          return;
        }
        if (event.key === 'ArrowUp' || event.key === 'k') {
          appState.update((state) => ({
            ...state,
            groupPickerIndex:
              groups.length === 0
                ? 0
                : state.groupPickerIndex === 0
                  ? groups.length - 1
                  : state.groupPickerIndex - 1
          }));
          event.preventDefault();
          return;
        }
        if (event.key === 'Enter') {
          pickGroup($appState.groupPickerIndex);
          event.preventDefault();
          return;
        }
      }

      switch (event.key) {
        case 'Escape':
          if ($appState.showHelp) {
            appState.update((state) => ({ ...state, showHelp: false }));
          }
          break;
        case 'h':
        case '?':
          appState.update((state) => ({ ...state, showHelp: !state.showHelp }));
          break;
        case 'g':
          cycleView();
          break;
        case 'G':
          appState.update((state) => ({
            ...state,
            showGroupPicker: true,
            groupPickerIndex:
              state.viewMode.kind === 'class' ? Math.min(state.viewMode.index, groups.length - 1) : 0
          }));
          break;
        case 'o':
          appState.update((state) => ({ ...state, viewMode: { kind: 'overall' }, selectedRow: 0 }));
          break;
        case 't':
          appState.update((state) => ({
            ...state,
            showSeriesPicker: true,
            showGroupPicker: false,
            seriesPickerIndex: ALL_SERIES.indexOf(state.activeSeries)
          }));
          break;
        case 'ArrowDown':
        case 'j':
          shiftSelection(1);
          break;
        case 'ArrowUp':
        case 'k':
          shiftSelection(-1);
          break;
        case 'PageDown':
          shiftSelection(10);
          break;
        case 'PageUp':
          shiftSelection(-10);
          break;
        case 'Home':
          appState.update((state) => ({ ...state, selectedRow: 0 }));
          break;
        case 'End':
          appState.update((state) => ({ ...state, selectedRow: Math.max(viewEntries.length - 1, 0) }));
          break;
        case ' ':
          void toggleFavourite();
          break;
        case 'f':
          jumpFavourite();
          break;
        case 's':
          appState.update((state) => ({
            ...state,
            search: { query: '', matches: [], currentMatch: 0, inputActive: true }
          }));
          break;
        case 'n':
          jumpSearch(1);
          break;
        case 'p':
          jumpSearch(-1);
          break;
        case 'r':
          appState.update((state) => ({
            ...state,
            demoFlag: {
              enabled: true,
              index: state.demoFlag.enabled ? (state.demoFlag.index + 1) % 5 : 0
            }
          }));
          break;
        case '0':
          appState.update((state) => ({ ...state, demoFlag: { ...state.demoFlag, enabled: false } }));
          break;
        default:
          return;
      }
      event.preventDefault();
    };

    window.addEventListener('keydown', handler);
    cleanupKeys = () => window.removeEventListener('keydown', handler);
  });

  onDestroy(() => {
    cleanupKeys();
    destroyStreams();
  });

  function normalizeClassName(value: string): string {
    return value.replaceAll(' ', '').replaceAll('_', '').toUpperCase();
  }

  function classDisplayName(value: string): string {
    const normalized = normalizeClassName(value);
    if (normalized === 'GTDPRO') {
      return 'GTD PRO';
    }
    return value.trim() || '-';
  }

  function groupedEntries(entries: TimingEntry[]): [string, TimingEntry[]][] {
    const grouped = new Map<string, TimingEntry[]>();
    for (const entry of entries) {
      const group = classDisplayName(entry.class_name);
      if (!grouped.has(group)) {
        grouped.set(group, []);
      }
      grouped.get(group)?.push(entry);
    }

    const groups = Array.from(grouped.entries());

    for (const group of groups) {
      group[1].sort((a, b) => Number(a.class_rank || 9999) - Number(b.class_rank || 9999));
    }

    // Match TUI behavior: order groups by best overall position in class.
    groups.sort((a, b) => {
      const aBest = a[1].reduce((min, entry) => Math.min(min, entry.position), Number.MAX_SAFE_INTEGER);
      const bBest = b[1].reduce((min, entry) => Math.min(min, entry.position), Number.MAX_SAFE_INTEGER);
      if (aBest !== bBest) return aBest - bBest;
      return a[0].localeCompare(b[0]);
    });

    return groups;
  }

  function nextViewMode(current: ViewMode, groupCount: number): ViewMode {
    if (groupCount === 0) {
      if (current.kind === 'overall') return { kind: 'grouped' };
      if (current.kind === 'grouped') return { kind: 'favourites' };
      return { kind: 'overall' };
    }

    if (current.kind === 'overall') return { kind: 'grouped' };
    if (current.kind === 'grouped') return { kind: 'class', index: 0 };
    if (current.kind === 'class') {
      return current.index + 1 < groupCount
        ? { kind: 'class', index: current.index + 1 }
        : { kind: 'favourites' };
    }
    return { kind: 'overall' };
  }

  function cycleView(): void {
    appState.update((state) => {
      const groups = groupedEntries(activeEntries);
      return { ...state, viewMode: nextViewMode(state.viewMode, groups.length), selectedRow: 0 };
    });
  }

  function shiftSelection(delta: number): void {
    appState.update((state) => {
      const max = Math.max(viewEntries.length - 1, 0);
      const next = Math.max(0, Math.min(max, state.selectedRow + delta));
      return { ...state, selectedRow: next };
    });
  }

  async function toggleFavourite(): Promise<void> {
    const selected = viewEntries[$appState.selectedRow];
    if (!selected) return;
    const key = favouriteKey($appState.activeSeries, selected.stable_id);
    appState.update((state) => {
      const next = new Set(state.favourites);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return { ...state, favourites: next };
    });
    await persistPreferences();
  }

  function jumpFavourite(): void {
    if (viewEntries.length === 0) return;
    const start = $appState.selectedRow;
    for (let offset = 1; offset <= viewEntries.length; offset += 1) {
      const idx = (start + offset) % viewEntries.length;
      const key = favouriteKey($appState.activeSeries, viewEntries[idx].stable_id);
      if ($appState.favourites.has(key)) {
        appState.update((state) => ({ ...state, selectedRow: idx }));
        return;
      }
    }
  }

  function entryMatchesSearch(entry: TimingEntry, query: string): boolean {
    const needle = query.trim().toLowerCase();
    if (!needle) return false;
    return (
      entry.car_number.toLowerCase().includes(needle) ||
      entry.driver.toLowerCase().includes(needle) ||
      entry.vehicle.toLowerCase().includes(needle) ||
      entry.team.toLowerCase().includes(needle)
    );
  }

  function jumpSearch(delta: number): void {
    if (searchMatches.length === 0) return;
    appState.update((state) => {
      const start = Math.min(state.search.currentMatch, searchMatches.length - 1);
      const next = (start + delta + searchMatches.length) % searchMatches.length;
      return {
        ...state,
        selectedRow: searchMatches[next],
        search: { ...state.search, currentMatch: next }
      };
    });
  }

  async function chooseSeries(series: Series): Promise<void> {
    appState.update((state) => ({
      ...state,
      activeSeries: series,
      showSeriesPicker: false,
      showGroupPicker: false,
      seriesPickerIndex: ALL_SERIES.indexOf(series),
      viewMode: { kind: 'overall' },
      selectedRow: 0
    }));
    await persistPreferences();
  }

  function pickGroup(index: number): void {
    if (groups.length === 0) {
      appState.update((state) => ({ ...state, showGroupPicker: false }));
      return;
    }
    const bounded = Math.max(0, Math.min(index, groups.length - 1));
    appState.update((state) => ({
      ...state,
      viewMode: { kind: 'class', index: bounded },
      selectedRow: 0,
      showGroupPicker: false,
      groupPickerIndex: bounded
    }));
  }

  $: activeSnapshot = $appState.snapshots[$appState.activeSeries] ?? null;
  $: activeEntries = activeSnapshot?.entries ?? [];
  $: groups = groupedEntries(activeEntries);
  $: groupedSections = (() => {
    let start = 0;
    return groups.map(([name, entries]) => {
      const section = { name, entries, start };
      start += entries.length;
      return section;
    });
  })();
  $: favouriteEntries = activeEntries.filter((entry) =>
    $appState.favourites.has(favouriteKey($appState.activeSeries, entry.stable_id))
  );
  $: viewEntries = (() => {
    const mode = $appState.viewMode;
    if (mode.kind === 'overall') return activeEntries;
    if (mode.kind === 'grouped') return groups.flatMap(([, entries]) => entries);
    if (mode.kind === 'class') return groups[mode.index]?.[1] ?? [];
    return favouriteEntries;
  })();
  $: searchMatches = viewEntries
    .map((entry, index) => (entryMatchesSearch(entry, $appState.search.query) ? index : -1))
    .filter((idx) => idx >= 0);
  $: searchCurrentMatch =
    searchMatches.length === 0
      ? 0
      : Math.min($appState.search.currentMatch, searchMatches.length - 1);
  $: markedStableId =
    searchMatches.length === 0 ? null : (viewEntries[searchMatches[searchCurrentMatch]]?.stable_id ?? null);
  $: viewModeLabel =
    $appState.viewMode.kind === 'class'
      ? `Class ${groups[$appState.viewMode.index]?.[0] ?? ''}`
      : $appState.viewMode.kind[0].toUpperCase() + $appState.viewMode.kind.slice(1);
  $: searchLabel = $appState.search.query
    ? `Search: ${$appState.search.query}${$appState.search.inputActive ? '_' : ''} (${searchMatches.length === 0 ? 0 : searchCurrentMatch + 1}/${searchMatches.length})`
    : '';
  $: demoLabel = $appState.demoFlag.enabled ? `| DEMO ${$appState.demoFlag.index}` : '';
  $: favCountForSeries = Array.from($appState.favourites).filter((value) =>
    value.startsWith(`${$appState.activeSeries}|`)
  ).length;
</script>

<main>
  {#if loading}
    <p>Loading web UI...</p>
  {:else if loadError}
    <p>Failed to initialize: {loadError}</p>
  {:else}
    <HeaderBar
      snapshot={activeSnapshot}
      viewModeLabel={viewModeLabel}
      favCount={favCountForSeries}
      searchLabel={searchLabel}
      demoLabel={demoLabel}
      errorText={activeSnapshot?.last_error ?? ''}
    />

    <TimingTable
      title={viewModeLabel}
      series={$appState.activeSeries}
      entries={viewEntries}
      groupedSections={groupedSections}
      isGroupedMode={$appState.viewMode.kind === 'grouped'}
      selectedRow={$appState.selectedRow}
      markedStableId={markedStableId}
      favourites={$appState.favourites}
    />

    <HelpModal open={$appState.showHelp} />
    <GroupModal
      open={$appState.showGroupPicker}
      groups={groups.map(([name]) => name)}
      selectedIndex={$appState.groupPickerIndex}
      onPick={pickGroup}
    />
    <SeriesModal
      open={$appState.showSeriesPicker}
      selectedSeries={ALL_SERIES[$appState.seriesPickerIndex]}
      onPick={chooseSeries}
    />
  {/if}
</main>

<style>
  main {
    max-width: 100%;
    height: 100dvh;
    min-height: 100dvh;
    padding: 0.7rem;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  p {
    color: var(--text-dim);
  }

  @media (max-width: 768px) {
    main {
      height: 100dvh;
      padding: 0.4rem;
    }
  }
</style>
