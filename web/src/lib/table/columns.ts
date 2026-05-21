import type { Series } from '../types';

export interface ColumnWidthRule {
  minCh: number;
  maxCh: number;
  paddingCh: number;
}

const columnsBySeries: Record<Series, string[]> = {
  dhlm: [
    'Pos',
    '#',
    'Class',
    'PIC',
    'Driver',
    'Vehicle',
    'Team',
    'Laps',
    'Gap',
    'Last',
    'Best',
    'S1',
    'S2',
    'S3',
    'S4',
    'S5',
  ],
  f1: ['Pos', '#', 'Driver', 'Team', 'Laps', 'Gap', 'Int', 'Last', 'Best', 'Pit', 'Stops', 'PIC'],
  imsa: [
    'Pos',
    '#',
    'Class',
    'PIC',
    'Driver',
    'Vehicle',
    'Laps',
    'Gap O',
    'Gap C',
    'Next C',
    'Last',
    'Best',
    'BL#',
    'Pit',
    'Stop',
    'Fastest Driver',
  ],
  nls: [
    'Pos',
    '#',
    'Class',
    'PIC',
    'Driver',
    'Vehicle',
    'Team',
    'Laps',
    'Gap',
    'Last',
    'Best',
    'S1',
    'S2',
    'S3',
    'S4',
    'S5',
  ],
  wec: [
    'Pos',
    '#',
    'Class',
    'PIC',
    'Driver',
    'Vehicle',
    'Team',
    'Laps',
    'Gap',
    'Last',
    'Best',
    'S1',
    'S2',
    'S3',
  ],
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
  'S5',
]);

const widthRuleByColumn: Record<string, ColumnWidthRule> = {
  '#': { maxCh: 8, minCh: 6, paddingCh: 1 },
  'BL#': { maxCh: 5, minCh: 4, paddingCh: 1 },
  'Best': { maxCh: 11, minCh: 8, paddingCh: 1 },
  'Class': { maxCh: 12, minCh: 7, paddingCh: 1 },
  'Driver': { maxCh: 32, minCh: 12, paddingCh: 1 },
  'Fastest Driver': { maxCh: 28, minCh: 14, paddingCh: 1 },
  'Gap': { maxCh: 13, minCh: 9, paddingCh: 1 },
  'Gap C': { maxCh: 13, minCh: 9, paddingCh: 1 },
  'Gap O': { maxCh: 13, minCh: 9, paddingCh: 1 },
  'Int': { maxCh: 13, minCh: 9, paddingCh: 1 },
  'Laps': { maxCh: 6, minCh: 4, paddingCh: 1 },
  'Last': { maxCh: 11, minCh: 8, paddingCh: 1 },
  'Next C': { maxCh: 13, minCh: 9, paddingCh: 1 },
  'PIC': { maxCh: 5, minCh: 4, paddingCh: 1 },
  'Pit': { maxCh: 5, minCh: 4, paddingCh: 1 },
  'Pos': { maxCh: 5, minCh: 4, paddingCh: 1 },
  'S1': { maxCh: 10, minCh: 8, paddingCh: 1 },
  'S2': { maxCh: 10, minCh: 8, paddingCh: 1 },
  'S3': { maxCh: 10, minCh: 8, paddingCh: 1 },
  'S4': { maxCh: 10, minCh: 8, paddingCh: 1 },
  'S5': { maxCh: 10, minCh: 8, paddingCh: 1 },
  'Stop': { maxCh: 6, minCh: 5, paddingCh: 1 },
  'Stops': { maxCh: 6, minCh: 5, paddingCh: 1 },
  'Team': { maxCh: 36, minCh: 14, paddingCh: 1 },
  'Vehicle': { maxCh: 34, minCh: 14, paddingCh: 1 },
};

const defaultRule: ColumnWidthRule = { maxCh: 16, minCh: 8, paddingCh: 1 };

export const getColumnsForSeries = function getColumnsForSeries(series: Series): string[] {
  return columnsBySeries[series];
};

export const isCompactColumn = function isCompactColumn(column: string): boolean {
  return compactColumns.has(column);
};

export const getColumnWidthRule = function getColumnWidthRule(column: string): ColumnWidthRule {
  return widthRuleByColumn[column] ?? defaultRule;
};
