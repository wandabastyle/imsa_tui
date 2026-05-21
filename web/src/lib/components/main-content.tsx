// Main app content component (authenticated view)
import type { JSX } from 'react';

import { HeaderBar } from './header-bar';
import { HelpModal } from './help-modal';
import { MessagesModal } from './messages-modal';
import { NlsLivetickerModal } from './nls-liveticker-modal';
import { SeriesModal } from './series-modal';
import { TimingTable } from './timing-table';
import type { AppState } from '../hooks';
import type { Series, TimingEntry } from '../types';

interface MainContentProps {
  state: AppState;
  activeSnapshot: { entries: TimingEntry[]; header: { class_colors?: Record<string, unknown> }; notices?: unknown[] } | null;
  activeEntries: TimingEntry[];
  activeSeries: Series;
  viewModeLabel: string;
  searchLabel: string;
  demoLabel: string;
  favCount: number;
  searchMatches: number[];
  onCloseHelp: () => void;
  onCloseMessages: () => void;
  onCloseNlsLiveticker: () => void;
  onPickSeries: (series: Series) => void;
}

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
    searchMatches,
    onCloseHelp,
    onCloseMessages,
    onCloseNlsLiveticker,
    onPickSeries,
  } = props;

  const firstMatchIndex = 0;

  return (
    <div className="app">
      <HeaderBar
        demoLabel={demoLabel}
        errorText={state.connectionErrors[firstMatchIndex] ?? ''}
        favCount={favCount}
        searchCurrentMatch={state.search.currentMatch}
        searchInputActive={state.search.inputActive}
        searchLabel={searchLabel}
        searchMatches={searchMatches.length}
        searchQuery={state.search.query}
        series={activeSeries}
        snapshot={activeSnapshot}
        viewModeLabel={viewModeLabel}
      />

      <TimingTable
        classColors={activeSnapshot?.header.class_colors ?? {}}
        entries={activeEntries}
        selectedRow={state.selectedRow}
        series={activeSeries}
      />

      <HelpModal onClose={onCloseHelp} open={state.showHelp} />

      <SeriesModal
        onPick={onPickSeries}
        open={state.showSeriesPicker}
        selectedIndex={state.seriesPickerIndex}
        selectedSeries={activeSeries}
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
    </div>
  );
};
