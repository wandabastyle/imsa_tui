import { useMemo, useRef, type JSX } from 'react';

import { getColumnsForSeries } from '../../table/columns';
import { useTableDimensions } from '../../table/use-table-dimensions';
import { GroupedTable } from './grouped-table';
import { SingleTable } from './single-table';
import type { GapAnchorInfo, TimingTableProps } from './types';
import { usePitTrackers } from './utils';

const DEFAULT_CURRENT_MATCH = 0;
const DEFAULT_MIN_ROWS_PER_GROUP = 5;
const FALLBACK_ZERO = 0;

export const TimingTable = (props: TimingTableProps): JSX.Element => {
  const {
    classColors,
    entries,
    selectedRow,
    series,
    title = 'Overall',
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    searchMatches = [],
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    currentSearchMatch = DEFAULT_CURRENT_MATCH,
    gapAnchorStableId = null,
    favourites = new Set(),
    groupedSections = [],
    isGroupedMode = false,
    markedStableId = null,
    minRowsPerGroup = DEFAULT_MIN_ROWS_PER_GROUP,
    loading = false,
  } = props;

  const columns = useMemo(() => getColumnsForSeries(series), [series]);
  const pitTrackers = usePitTrackers(entries, series);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const dimensions = useTableDimensions(scrollContainerRef);

  const gapAnchor = useMemo<GapAnchorInfo | null>(() => {
    if (gapAnchorStableId === null || gapAnchorStableId === '') {
      return null;
    }
    const entry = entries.find((foundEntry) => foundEntry.stable_id === gapAnchorStableId);
    if (!entry) {
      return null;
    }
    return {
      laps: Number.parseInt(entry.laps.trim(), 10) || FALLBACK_ZERO,
      position: entry.position,
      stableId: entry.stable_id,
    };
  }, [entries, gapAnchorStableId]);

  return (
    <section className="table-wrap">
      {title && <div className="table-title">{title}</div>}
      <div className="table-scroll" ref={scrollContainerRef}>
        {isGroupedMode ? (
          <GroupedTable
            scrollContainerRef={scrollContainerRef}
            groupedSections={groupedSections}
            columns={columns}
            selectedRow={selectedRow}
            series={series}
            classColors={classColors}
            gapAnchor={gapAnchor}
            pitTrackers={pitTrackers}
            favourites={favourites}
            markedStableId={markedStableId}
            title={title}
            loading={loading}
            viewportHeight={dimensions?.viewportHeight ?? null}
            rowHeight={dimensions?.rowHeight ?? null}
            minRowsPerGroup={minRowsPerGroup}
          />
        ) : (
          <SingleTable
            scrollContainerRef={scrollContainerRef}
            entries={entries}
            columns={columns}
            selectedRow={selectedRow}
            series={series}
            classColors={classColors}
            gapAnchor={gapAnchor}
            pitTrackers={pitTrackers}
            favourites={favourites}
            markedStableId={markedStableId}
            title={title}
            loading={loading}
          />
        )}
      </div>
    </section>
  );
};
