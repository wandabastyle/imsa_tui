// View utilities
import type { TimingEntry, ViewMode } from './types';

export const classDisplayName = function classDisplayName(value: string): string {
  const normalized: string = value.replaceAll(' ', '').replaceAll('_', '').toUpperCase();
  if (normalized === 'GTDPRO') {
    return 'GTD PRO';
  }
  return value.trim() || '-';
};

export const getGroups = function getGroups(entries: TimingEntry[]): string[] {
  const grouped = new Map<string, TimingEntry[]>();
  for (const entry of entries) {
    const group: string = classDisplayName(entry.class_name);
    if (!grouped.has(group)) {
      grouped.set(group, []);
    }
    const groupEntries: TimingEntry[] | undefined = grouped.get(group);
    if (groupEntries !== undefined) {
      groupEntries.push(entry);
    }
  }
  return [...grouped.keys()].sort();
};

const MINIMUM_GROUP_COUNT = 0;
const INDEX_INCREMENT = 1;

export const nextViewMode = function nextViewMode(current: ViewMode, groupCount: number): ViewMode {
  if (groupCount === MINIMUM_GROUP_COUNT) {
    if (current.kind === 'overall') {
      return { kind: 'grouped' };
    }
    if (current.kind === 'grouped') {
      return { kind: 'favourites' };
    }
    return { kind: 'overall' };
  }
  if (current.kind === 'overall') {
    return { kind: 'grouped' };
  }
  if (current.kind === 'grouped') {
    return { index: 0, kind: 'class' };
  }
  if (current.kind === 'class') {
    return current.index + INDEX_INCREMENT < groupCount
      ? { index: current.index + INDEX_INCREMENT, kind: 'class' }
      : { kind: 'favourites' };
  }
  return { kind: 'overall' };
};
