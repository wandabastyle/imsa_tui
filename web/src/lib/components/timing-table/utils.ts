import { useEffect, useRef, useState, type CSSProperties } from 'react';

import type { Series, TimingClassColor, TimingEntry } from '../../generated/web-shared';
import { isCompactColumn } from '../../table/columns';
import {
  GAP_CH_REGEX,
  GAP_TIME_REGEX,
  HEX_COLOR_REGEX,
  LAP_SUFFIX_REGEX,
  LONG_TEXT_COLUMNS,
  MARQUEE_CYCLE_MAX,
  MARQUEE_GAP,
  MATCH_GROUP_ONE,
  NUMERIC_LAPS_REGEX,
  PIT_IN_DURATION_MS,
  PIT_OUT_DURATION_MS,
  RADIX_DECIMAL,
  RELATIVE_GAP_COLUMNS,
  STANDARD_STATIC_COLORS,
  TICK_INTERVAL_MS,
  WEC_STATIC_COLORS,
} from './constants';
import type { GapAnchorInfo, ParsedGap, PitTracker } from './types';

const LEGACY_STABLE_ID_PARTS_MIN = 3;
const INDEX_ZERO = 0;
const INDEX_ONE = 1;
const INDEX_TWO = 2;
const INDEX_THREE = 3;
const INDEX_FOUR = 4;
const NEGATIVE_ONE = -1;
const POSITIVE_ONE = 1;
const ZERO = 0;
const SECONDS_PER_MINUTE = 60;
const MILLISECONDS_PER_SECOND = 1000;

export const looksLikeHexColor = (value: string | undefined): boolean => {
  if (value === undefined || value === '') {
    return false;
  }
  return HEX_COLOR_REGEX.test(value.trim());
};

const resolveLiveClassColor = (
  classColors: Record<string, TimingClassColor | undefined>,
  classKey: string,
): string | null => {
  const direct = Object.hasOwn(classColors, classKey) ? classColors[classKey] : undefined;
  if (direct !== undefined && looksLikeHexColor(direct.color)) {
    return direct.color.trim();
  }
  return null;
};

export const resolveClassTextColor = (
  series: Series,
  className: string,
  classColors: Record<string, TimingClassColor | undefined>,
): string | null => {
  if (series === 'dhlm' || series === 'nls') {
    return null;
  }

  const key = className.trim().toUpperCase();
  const liveColor = resolveLiveClassColor(classColors, key);
  if (liveColor !== null && liveColor !== '') {
    return liveColor;
  }

  if (series === 'wec') {
    return WEC_STATIC_COLORS[key] ?? null;
  }

  return STANDARD_STATIC_COLORS[key] ?? null;
};

const trimLegacyClassSuffix = (stableId: string, expectedPrefix: string): string => {
  if (!stableId.startsWith(`${expectedPrefix}:`)) {
    return stableId;
  }
  const parts = stableId.split(':');
  if (parts.length < LEGACY_STABLE_ID_PARTS_MIN) {
    return stableId;
  }
  return `${parts[INDEX_ZERO]}:${parts[INDEX_ONE]}`;
};

export const normalizeStableId = (stableId: string, series: Series): string => {
  if (series === 'imsa') {
    return trimLegacyClassSuffix(stableId, 'fallback');
  }
  if (series === 'nls') {
    return trimLegacyClassSuffix(stableId, 'stnr');
  }
  return stableId;
};

export const isFavourite = (
  entry: TimingEntry,
  favourites: Set<string>,
  series: Series,
): boolean => {
  const normalizedId = normalizeStableId(entry.stable_id, series);
  return favourites.has(`${series}|${normalizedId}`);
};

export const isRelativeGapColumn = (column: string): boolean => RELATIVE_GAP_COLUMNS.has(column);

export const parseGap = (value: string): ParsedGap | null => {
  const raw = value.trim();
  const lapsMatch = LAP_SUFFIX_REGEX.exec(raw);
  if (lapsMatch) {
    const sign = lapsMatch[INDEX_ONE] === '-' ? NEGATIVE_ONE : POSITIVE_ONE;
    const laps = Number.parseInt(lapsMatch[INDEX_TWO], RADIX_DECIMAL);
    if (!Number.isNaN(laps)) {
      return { kind: 'laps', value: sign * laps };
    }
  }

  const timeMatch = GAP_TIME_REGEX.exec(raw);
  if (timeMatch) {
    const sign = timeMatch[INDEX_ONE] === '-' ? NEGATIVE_ONE : POSITIVE_ONE;
    const minutes = Number.parseInt(timeMatch[INDEX_TWO], RADIX_DECIMAL);
    const seconds = Number.parseInt(timeMatch[INDEX_THREE], RADIX_DECIMAL);
    const millis = Number.parseInt(timeMatch[INDEX_FOUR], RADIX_DECIMAL);
    const totalMs = (minutes * SECONDS_PER_MINUTE + seconds) * MILLISECONDS_PER_SECOND + millis;
    return { kind: 'time', value: sign * totalMs };
  }

  return null;
};

export const formatLapDelta = (lapDelta: number): string => {
  if (lapDelta === ZERO) {
    return '+0L';
  }
  const prefix = lapDelta > ZERO ? '+' : '-';
  return `${prefix}${Math.abs(lapDelta)}L`;
};

export const anchorGapLabel = (entry: TimingEntry): string => {
  const laps = entry.laps.trim();
  return NUMERIC_LAPS_REGEX.test(laps) ? `----LAP ${laps}` : '----';
};

export const calculateRelativeGap = (
  entry: TimingEntry,
  column: string,
  current: string,
  gapAnchor: GapAnchorInfo | null,
): string => {
  if (!gapAnchor || !isRelativeGapColumn(column)) {
    return current;
  }
  if (entry.stable_id === gapAnchor.stableId) {
    return anchorGapLabel(entry);
  }
  const rowLaps = Number.parseInt(entry.laps.trim(), RADIX_DECIMAL);
  const anchorLaps = gapAnchor.laps;
  if (!Number.isNaN(rowLaps) && !Number.isNaN(anchorLaps) && rowLaps !== anchorLaps) {
    return formatLapDelta(anchorLaps - rowLaps);
  }
  return current;
};

export const compactColumnClass = (column: string): string =>
  isCompactColumn(column) ? 'tight-col' : '';

export const pitCellClass = (column: string, value: string, series: Series): string => {
  if ((column === 'Pit' || column === 'PIT') && value.toLowerCase() === 'yes') {
    return 'pit-active';
  }
  if (series === 'nls' && column === 'S5' && value.toUpperCase() === 'PIT') {
    return 'pit-active';
  }
  if ((column === 'Stop' || column === 'Stops') && value !== '-' && value !== '0') {
    return 'stops-hot';
  }
  return '';
};

export const pitSignalActive = (entry: TimingEntry, series: Series): boolean => {
  if (series === 'imsa' || series === 'f1' || series === 'wec') {
    return entry.pit.toLowerCase() === 'yes';
  }
  return entry.sector_5.trim().toUpperCase() === 'PIT';
};

export const getCellsForEntry = (
  entry: TimingEntry,
  series: Series,
  favourites: Set<string>,
): string[] => {
  const favFlag = isFavourite(entry, favourites, series) ? '★ ' : '';
  if (series === 'imsa') {
    return [
      String(entry.position),
      `${favFlag}${entry.car_number}`,
      entry.class_name,
      entry.class_rank,
      entry.driver,
      entry.vehicle,
      entry.laps,
      entry.gap_overall,
      entry.gap_class,
      entry.gap_next_in_class,
      entry.last_lap,
      entry.best_lap,
      entry.best_lap_no,
      entry.pit,
      entry.pit_stops,
      entry.fastest_driver,
    ];
  }
  if (series === 'wec') {
    return [
      String(entry.position),
      `${favFlag}${entry.car_number}`,
      entry.class_name,
      entry.class_rank,
      entry.driver,
      entry.vehicle,
      entry.team,
      entry.laps,
      entry.gap_overall,
      entry.last_lap,
      entry.best_lap,
      entry.sector_1,
      entry.sector_2,
      entry.sector_3,
    ];
  }
  if (series === 'nls') {
    return [
      String(entry.position),
      `${favFlag}${entry.car_number}`,
      entry.class_name,
      entry.class_rank,
      entry.driver,
      entry.vehicle,
      entry.team,
      entry.laps,
      entry.gap_overall,
      entry.last_lap,
      entry.best_lap,
      entry.sector_1,
      entry.sector_2,
      entry.sector_3,
      entry.sector_4,
      entry.sector_5,
    ];
  }
  return [
    String(entry.position),
    `${favFlag}${entry.car_number}`,
    entry.driver,
    entry.team,
    entry.laps,
    entry.gap_overall,
    entry.gap_class,
    entry.last_lap,
    entry.best_lap,
    entry.pit,
    entry.pit_stops,
    entry.class_rank,
  ];
};

export const getEmptyMessage = (
  loading: boolean,
  entriesLength: number,
  series: Series,
  title: string,
): string => {
  if (loading) {
    return `Waiting for ${series.toUpperCase()} snapshot...`;
  }
  if (entriesLength === ZERO) {
    return `No timing data available for ${series.toUpperCase()}`;
  }
  if (title.toLowerCase().includes('favourite') || title.toLowerCase().includes('fav')) {
    return `No favourites for ${series.toUpperCase()}. Press Space on a car to add one.`;
  }
  return 'No data available.';
};

export const getRowPitPhase = (
  entry: TimingEntry,
  pitTrackers: Map<string, PitTracker>,
): string => {
  const now = Date.now();
  const tracker = pitTrackers.get(entry.stable_id);
  if (!tracker) {
    return '';
  }
  if (tracker.inPit && now <= tracker.inUntil) {
    return 'pit-in';
  }
  if (tracker.inPit) {
    return 'pit-active';
  }
  if (now <= tracker.outUntil) {
    return 'pit-out';
  }
  return '';
};

export const usePitTrackers = (entries: TimingEntry[], series: Series): Map<string, PitTracker> => {
  const [pitTrackers, setPitTrackers] = useState<Map<string, PitTracker>>(new Map());
  const prevEntriesRef = useRef<Map<string, boolean>>(new Map());

  useEffect(() => {
    const now = Date.now();
    const newTrackers = new Map(pitTrackers);
    const currentStableIds = new Set(entries.map((entry) => entry.stable_id));

    for (const key of newTrackers.keys()) {
      if (!currentStableIds.has(key)) {
        newTrackers.delete(key);
      }
    }

    for (const entry of entries) {
      const key = entry.stable_id;
      const signal = pitSignalActive(entry, series);
      const tracker = newTrackers.get(key) ?? { inPit: false, inUntil: 0, outUntil: 0 };

      if (signal) {
        if (!tracker.inPit) {
          tracker.inPit = true;
          tracker.inUntil = now + PIT_IN_DURATION_MS;
        }
        tracker.outUntil = 0;
      } else if (tracker.inPit) {
        tracker.inPit = false;
        tracker.inUntil = 0;
        tracker.outUntil = now + PIT_OUT_DURATION_MS;
      }

      newTrackers.set(key, tracker);
      prevEntriesRef.current.set(key, signal);
    }

    setPitTrackers(newTrackers);
  }, [entries, pitTrackers, series]);

  return pitTrackers;
};

export const useMarquee = (
  isSelected: boolean,
  text: string,
  width: number,
  tick: number,
): string => {
  if (!isSelected || text.length <= width) {
    return text;
  }
  const cycle = text.length + MARQUEE_GAP;
  const offset = tick % cycle;
  if (offset < text.length) {
    return `${text.slice(offset)}   ${text.slice(INDEX_ZERO, offset)}`;
  }
  return `${' '.repeat(offset - text.length)}${text}`;
};

export const useMarqueeTick = (): number => {
  const [tick, setTick] = useState(INDEX_ZERO);
  useEffect(() => {
    const interval = setInterval(() => {
      setTick((prev) => (prev + INDEX_ONE) % MARQUEE_CYCLE_MAX);
    }, TICK_INTERVAL_MS);
    return (): void => {
      clearInterval(interval);
    };
  }, []);
  return tick;
};

export const columnWidthChars = (rawWidth: string | undefined, fallback: number): number => {
  const raw = rawWidth ?? `${fallback}ch`;
  const match = GAP_CH_REGEX.exec(raw);
  if (match === null) {
    return fallback;
  }
  return Number.parseInt(match[MATCH_GROUP_ONE], RADIX_DECIMAL);
};

export const isLongTextColumn = (column: string): boolean => LONG_TEXT_COLUMNS.has(column);
export const rowClassName = (rowClasses: string[]): string => rowClasses.join(' ');
export const rowStyle = (
  classTint: string | null,
  isSelected: boolean,
): CSSProperties & { '--class-color'?: string } => {
  const style: CSSProperties & { '--class-color'?: string } = {};
  if (classTint !== null && classTint !== '' && !isSelected) {
    style['--class-color'] = classTint;
  }
  return style;
};
export const classTintActive = (classTint: string | null): boolean =>
  classTint !== null && classTint !== '';
export const rowKey = (stableId: string, column: string): string => `${stableId}-${column}`;
export const tableColKey = (series: Series, kind: string, index: number, width: string): string =>
  `${series}-${kind}-${index}-${width}`;
export const maybeClassTint = (
  series: Series,
  className: string,
  classColors: Record<string, TimingClassColor | undefined>,
): string | null => resolveClassTextColor(series, className, classColors);
export const columnClassName = (column: string, value: string, series: Series): string => {
  const pitClass = pitCellClass(column, value, series);
  const compactClass = compactColumnClass(column);
  return `${pitClass} ${compactClass}`.trim();
};
