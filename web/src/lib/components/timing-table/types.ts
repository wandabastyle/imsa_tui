import type { Series, TimingClassColor, TimingEntry } from '../../generated/web-shared';

export interface GroupSection {
  name: string;
  entries: TimingEntry[];
  start: number;
}

export interface TimingTableProps {
  classColors: Record<string, TimingClassColor | undefined>;
  entries: TimingEntry[];
  selectedRow: number;
  series: Series;
  title?: string;
  searchMatches?: number[];
  currentSearchMatch?: number;
  gapAnchorStableId?: string | null;
  favourites?: Set<string>;
  groupedSections?: GroupSection[];
  isGroupedMode?: boolean;
  markedStableId?: string | null;
  loading?: boolean;
}

export interface PitTracker {
  inPit: boolean;
  inUntil: number;
  outUntil: number;
}

export interface GapAnchorInfo {
  laps: number;
  position: number;
  stableId: string;
}

export interface ParsedGap {
  kind: 'time' | 'laps';
  value: number;
}
