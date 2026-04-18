// Shared frontend types mirroring backend DTOs from the Rust web API.

export type Series = 'imsa' | 'nls' | 'f1' | 'wec';

export interface TimingClassColor {
  foreground: string;
  background: string;
}

export interface TimingHeader {
  session_name: string;
  event_name: string;
  track_name: string;
  day_time: string;
  flag: string;
  time_to_go: string;
  class_colors: Record<string, TimingClassColor>;
}

export interface TimingEntry {
  position: number;
  car_number: string;
  class_name: string;
  class_rank: string;
  driver: string;
  vehicle: string;
  team: string;
  laps: string;
  gap_overall: string;
  gap_class: string;
  gap_next_in_class: string;
  last_lap: string;
  best_lap: string;
  sector_1: string;
  sector_2: string;
  sector_3: string;
  sector_4: string;
  sector_5: string;
  best_lap_no: string;
  pit: string;
  pit_stops: string;
  fastest_driver: string;
  stable_id: string;
}

export interface SeriesSnapshot {
  header: TimingHeader;
  entries: TimingEntry[];
  status: string;
  last_error: string | null;
  last_update_unix_ms: number | null;
}

export interface SnapshotResponse {
  series: Series;
  snapshot: SeriesSnapshot;
}

export interface Preferences {
  favourites: string[];
  selected_series: Series;
}

export type ViewMode =
  | { kind: 'overall' }
  | { kind: 'grouped' }
  | { kind: 'class'; index: number }
  | { kind: 'favourites' };

export const ALL_SERIES: Series[] = ['imsa', 'nls', 'f1', 'wec'];
