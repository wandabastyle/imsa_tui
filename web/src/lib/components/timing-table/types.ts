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
  minRowsPerGroup?: number;
  setMinRowsPerGroup?: (value: number) => void;
}

export interface GroupedTableProps {
  scrollContainerRef: React.RefObject<HTMLDivElement | null>;
  groupedSections: GroupSection[];
  columns: string[];
  selectedRow: number;
  series: Series;
  classColors: Record<string, TimingClassColor | undefined>;
  gapAnchor: GapAnchorInfo | null;
  pitTrackers: Map<string, PitTracker>;
  favourites: Set<string>;
  markedStableId: string | null;
  title: string;
  loading: boolean;
  viewportHeight: number | null;
  rowHeight: number | null;
  minRowsPerGroup: number;
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
