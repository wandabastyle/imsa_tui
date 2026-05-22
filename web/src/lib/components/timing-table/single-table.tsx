import { useEffect, useMemo, useRef, type JSX } from 'react';

import type { Series, TimingClassColor, TimingEntry } from '../../generated/web-shared';
import { asChWidths, computeStableColumnWidths } from '../../table/widths';
import { DEFAULT_COL_WIDTH_CH, EXTRA_WIDTH_FACTOR, FIRST_ROW, INITIAL_INDEX } from './constants';
import type { GapAnchorInfo, PitTracker } from './types';
import {
  calculateRelativeGap,
  classTintActive,
  columnClassName,
  columnWidthChars,
  compactColumnClass,
  getCellsForEntry,
  getEmptyMessage,
  getRowPitPhase,
  isLongTextColumn,
  isRelativeGapColumn,
  maybeClassTint,
  rowClassName,
  rowKey,
  rowStyle,
  tableColKey,
  useMarquee,
  useMarqueeTick,
} from './utils';

interface SingleTableProps {
  entries: TimingEntry[];
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
}

const EMPTY_LENGTH = 0;

export const SingleTable = (props: SingleTableProps): JSX.Element => {
  const {
    entries,
    columns,
    selectedRow,
    series,
    classColors,
    gapAnchor,
    pitTrackers,
    favourites,
    markedStableId,
    title,
    loading,
  } = props;
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map());
  const marqueeTick = useMarqueeTick();
  const lastSelectedRowRef = useRef<number>(INITIAL_INDEX);
  const lastSeriesRef = useRef<Series | null>(null);
  const lastTitleRef = useRef<string>('');

  const rowsData = useMemo(
    () =>
      entries.map((entry) => {
        const cells = getCellsForEntry(entry, series, favourites);
        return cells.map((cell, colIndex) => {
          const column = columns[colIndex];
          return isRelativeGapColumn(column) && gapAnchor
            ? calculateRelativeGap(entry, column, cell, gapAnchor)
            : cell;
        });
      }),
    [columns, entries, favourites, gapAnchor, series],
  );

  const widths = useMemo(() => {
    const contextKey = `${series}|${title}`;
    return computeStableColumnWidths(contextKey, columns, rowsData, EXTRA_WIDTH_FACTOR);
  }, [columns, rowsData, series, title]);

  const widthStyles = useMemo(() => asChWidths(widths), [widths]);

  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) {
      return;
    }

    const selectionChanged = selectedRow !== lastSelectedRowRef.current;
    const contextChanged = series !== lastSeriesRef.current || title !== lastTitleRef.current;
    if (!selectionChanged && !contextChanged) {
      return;
    }

    const selected = rowRefs.current.get(selectedRow) ?? container.querySelector('tr.selected');
    if (!selected) {
      lastSelectedRowRef.current = selectedRow;
      lastSeriesRef.current = series;
      lastTitleRef.current = title;
      return;
    }

    if (selectedRow === FIRST_ROW && (selectionChanged || contextChanged)) {
      container.scrollTop = 0;
    } else {
      selected.scrollIntoView({ block: 'center', inline: 'nearest' });
    }

    lastSelectedRowRef.current = selectedRow;
    lastSeriesRef.current = series;
    lastTitleRef.current = title;
  }, [selectedRow, series, title]);

  if (entries.length === EMPTY_LENGTH) {
    return (
      <table>
        <colgroup>
          {widthStyles.map((width, index) => (
            <col key={tableColKey(series, 'flat', index, width)} style={{ width }} />
          ))}
        </colgroup>
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column} className={compactColumnClass(column)}>
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr>
            <td className="empty-message" colSpan={columns.length}>
              {getEmptyMessage(loading, entries.length, series, title)}
            </td>
          </tr>
        </tbody>
      </table>
    );
  }

  return (
    <table>
      <colgroup>
        {widthStyles.map((width, index) => (
          <col key={tableColKey(series, 'flat', index, width)} style={{ width }} />
        ))}
      </colgroup>
      <thead>
        <tr>
          {columns.map((column) => (
            <th key={column} className={compactColumnClass(column)}>
              {column}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {entries.map((entry, index) => {
          const isSelected = index === selectedRow;
          const classTint = maybeClassTint(series, entry.class_name, classColors);
          const pitPhase = getRowPitPhase(entry, pitTrackers);
          const rowClasses: string[] = [];
          if (classTintActive(classTint)) {
            rowClasses.push('class-tint');
          }
          if (pitPhase) {
            rowClasses.push(pitPhase);
          }
          if (isSelected) {
            rowClasses.push('selected');
          }
          if (entry.stable_id === markedStableId) {
            rowClasses.push('search-mark');
          }
          const style = rowStyle(classTint, isSelected);
          const cells = getCellsForEntry(entry, series, favourites);

          return (
            <tr
              key={entry.stable_id}
              ref={(el) => {
                if (el) {
                  rowRefs.current.set(index, el);
                }
              }}
              data-index={index}
              data-stable-id={entry.stable_id}
              className={rowClassName(rowClasses)}
              style={style}
            >
              {cells.map((cell, colIndex) => {
                const column = columns[colIndex];
                const displayValue =
                  isRelativeGapColumn(column) && gapAnchor
                    ? calculateRelativeGap(entry, column, cell, gapAnchor)
                    : cell;
                const width = columnWidthChars(widthStyles[colIndex], DEFAULT_COL_WIDTH_CH);
                const finalValue = useMarquee(
                  isSelected && isLongTextColumn(column),
                  displayValue,
                  width,
                  marqueeTick,
                );
                return (
                  <td
                    key={rowKey(entry.stable_id, column)}
                    className={columnClassName(column, cell, series)}
                  >
                    {finalValue}
                  </td>
                );
              })}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
};
