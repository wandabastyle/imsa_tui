// Timing-table.tsx - Main timing data table
import { useEffect, useMemo, useRef, type JSX } from 'react';

import type { TimingEntry, Series, TimingClassColor } from '../lib/generated/web-shared';
import { getColumnsForSeries, isCompactColumn } from '../lib/table/columns';
import { computeStableColumnWidths, asChWidths } from '../lib/table/widths';

const ROW_HEIGHT_PX = 28;
const SCROLL_INTO_VIEW_BLOCK = 'nearest';
const SLICE_START_INDEX = 0;
const FIRST_COLUMN_OFFSET = 1;

interface TimingTableProps {
  classColors: Record<string, TimingClassColor>;
  entries: TimingEntry[];
  selectedRow: number;
  series: Series;
}

// Column to property mapping for formatCellValue
const COLUMN_TO_PROPERTY: Record<string, keyof TimingEntry> = {
  '#': 'car_number',
  'BL#': 'best_lap_no',
  'Best': 'best_lap',
  'Class': 'class_name',
  'Driver': 'driver',
  'Fastest Driver': 'fastest_driver',
  'Gap': 'gap_overall',
  'Gap C': 'gap_class',
  'Gap O': 'gap_overall',
  'Int': 'gap_next_in_class',
  'Laps': 'laps',
  'Last': 'last_lap',
  'Next C': 'gap_next_in_class',
  'PIC': 'class_rank',
  'Pit': 'pit',
  'Pos': 'position',
  'S1': 'sector_1',
  'S2': 'sector_2',
  'S3': 'sector_3',
  'S4': 'sector_4',
  'S5': 'sector_5',
  'Stop': 'pit_stops',
  'Stops': 'pit_stops',
  'Team': 'team',
  'Vehicle': 'vehicle',
};

const formatCellValue = function formatCellValue(
  entry: TimingEntry,
  column: string,
): string {
  const property: keyof TimingEntry | undefined = COLUMN_TO_PROPERTY[column];
  if (property === undefined) {
    return '';
  }
  const value: string | number = entry[property];
  return typeof value === 'number' ? String(value) : (value ?? '');
};

const getClassColor = function getClassColor(
  className: string,
  classColors: Record<string, TimingClassColor>,
): string | null {
  const colorConfig: TimingClassColor | undefined = classColors[className];
  if (colorConfig === undefined) {
    return null;
  }
  const color: string | undefined = colorConfig.color;
  if (color === undefined || color === '') {
    return null;
  }
  return color;
};

const getPitColor = function getPitColor(pitValue: string): string | null {
  const upperPit = pitValue.toUpperCase();
  if (upperPit === 'IN') {
    return 'var(--pit-in)';
  }
  if (upperPit === 'PIT') {
    return 'var(--pit-active)';
  }
  if (upperPit === 'OUT') {
    return 'var(--pit-out)';
  }
  return null;
};

export const TimingTable = function TimingTable(
  props: TimingTableProps,
): JSX.Element {
  const { classColors, entries, selectedRow, series } = props;

  const columns: string[] = useMemo((): string[] => getColumnsForSeries(series), [series]);

  const rowsData: string[][] = useMemo(
    (): string[][] =>
      entries.map((entry: TimingEntry): string[] =>
        columns.map((column: string): string => formatCellValue(entry, column)),
      ),
    [entries, columns],
  );

  const widths: number[] = useMemo((): number[] => {
    const contextKey: string = `${series}-table`;
    return computeStableColumnWidths(contextKey, columns, rowsData);
  }, [columns, rowsData, series]);

  const widthStyles: string[] = useMemo((): string[] => asChWidths(widths), [widths]);

  const tableRef = useRef<HTMLDivElement>(null);
  const selectedRowRef = useRef<HTMLTableRowElement>(null);

  // Scroll selected row into view
  useEffect(() => {
    selectedRowRef.current?.scrollIntoView({
      behavior: 'smooth',
      block: SCROLL_INTO_VIEW_BLOCK,
    });
  }, [selectedRow]);

  const lastColumnIndex = columns.length - FIRST_COLUMN_OFFSET;

  return (
    <div
      ref={tableRef}
      className="timing-table-container"
      style={{
        flex: 1,
        minHeight: 0,
        overflow: 'auto',
      }}
    >
      <table
        style={{
          borderCollapse: 'collapse',
          fontSize: '0.8rem',
          tableLayout: 'fixed',
          width: '100%',
        }}
      >
        <thead
          style={{
            backgroundColor: 'var(--bg-table-header)',
            borderBottom: '1px solid var(--border)',
            position: 'sticky',
            top: 0,
            zIndex: 10,
          }}
        >
          <tr>
            {columns.map((column, colIndex) => {
              const isCompact = isCompactColumn(column);
              return (
                <th
                  key={column}
                  style={{
                    borderRight:
                      colIndex < lastColumnIndex
                        ? '1px solid var(--grid)'
                        : 'none',
                    color: 'var(--text-dim)',
                    fontWeight: 600,
                    overflow: 'hidden',
                    padding: '0.25rem 0.375rem',
                    textAlign: isCompact ? 'center' : 'left',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    width: widthStyles[colIndex],
                  }}
                >
                  {column}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {entries.length === SLICE_START_INDEX ? (
            <tr>
              <td
                colSpan={columns.length}
                style={{
                  color: 'var(--text-dim)',
                  padding: '2rem',
                  textAlign: 'center',
                }}
              >
                No data available — waiting for {series.toUpperCase()} feed…
              </td>
            </tr>
          ) : (
            entries.map((entry: TimingEntry, rowIndex: number) => {
              const isSelected: boolean = rowIndex === selectedRow;
              const rowRef: React.RefObject<HTMLTableRowElement> | undefined = isSelected ? selectedRowRef : undefined;
              const classColor: string | null = getClassColor(entry.class_name, classColors);
              const pitColor: string | null = getPitColor(entry.pit);

              return (
                <tr
                  key={entry.stable_id}
                  ref={rowRef}
                  style={{
                    backgroundColor: isSelected
                      ? 'var(--bg-selected)'
                      : undefined,
                    height: `${ROW_HEIGHT_PX}px`,
                  }}
                >
                  {columns.map((column: string, colIndex: number) => {
                    const isCompact: boolean = isCompactColumn(column);
                    const value: string = formatCellValue(entry, column);
                    const isPitColumn: boolean = column === 'Pit';

                    const cellColor: string | undefined = ((): string | undefined => {
                      if (isPitColumn && pitColor !== null) {
                        return pitColor;
                      }
                      if (column === 'Class' && classColor !== null) {
                        return classColor;
                      }
                      return undefined;
                    })();

                    return (
                      <td
                        key={`${entry.stable_id}-${column}`}
                        style={{
                          borderRight:
                            colIndex < lastColumnIndex
                              ? '1px solid var(--grid)'
                              : 'none',
                          color: cellColor,
                          fontFamily: isCompact ? 'monospace' : undefined,
                          overflow: 'hidden',
                          padding: '0.25rem 0.375rem',
                          textAlign: isCompact ? 'center' : 'left',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                        title={value}
                      >
                        {value}
                      </td>
                    );
                  })}
                </tr>
              );
            })
          )}
        </tbody>
      </table>
    </div>
  );
};
