import { getColumnWidthRule } from '$lib/table/columns';

const widthBaselinesByContext = new Map<string, number[]>();

export interface WidthComputationOptions {
  previousWidthsCh?: number[];
  maxShrinkPerUpdate?: number;
}

function textWidthCh(value: string): number {
  return Array.from(value).length;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function computeColumnWidths(
  columns: string[],
  rows: string[][],
  options: WidthComputationOptions = {}
): number[] {
  const previous = options.previousWidthsCh ?? [];
  const maxShrink = options.maxShrinkPerUpdate ?? 1;

  return columns.map((column, colIndex) => {
    const rule = getColumnWidthRule(column);
    let observed = textWidthCh(column);

    for (const row of rows) {
      observed = Math.max(observed, textWidthCh(row[colIndex] ?? ''));
    }

    const target = clamp(observed + rule.paddingCh, rule.minCh, rule.maxCh);
    const baseline = previous[colIndex] ?? target;

    if (target >= baseline) {
      return target;
    }

    return Math.max(target, baseline - maxShrink);
  });
}

export function asChWidths(widths: number[]): string[] {
  return widths.map((width) => `${String(width)}ch`);
}

export function computeStableColumnWidths(
  contextKey: string,
  columns: string[],
  rows: string[][],
  maxShrinkPerUpdate = 1
): number[] {
  const previousWidthsCh = widthBaselinesByContext.get(contextKey);
  const nextWidthsCh = computeColumnWidths(columns, rows, {
    previousWidthsCh,
    maxShrinkPerUpdate
  });
  widthBaselinesByContext.set(contextKey, nextWidthsCh);
  return nextWidthsCh;
}
