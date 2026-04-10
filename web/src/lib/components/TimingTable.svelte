<script lang="ts">
  // Shared leaderboard table renderer for overall, grouped, class, and favourites views.

  import { afterUpdate } from 'svelte';
  import type { Series, TimingEntry } from '$lib/types';

  interface GroupSection {
    name: string;
    entries: TimingEntry[];
    start: number;
  }

  export let title = 'Overall';
  export let series: Series;
  export let entries: TimingEntry[] = [];
  export let groupedSections: GroupSection[] = [];
  export let isGroupedMode = false;
  export let selectedRow = 0;
  export let markedStableId: string | null = null;
  export let favourites = new Set<string>();
  let scrollContainer: HTMLDivElement | null = null;

  const columnsBySeries: Record<Series, string[]> = {
    imsa: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Laps', 'Gap O', 'Gap C', 'Next C', 'Last', 'Best', 'BL#', 'Pit', 'Stop', 'Fastest Driver'],
    nls: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Team', 'Laps', 'Gap', 'Last', 'Best'],
    f1: ['Pos', '#', 'Driver', 'Team', 'Laps', 'Gap', 'Int', 'Last', 'Best', 'Pit', 'Stops', 'PIC']
  };

  const widthBySeries: Record<Series, string[]> = {
    // Mirrors the TUI fixed-column intent so grouped sections line up perfectly.
    imsa: ['4ch', '5ch', '7ch', '4ch', '24ch', '20ch', '6ch', '11ch', '11ch', '11ch', '10ch', '10ch', '5ch', '5ch', '5ch', '18ch'],
    nls: ['4ch', '5ch', '9ch', '5ch', '24ch', '16ch', '20ch', '7ch', '11ch', '10ch', '10ch'],
    f1: ['4ch', '5ch', '26ch', '16ch', '7ch', '11ch', '11ch', '10ch', '10ch', '5ch', '5ch', '7ch']
  };

  function favouriteFlag(entry: TimingEntry): string {
    return favourites.has(`${series}|${normalizeStableId(entry.stable_id)}`) ? '★ ' : '';
  }

  function normalizeStableId(stableId: string): string {
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

  function cells(entry: TimingEntry): string[] {
    if (series === 'imsa') {
      return [
        String(entry.position),
        `${favouriteFlag(entry)}${entry.car_number}`,
        entry.class_name,
        entry.class_rank,
        entry.driver,
        entry.vehicle,
        entry.laps,
        entry.gap_overall,
        entry.gap_class,
        entry.gap_next_in_class,
        entry.last_lap,
        entry.best_lap,
        entry.best_lap_no,
        entry.pit,
        entry.pit_stops,
        entry.fastest_driver
      ];
    }
    if (series === 'nls') {
      return [
        String(entry.position),
        `${favouriteFlag(entry)}${entry.car_number}`,
        entry.class_name,
        entry.class_rank,
        entry.driver,
        entry.vehicle,
        entry.team,
        entry.laps,
        entry.gap_overall,
        entry.last_lap,
        entry.best_lap
      ];
    }
    return [
      String(entry.position),
      `${favouriteFlag(entry)}${entry.car_number}`,
      entry.driver,
      entry.team,
      entry.laps,
      entry.gap_overall,
      entry.gap_class,
      entry.last_lap,
      entry.best_lap,
      entry.pit,
      entry.pit_stops,
      entry.class_rank
    ];
  }

  function rowClass(entry: TimingEntry): string {
    const className = entry.class_name.replaceAll(' ', '').replaceAll('_', '').toUpperCase();
    if (className === 'GTP') return 'class-gtp';
    if (className === 'LMP2') return 'class-lmp2';
    if (className === 'GTDPRO') return 'class-gtdpro';
    if (className === 'GTD') return 'class-gtd';
    return '';
  }

  function pitCellClass(column: string, value: string): string {
    if ((column === 'Pit' || column === 'PIT') && value.toLowerCase() === 'yes') {
      return 'pit-active';
    }
    if ((column === 'Stop' || column === 'Stops') && value !== '-' && value !== '0') {
      return 'stops-hot';
    }
    return '';
  }

  // Keep keyboard navigation usable by ensuring the selected row stays visible.
  afterUpdate(() => {
    const container = scrollContainer;
    if (!container) {
      return;
    }
    const selected = container.querySelector('tr.selected') as HTMLElement | null;
    if (!selected) {
      return;
    }
    selected.scrollIntoView({ block: 'nearest' });
  });
</script>

<section class="table-wrap">
  <div class="table-title">{title}</div>
  <div class="table-scroll" bind:this={scrollContainer}>
    {#if isGroupedMode}
      <div class="group-stack">
        {#if groupedSections.length === 0}
          <p class="empty">No grouped class data available yet.</p>
        {:else}
          {#each groupedSections as section}
            <section class="group-section">
              <div class="group-title">{section.name} ({section.entries.length} cars)</div>
              <table>
                <colgroup>
                  {#each widthBySeries[series] as width}
                    <col style={`width:${width}`} />
                  {/each}
                </colgroup>
                <thead>
                  <tr>
                    {#each columnsBySeries[series] as column}
                      <th>{column}</th>
                    {/each}
                  </tr>
                </thead>
                <tbody>
                  {#each section.entries as entry, index}
                    <tr class={`${rowClass(entry)} ${section.start + index === selectedRow ? 'selected' : ''} ${entry.stable_id === markedStableId ? 'search-mark' : ''}`}>
                      {#each cells(entry) as cell, colIndex}
                        <td class={pitCellClass(columnsBySeries[series][colIndex], cell)}>{cell}</td>
                      {/each}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </section>
          {/each}
        {/if}
      </div>
    {:else}
      <table>
        <colgroup>
          {#each widthBySeries[series] as width}
            <col style={`width:${width}`} />
          {/each}
        </colgroup>
        <thead>
          <tr>
            {#each columnsBySeries[series] as column}
              <th>{column}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#if entries.length === 0}
            <tr>
              <td colspan={columnsBySeries[series].length}>No timing data yet.</td>
            </tr>
          {:else}
            {#each entries as entry, index}
              <tr class={`${rowClass(entry)} ${index === selectedRow ? 'selected' : ''} ${entry.stable_id === markedStableId ? 'search-mark' : ''}`}>
                {#each cells(entry) as cell, colIndex}
                  <td class={pitCellClass(columnsBySeries[series][colIndex], cell)}>{cell}</td>
                {/each}
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    {/if}
  </div>
</section>

<style>
  .table-wrap {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: #071628;
    min-height: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .table-title {
    padding: 0.35rem 0.55rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 0.9rem;
  }

  .table-scroll {
    overflow: auto;
  }

  .group-stack {
    padding: 0.25rem;
    display: grid;
    gap: 0.45rem;
  }

  .group-section {
    border: 1px solid #355378;
    border-radius: 5px;
    overflow: hidden;
    background: #0a1a2d;
  }

  .group-title {
    padding: 0.3rem 0.5rem;
    font-size: 0.84rem;
    color: var(--text-dim);
    border-bottom: 1px solid #355378;
    background: #11263d;
  }

  .empty {
    color: var(--text-dim);
    padding: 0.65rem;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
    table-layout: fixed;
  }

  th,
  td {
    border-bottom: 1px solid #24344a;
    padding: 0.25rem 0.38rem;
    white-space: nowrap;
  }

  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: #182436;
    text-align: left;
  }

  tr.selected {
    background: #244f82;
    font-weight: 700;
  }

  tr.search-mark:not(.selected) {
    background: #113158;
    box-shadow: inset 0 0 0 1px #2a79c7;
  }

  td.pit-active {
    color: #ffd166;
    font-weight: 700;
  }

  td.stops-hot {
    color: #ff8a65;
    font-weight: 700;
  }

  tr.class-lmp2 {
    color: #3f90da;
  }

  tr.class-gtdpro {
    color: #d22630;
  }

  tr.class-gtd {
    color: #00a651;
  }

  tr.class-gtp {
    color: #e9eef8;
  }

  tr.selected {
    color: #ffffff;
  }
</style>
