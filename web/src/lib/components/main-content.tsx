// Main app content component (authenticated view)
import type { JSX } from 'react';

import type { AppState } from '../hooks';
import { ALL_SERIES, type Series, type SeriesSnapshot, type TimingEntry } from '../types';

import { GroupModal } from './group-modal';
import { HeaderBar } from './header-bar';
import { HelpModal } from './help-modal';
import { MessagesModal } from './messages-modal';
import { NlsLivetickerModal } from './nls-liveticker-modal';
import { SeriesModal } from './series-modal';
import { TimingTable } from './timing-table';

interface GroupedSection {
  name: string;
  entries: TimingEntry[];
  start: number;
}

interface MainContentProps {
  state: AppState;
  activeSnapshot: SeriesSnapshot | null;
  activeEntries: TimingEntry[];
  activeSeries: Series;
  viewModeLabel: string;
  searchLabel: string;
  demoLabel: string;
  favCount: number;
  groups: [string, TimingEntry[]][];
  groupedSections: GroupedSection[];
  groupPickerIndex: number;
  gapAnchorStableId: string | null;
  markedStableId: string | null;
  searchMatches: number[];
  searchCurrentMatch: number;
  onCloseGroupPicker: () => void;
  onCloseHelp: () => void;
  onCloseMessages: () => void;
  onCloseNlsLiveticker: () => void;
  onCloseSeriesPicker: () => void;
  onPickGroup: (index: number) => void;
  onPickSeries: (series: Series) => void;
  onSignOut: () => void;
}

// Convert groups to string array for GroupModal
const extractGroupNames = (groups: [string, TimingEntry[]][]): string[] =>
  groups.map(([name]) => name);

export const MainContent = (props: MainContentProps): JSX.Element => {
  const {
    state,
    activeSnapshot,
    activeEntries,
    activeSeries,
    viewModeLabel,
    searchLabel,
    demoLabel,
    favCount,
    groups,
    groupedSections,
    groupPickerIndex,
    gapAnchorStableId,
    markedStableId,
    searchMatches,
    onCloseGroupPicker,
    onCloseHelp,
    onCloseMessages,
    onCloseNlsLiveticker,
    onCloseSeriesPicker,
    onPickGroup,
    onPickSeries,
    onSignOut,
  } = props;

  // Convert groups for GroupModal which expects string[]
  const groupNames = extractGroupNames(groups);

  return (
    <main>
      <div className="header-row">
        <HeaderBar
          demoLabel={demoLabel}
          favCount={favCount}
          searchCurrentMatch={state.search.currentMatch}
          searchInputActive={state.search.inputActive}
          searchLabel={searchLabel}
          searchMatches={searchMatches.length}
          searchQuery={state.search.query}
          snapshot={activeSnapshot}
          viewModeLabel={viewModeLabel}
        />
        <button className="logout-btn" onClick={onSignOut} type="button">
          Logout
        </button>
      </div>

      <TimingTable
        classColors={activeSnapshot?.header.class_colors ?? {}}
        entries={activeEntries}
        selectedRow={state.selectedRow}
        series={activeSeries}
        title={viewModeLabel}
        groupedSections={groupedSections}
        isGroupedMode={state.viewMode.kind === 'grouped'}
        markedStableId={markedStableId}
        favourites={state.favourites}
        gapAnchorStableId={gapAnchorStableId}
        searchMatches={searchMatches}
        currentSearchMatch={state.search.currentMatch}
        minRowsPerGroup={state.minRowsPerGroup}
      />

      <HelpModal onClose={onCloseHelp} open={state.showHelp} />

      <SeriesModal
        onClose={onCloseSeriesPicker}
        onPick={onPickSeries}
        open={state.showSeriesPicker}
        selectedSeries={ALL_SERIES[state.seriesPickerIndex]}
      />

      <GroupModal
        open={state.showGroupPicker}
        groups={groupNames}
        selectedIndex={groupPickerIndex}
        onPick={onPickGroup}
        onClose={onCloseGroupPicker}
      />

      <MessagesModal
        notices={activeSnapshot?.notices ?? []}
        onClose={onCloseMessages}
        open={state.showMessages}
      />

      <NlsLivetickerModal
        entries={state.nlsLiveticker.entries}
        lastError={state.nlsLiveticker.lastError}
        lastUpdateUnixMs={state.nlsLiveticker.lastUpdateUnixMs}
        onClose={onCloseNlsLiveticker}
        open={state.showNlsLiveticker}
      />
    </main>
  );
};
