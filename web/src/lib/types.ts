// Re-export all generated types from Rust backend
export * from './generated/web-shared';

import type { Series } from './generated/web-shared';

// Additional frontend-only types
export type ViewMode =
  | { kind: 'class'; index: number }
  | { kind: 'favourites' }
  | { kind: 'grouped' }
  | { kind: 'overall' };

// Series list for iteration
export const ALL_SERIES: Series[] = [
  'dhlm',
  'f1',
  'imsa',
  'nls',
  'wec',
];
