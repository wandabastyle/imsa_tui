// Re-export all generated types from Rust backend
export * from './generated/web-shared';

// Additional frontend-only types
export type ViewMode =
  | { kind: 'overall' }
  | { kind: 'grouped' }
  | { kind: 'class'; index: number }
  | { kind: 'favourites' };

// Series list for iteration
export const ALL_SERIES: import('./generated/web-shared').Series[] = [
  'imsa',
  'nls',
  'f1',
  'wec',
  'dhlm',
];
