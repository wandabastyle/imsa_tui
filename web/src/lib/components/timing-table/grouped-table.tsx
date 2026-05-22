import { useEffect, useMemo, useRef, type JSX } from 'react';

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
}

const EMPTY_LENGTH = 0;
const UNFOCUSED_TAB_INDEX = -1;

export const GroupedTable = (props: GroupedTableProps): JSX.Element => {
  const {
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
  } = props;
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map());
  const marqueeTick = useMarqueeTick();
  const lastSelectedRowRef = useRef<number>(INITIAL_INDEX);
  const lastSeriesRef = useRef<Series | null>(null);
  const lastTitleRef = useRef<string>('');

  const allEntries = useMemo(
    () => groupedSections.flatMap((section) => section.entries),
    [groupedSections],
  );

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
    selected.focus({ preventScroll: true });
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
      {groupedSections.map((section) => (
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
              {section.entries.map((entry, index) => {
                const absoluteIndex = section.start + index;
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
                      }
                    }}
                    className={rowClassName(rowClasses)}
                    style={style}
                    tabIndex={isSelected ? INITIAL_INDEX : UNFOCUSED_TAB_INDEX}
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
        </section>
      ))}
    </div>
  );
};
