<script lang="ts">
  // Shared leaderboard table renderer for overall, grouped, class, and favourites views.

  import { afterUpdate, onMount } from 'svelte';
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
  export let gapAnchorStableId: string | null = null;
  let scrollContainer: HTMLDivElement | null = null;
  let marqueeTick = 0;
  let gapAnchorEntry: TimingEntry | null = null;
  const pitTrackers = new Map<string, { inPit: boolean; inUntil: number; outUntil: number }>();

  const columnsBySeries: Record<Series, string[]> = {
    imsa: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Laps', 'Gap O', 'Gap C', 'Next C', 'Last', 'Best', 'BL#', 'Pit', 'Stop', 'Fastest Driver'],
    nls: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Team', 'Laps', 'Gap', 'Last', 'Best', 'S1', 'S2', 'S3', 'S4', 'S5'],
    f1: ['Pos', '#', 'Driver', 'Team', 'Laps', 'Gap', 'Int', 'Last', 'Best', 'Pit', 'Stops', 'PIC']
  };

  const widthBySeries: Record<Series, string[]> = {
    // Mirrors the TUI fixed-column intent so grouped sections line up perfectly.
    imsa: ['4ch', '7ch', '7ch', '4ch', '24ch', '20ch', '6ch', '11ch', '11ch', '11ch', '10ch', '10ch', '5ch', '5ch', '5ch', '18ch'],
    nls: ['4ch', '7ch', '9ch', '5ch', '14ch', '26ch', '32ch', '4ch', '11ch', '9ch', '9ch', '9ch', '9ch', '9ch', '9ch', '9ch'],
    f1: ['4ch', '7ch', '26ch', '16ch', '7ch', '11ch', '11ch', '10ch', '10ch', '5ch', '5ch', '7ch']
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
        entry.best_lap,
        entry.sector_1,
        entry.sector_2,
        entry.sector_3,
        entry.sector_4,
        entry.sector_5
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

  function pitSignalActive(entry: TimingEntry): boolean {
    if (series === 'imsa' || series === 'f1') {
      return entry.pit.toLowerCase() === 'yes';
    }
    return entry.sector_5.trim().toUpperCase() === 'PIT';
  }

  function rowPitPhase(entry: TimingEntry): string {
    const now = Date.now();
    const key = entry.stable_id;
    const tracker = pitTrackers.get(key) ?? { inPit: false, inUntil: 0, outUntil: 0 };
    const signal = pitSignalActive(entry);

    if (signal) {
      if (!tracker.inPit) {
        tracker.inPit = true;
        tracker.inUntil = now + 1200;
      }
      tracker.outUntil = 0;
    } else if (tracker.inPit) {
      tracker.inPit = false;
      tracker.inUntil = 0;
      tracker.outUntil = now + 1800;
    }

    pitTrackers.set(key, tracker);

    if (tracker.inPit && now <= tracker.inUntil) {
      return 'pit-in';
    }
    if (tracker.inPit) {
      return 'pit-row';
    }
    if (now <= tracker.outUntil) {
      return 'pit-out';
    }
    return '';
  }

  function pitCellClass(column: string, value: string): string {
    if ((column === 'Pit' || column === 'PIT') && value.toLowerCase() === 'yes') {
      return 'pit-active';
    }
    if (column === 'S5') {
      const upper = value.toUpperCase();
      if (upper === 'PIT') {
        return 'pit-active';
      }
    }
    if ((column === 'Stop' || column === 'Stops') && value !== '-' && value !== '0') {
      return 'stops-hot';
    }
    return '';
  }

  function compactColumnClass(column: string): string {
    if (['Laps', 'Gap', 'Last', 'Best', 'S1', 'S2', 'S3', 'S4', 'S5'].includes(column)) {
      return 'tight-col';
    }
    return '';
  }

  function isRelativeGapColumn(column: string): boolean {
    return column === 'Gap O' || column === 'Gap C' || column === 'Next C' || column === 'Gap' || column === 'Int';
  }

  function gapRawForColumn(entry: TimingEntry, column: string): string {
    if (column === 'Gap O' || column === 'Gap') return entry.gap_overall;
    if (column === 'Gap C' || column === 'Int') return entry.gap_class;
    if (column === 'Next C') return entry.gap_next_in_class;
    return '';
  }

  function anchorGapLabel(entry: TimingEntry): string {
    const laps = entry.laps.trim();
    return /^\d+$/.test(laps) ? `----LAP ${laps}` : '----';
  }

  type ParsedGap = { kind: 'time'; ms: number } | { kind: 'laps'; laps: number };

  function parseGapValue(raw: string): ParsedGap | null {
    const trimmed = raw.trim();
    if (!trimmed || trimmed === '-' || /^----LAP/i.test(trimmed) || /^leader$/i.test(trimmed)) {
      return null;
    }

    if (/lap/i.test(trimmed)) {
      const match = trimmed.match(/[+-]?\d+/);
      if (!match) return null;
      const laps = Number.parseInt(match[0], 10);
      if (Number.isNaN(laps)) return null;
      return { kind: 'laps', laps };
    }

    const normalized = trimmed.replace(/^\+/, '');
    if (!/^[0-9:.]+$/.test(normalized)) return null;

    let seconds = 0;
    if (normalized.includes(':')) {
      const [minsPart, secsPart] = normalized.split(':');
      const mins = Number.parseInt(minsPart, 10);
      const secs = Number.parseFloat(secsPart);
      if (Number.isNaN(mins) || Number.isNaN(secs)) return null;
      seconds = mins * 60 + secs;
    } else {
      const secs = Number.parseFloat(normalized);
      if (Number.isNaN(secs)) return null;
      seconds = secs;
    }

    return { kind: 'time', ms: Math.round(seconds * 1000) };
  }

  function formatTimeDelta(msDelta: number): string {
    const sign = msDelta >= 0 ? '+' : '-';
    const abs = Math.abs(msDelta);
    const minutes = Math.floor(abs / 60000);
    const seconds = (abs % 60000) / 1000;
    if (minutes > 0) {
      return `${sign}${minutes}:${seconds.toFixed(3).padStart(6, '0')}`;
    }
    return `${sign}${seconds.toFixed(3)}`;
  }

  function formatLapDelta(lapsDelta: number): string {
    const sign = lapsDelta >= 0 ? '+' : '-';
    const abs = Math.abs(lapsDelta);
    return `${sign}${abs} ${abs === 1 ? 'LAP' : 'LAPS'}`;
  }

  function relativeGapCell(entry: TimingEntry, column: string, current: string): string {
    if (!gapAnchorStableId || !isRelativeGapColumn(column)) {
      return current;
    }

    if (entry.stable_id === gapAnchorStableId) {
      return anchorGapLabel(entry);
    }

    const anchor = gapAnchorEntry;
    if (!anchor) return current;

    const rowLaps = Number.parseInt(entry.laps.trim(), 10);
    const anchorLaps = Number.parseInt(anchor.laps.trim(), 10);
    if (!Number.isNaN(rowLaps) && !Number.isNaN(anchorLaps) && rowLaps !== anchorLaps) {
      return formatLapDelta(anchorLaps - rowLaps);
    }

    const rowGap = parseGapValue(current);
    const anchorGap = parseGapValue(gapRawForColumn(anchor, column));
    if (!rowGap || !anchorGap) return current;

    if (rowGap.kind === 'time' && anchorGap.kind === 'time') {
      return formatTimeDelta(rowGap.ms - anchorGap.ms);
    }
    if (rowGap.kind === 'laps' && anchorGap.kind === 'laps') {
      return formatLapDelta(rowGap.laps - anchorGap.laps);
    }
    return current;
  }

  function renderCell(entry: TimingEntry, cell: string, colIndex: number, selected: boolean): string {
    const column = columnsBySeries[series][colIndex];
    const relative = relativeGapCell(entry, column, cell);
    return marqueeCellText(relative, colIndex, selected, marqueeTick);
  }

  function columnWidthChars(colIndex: number): number {
    const raw = widthBySeries[series][colIndex] ?? '12ch';
    const match = raw.match(/(\d+)ch/);
    return match ? Number.parseInt(match[1], 10) : 12;
  }

  function marqueeCellText(value: string, colIndex: number, selected: boolean, tick: number): string {
    if (!selected) return value;
    const width = columnWidthChars(colIndex);
    const chars = Array.from(value);
    if (chars.length <= width) return value;

    const gap = 3;
    const cycle = chars.length + gap;
    const offset = tick % cycle;
    if (offset < chars.length) {
      return `${chars.slice(offset).join('')}   ${chars.slice(0, offset).join('')}`;
    }
    return `${' '.repeat(offset - chars.length)}${value}`;
  }

  onMount(() => {
    const timer = window.setInterval(() => {
      marqueeTick = (marqueeTick + 1) % 10000;
    }, 240);
    return () => window.clearInterval(timer);
  });

  $: gapAnchorEntry = gapAnchorStableId
    ? entries.find((candidate) => candidate.stable_id === gapAnchorStableId) ?? null
    : null;

  $: {
    const currentIds = new Set(entries.map((entry) => entry.stable_id));
    for (const key of pitTrackers.keys()) {
      if (!currentIds.has(key)) {
        pitTrackers.delete(key);
      }
    }
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
                        <th class={compactColumnClass(column)}>{column}</th>
                      {/each}
                  </tr>
                </thead>
                <tbody>
                  {#each section.entries as entry, index}
                    <tr class={`${rowClass(entry)} ${rowPitPhase(entry)} ${section.start + index === selectedRow ? 'selected' : ''} ${entry.stable_id === markedStableId ? 'search-mark' : ''}`}>
                      {#each cells(entry) as cell, colIndex}
                        <td class={`${pitCellClass(columnsBySeries[series][colIndex], cell)} ${compactColumnClass(columnsBySeries[series][colIndex])}`.trim()}>{renderCell(entry, cell, colIndex, section.start + index === selectedRow)}</td>
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
              <th class={compactColumnClass(column)}>{column}</th>
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
              <tr class={`${rowClass(entry)} ${rowPitPhase(entry)} ${index === selectedRow ? 'selected' : ''} ${entry.stable_id === markedStableId ? 'search-mark' : ''}`}>
                {#each cells(entry) as cell, colIndex}
                  <td class={`${pitCellClass(columnsBySeries[series][colIndex], cell)} ${compactColumnClass(columnsBySeries[series][colIndex])}`.trim()}>{renderCell(entry, cell, colIndex, index === selectedRow)}</td>
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
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 0;
  }

  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: #182436;
    text-align: left;
  }

  th.tight-col,
  td.tight-col {
    padding-left: 0.2rem;
    padding-right: 0.2rem;
  }

  tr.selected {
    background: #244f82;
    font-weight: 700;
  }

  tr.search-mark:not(.selected) {
    background: #113158;
    box-shadow: inset 0 0 0 1px #2a79c7;
  }

  tr.pit-row:not(.selected) {
    color: #ffd166;
    font-weight: 700;
    background: rgba(130, 97, 20, 0.16);
  }

  tr.pit-row.selected {
    color: #ffe08a;
  }

  tr.pit-in:not(.selected) {
    color: #7fdfff;
    font-weight: 700;
    background: rgba(37, 125, 184, 0.28);
  }

  tr.pit-in.selected {
    color: #b9ecff;
    background: rgba(52, 150, 214, 0.38);
  }

  tr.pit-out:not(.selected) {
    color: #f5b3ff;
    font-weight: 700;
    background: rgba(133, 66, 170, 0.18);
  }

  tr.pit-out.selected {
    color: #ffd8ff;
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
