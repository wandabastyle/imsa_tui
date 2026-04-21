import type { Series } from '$lib/types';

export interface ColumnWidthRule {
  minCh: number;
  maxCh: number;
  paddingCh: number;
}

const columnsBySeries: Record<Series, string[]> = {
  imsa: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Laps', 'Gap O', 'Gap C', 'Next C', 'Last', 'Best', 'BL#', 'Pit', 'Stop', 'Fastest Driver'],
  nls: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Team', 'Laps', 'Gap', 'Last', 'Best', 'S1', 'S2', 'S3', 'S4', 'S5'],
  f1: ['Pos', '#', 'Driver', 'Team', 'Laps', 'Gap', 'Int', 'Last', 'Best', 'Pit', 'Stops', 'PIC'],
  wec: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Team', 'Laps', 'Gap', 'Last', 'Best', 'S1', 'S2', 'S3'],
  dhlm: ['Pos', '#', 'Class', 'PIC', 'Driver', 'Vehicle', 'Team', 'Laps', 'Gap', 'Last', 'Best', 'S1', 'S2', 'S3', 'S4', 'S5']
};

const compactColumns = new Set([
  'Pos',
  '#',
  'PIC',
  'BL#',
  'Pit',
  'Stop',
  'Stops',
  'Laps',
  'Gap',
  'Gap O',
  'Gap C',
  'Next C',
  'Int',
  'Last',
  'Best',
  'S1',
  'S2',
  'S3',
  'S4',
  'S5'
]);

const widthRuleByColumn: Record<string, ColumnWidthRule> = {
  Pos: { minCh: 4, maxCh: 5, paddingCh: 1 },
  '#': { minCh: 6, maxCh: 8, paddingCh: 1 },
  Class: { minCh: 7, maxCh: 12, paddingCh: 1 },
  PIC: { minCh: 4, maxCh: 5, paddingCh: 1 },
  Driver: { minCh: 12, maxCh: 32, paddingCh: 1 },
  Vehicle: { minCh: 14, maxCh: 34, paddingCh: 1 },
  Team: { minCh: 14, maxCh: 36, paddingCh: 1 },
  Laps: { minCh: 4, maxCh: 6, paddingCh: 1 },
  'Gap O': { minCh: 9, maxCh: 13, paddingCh: 1 },
  'Gap C': { minCh: 9, maxCh: 13, paddingCh: 1 },
  'Next C': { minCh: 9, maxCh: 13, paddingCh: 1 },
  Gap: { minCh: 9, maxCh: 13, paddingCh: 1 },
  Int: { minCh: 9, maxCh: 13, paddingCh: 1 },
  Last: { minCh: 8, maxCh: 11, paddingCh: 1 },
  Best: { minCh: 8, maxCh: 11, paddingCh: 1 },
  'BL#': { minCh: 4, maxCh: 5, paddingCh: 1 },
  Pit: { minCh: 4, maxCh: 5, paddingCh: 1 },
  Stop: { minCh: 5, maxCh: 6, paddingCh: 1 },
  Stops: { minCh: 5, maxCh: 6, paddingCh: 1 },
  'Fastest Driver': { minCh: 14, maxCh: 28, paddingCh: 1 },
  S1: { minCh: 8, maxCh: 10, paddingCh: 1 },
  S2: { minCh: 8, maxCh: 10, paddingCh: 1 },
  S3: { minCh: 8, maxCh: 10, paddingCh: 1 },
  S4: { minCh: 8, maxCh: 10, paddingCh: 1 },
  S5: { minCh: 8, maxCh: 10, paddingCh: 1 }
};

const defaultRule: ColumnWidthRule = { minCh: 8, maxCh: 16, paddingCh: 1 };

export function getColumnsForSeries(series: Series): string[] {
  return columnsBySeries[series];
}

export function isCompactColumn(column: string): boolean {
  return compactColumns.has(column);
}

export function getColumnWidthRule(column: string): ColumnWidthRule {
  return widthRuleByColumn[column] ?? defaultRule;
}
