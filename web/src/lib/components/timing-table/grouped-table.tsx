import { useLayoutEffect, useMemo, useRef, type JSX } from 'react';

import type { Series, TimingClassColor } from '../../generated/web-shared';
import { asChWidths, computeStableColumnWidths } from '../../table/widths';
import { DEFAULT_COL_WIDTH_CH, EXTRA_WIDTH_FACTOR, FIRST_ROW, INITIAL_INDEX } from './constants';
import type { GapAnchorInfo, GroupSection, PitTracker } from './types';
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

interface GroupedTableProps {
  scrollContainerRef: React.RefObject<HTMLDivElement | null>;
  groupedSections: GroupSection[];
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
  viewportHeight: number | null;
  rowHeight: number | null;
  minRowsPerGroup: number;
}

const EMPTY_LENGTH = 0;
const INDEX_NOT_FOUND = -1;
const ZERO_ROWS = 0;
const ONE_HALF = 2;
const DEFAULT_ROW_HEIGHT_VALUE = 24;
const MAX_VISIBLE_ROWS_DEFAULT = 10;
const FOCUSED_TAB_INDEX = 0;
const UNFOCUSED_TAB_INDEX = -1;

export const GroupedTable = (props: GroupedTableProps): JSX.Element => {
  const {
    scrollContainerRef,
    groupedSections,
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
    viewportHeight,
    rowHeight,
    minRowsPerGroup,
  } = props;
  const rowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map());
  const marqueeTick = useMarqueeTick();
  const lastSelectedRowRef = useRef<number>(INITIAL_INDEX);
  const lastSeriesRef = useRef<Series | null>(null);
  const lastTitleRef = useRef<string>('');

  const allEntries = useMemo(
    () => groupedSections.flatMap((section) => section.entries),
    [groupedSections],
  );

  const maxVisibleRows = useMemo(() => {
    if (viewportHeight === null || rowHeight === null || rowHeight === ZERO_ROWS) {
      return MAX_VISIBLE_ROWS_DEFAULT;
    }
    return Math.floor(viewportHeight / rowHeight) - DEFAULT_ROW_HEIGHT_VALUE;
  }, [viewportHeight, rowHeight]);

  const rowsData = useMemo(
    () =>
      allEntries.map((entry) => {
        const cells = getCellsForEntry(entry, series, favourites);
        return cells.map((cell, colIndex) => {
          const column = columns[colIndex];
          return isRelativeGapColumn(column) && gapAnchor
            ? calculateRelativeGap(entry, column, cell, gapAnchor)
            : cell;
        });
      }),
    [allEntries, columns, favourites, gapAnchor, series],
  );

  const widths = useMemo(() => {
    const contextKey = `${series}|${title}`;
    return computeStableColumnWidths(contextKey, columns, rowsData, EXTRA_WIDTH_FACTOR);
  }, [columns, rowsData, series, title]);

  const widthStyles = useMemo(() => asChWidths(widths), [widths]);

  useLayoutEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) {
      return;
    }
    const selectionChanged = selectedRow !== lastSelectedRowRef.current;
    const contextChanged = series !== lastSeriesRef.current || title !== lastTitleRef.current;
    if (!selectionChanged && !contextChanged) {
      return;
    }
    const selected = container.querySelector('tr.selected') ?? rowRefs.current.get(selectedRow);
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

    if (selected instanceof HTMLElement) {
      selected.focus({ preventScroll: true });
    }

    lastSelectedRowRef.current = selectedRow;
    lastSeriesRef.current = series;
    lastTitleRef.current = title;
  }, [selectedRow, series, title]);

  if (groupedSections.length === EMPTY_LENGTH) {
    return (
      <div className="group-stack">
        <p className="empty">{getEmptyMessage(loading, EMPTY_LENGTH, series, title)}</p>
      </div>
    );
  }

  return (
    <div className="group-stack">
      {groupedSections.map((section) => {
        const totalCars = allEntries.length;
        const groupCarCount = section.entries.length;
        const totalRows = maxVisibleRows;
        const proportionalRows = Math.round((groupCarCount * totalRows) / totalCars);
        const clampedRows = Math.max(minRowsPerGroup, Math.min(proportionalRows, groupCarCount));

        const isSelectedInGroup = section.entries.some(
          (entry, index) => section.start + index === selectedRow,
        );
        const selectedGroupIndex = section.entries.findIndex(
          (entry) => section.start + section.entries.indexOf(entry) === selectedRow,
        );
        const selectedIndexInGroup =
          selectedGroupIndex === INDEX_NOT_FOUND
            ? Math.floor(clampedRows / ONE_HALF)
            : selectedGroupIndex;

        const sliceStart = Math.max(
          ZERO_ROWS,
          Math.min(
            selectedIndexInGroup - Math.floor(clampedRows / ONE_HALF),
            groupCarCount - clampedRows,
          ),
        );
        const visibleEntries = section.entries.slice(sliceStart, sliceStart + clampedRows);

        const hasMore = section.entries.length > visibleEntries.length && isSelectedInGroup;

        return (
          <section key={section.name} className="group-section">
            <div className="group-title">
              {section.name} ({section.entries.length} cars)
            </div>
            <table>
              <colgroup>
                {widthStyles.map((width, index) => (
                  <col key={tableColKey(series, 'group', index, width)} style={{ width }} />
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
                {visibleEntries.map((entry, index) => {
                  const absoluteIndex = section.start + sliceStart + index;
                  const isSelected = absoluteIndex === selectedRow;
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
                          rowRefs.current.set(absoluteIndex, el);
                        } else {
                          rowRefs.current.delete(absoluteIndex);
                        }
                      }}
                      data-index={absoluteIndex}
                      data-stable-id={entry.stable_id}
                      className={rowClassName(rowClasses)}
                      style={style}
                      tabIndex={isSelected ? FOCUSED_TAB_INDEX : UNFOCUSED_TAB_INDEX}
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
            {hasMore && (
              <div className="group-truncated">
                (+{section.entries.length - visibleEntries.length} more)
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
};
