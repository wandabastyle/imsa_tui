export const PIT_IN_DURATION_MS = 1200;
export const PIT_OUT_DURATION_MS = 1800;
export const MARQUEE_GAP = 3;
export const MARQUEE_CYCLE_MAX = 10_000;
export const TICK_INTERVAL_MS = 240;
export const DEFAULT_COL_WIDTH_CH = 12;
export const EXTRA_WIDTH_FACTOR = 1;
export const INITIAL_INDEX = -1;
export const FIRST_ROW = 0;
export const RADIX_DECIMAL = 10;
export const MATCH_GROUP_ONE = 1;

export const RELATIVE_GAP_COLUMNS = new Set(['Gap', 'Gap O', 'Gap C', 'Next C', 'Int']);
export const LONG_TEXT_COLUMNS = new Set(['Driver', 'Vehicle', 'Team', 'Fastest Driver']);

export const WEC_STATIC_COLORS: Record<string, string> = {
  HYPER: '#e21e19',
  HYPERCAR: '#e21e19',
  INV: '#ffffff',
  LMGT3: '#0b9314',
  LMGTE: '#ffa912',
  LMH: '#e21e19',
  LMP1: '#ff1053',
  LMP2: '#3f90da',
};

export const STANDARD_STATIC_COLORS: Record<string, string> = {
  GTD: '#00a651',
  'GTD-PRO': '#d22630',
  GTP: '#ffffff',
  LMGT3: '#1e90ff',
  LMH: '#dc143c',
  LMP2: '#3f90da',
  MASTERS: '#f1d302',
  PRO: '#e67e22',
  'PRO-AM': '#4caf50',
};

export const HEX_COLOR_REGEX = /^#[0-9a-fA-F]{6}$/u;
export const GAP_TIME_REGEX = /^([+-])?(\d+):(\d{2})\.(\d{3})$/u;
export const LAP_SUFFIX_REGEX = /^\s*([+-])?\s*(\d+)\s*L\s*$/iu;
export const GAP_CH_REGEX = /(\d+)ch/u;
export const NUMERIC_LAPS_REGEX = /^\d+$/u;
