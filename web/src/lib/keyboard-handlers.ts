// Keyboard handlers for the App component
import type { KeyboardEvent } from 'react';

import type { AppState, UseAppStateReturn } from './hooks';
import { ALL_SERIES } from './types';

const DEFAULT_GROUP_PICKER_INDEX = 0;
const DEFAULT_SELECTED_ROW = 0;
const FIRST_MATCH_INDEX = 0;
const INDEX_DECREMENT = -1;
const INDEX_INCREMENT = 1;
const JUMP_BACKWARD = -1;
const JUMP_FORWARD = 1;
const KEY_LENGTH_SINGLE = 1;
const MINIMUM_LENGTH = 0;
const PAGE_JUMP_SIZE = 10;
const SLICE_START_INDEX = 0;
const SLICE_REMOVE_LAST = 1;
const ZERO_LENGTH = 0;

interface GroupPickerHandlers {
  groupPickerIndex: number;
  groupsLength: number;
  selectGroup: (index: number) => void;
  setState: UseAppStateReturn['setState'];
}

export const handleGroupPickerKeydown = (
  event: KeyboardEvent,
  handlers: GroupPickerHandlers,
): void => {
  const { groupPickerIndex, groupsLength, selectGroup, setState } = handlers;

  if (event.key === 'Escape') {
    setState((prev: AppState) => ({ ...prev, showGroupPicker: false }));
    event.preventDefault();
  } else if (event.key === 'ArrowDown' || event.key === 'j') {
    setState((prev: AppState) => ({
      ...prev,
      groupPickerIndex: (prev.groupPickerIndex + INDEX_INCREMENT) % groupsLength,
    }));
    event.preventDefault();
  } else if (event.key === 'ArrowUp' || event.key === 'k') {
    setState((prev: AppState) => ({
      ...prev,
      groupPickerIndex:
        prev.groupPickerIndex === DEFAULT_GROUP_PICKER_INDEX
          ? groupsLength + INDEX_DECREMENT
          : prev.groupPickerIndex + INDEX_DECREMENT,
    }));
    event.preventDefault();
  } else if (event.key === 'Enter') {
    selectGroup(groupPickerIndex);
    event.preventDefault();
  }
};

interface SearchHandlers {
  searchMatches: number[];
  setState: UseAppStateReturn['setState'];
}

export const handleSearchKeydown = (event: KeyboardEvent, handlers: SearchHandlers): void => {
  const { searchMatches, setState } = handlers;

  if (event.key === 'Escape') {
    setState((prev: AppState) => ({
      ...prev,
      search: { ...prev.search, inputActive: false },
    }));
    event.preventDefault();
  } else if (event.key === 'Enter') {
    setState((prev: AppState) => ({
      ...prev,
      search: { ...prev.search, currentMatch: FIRST_MATCH_INDEX, inputActive: false },
    }));
    if (searchMatches.length > MINIMUM_LENGTH) {
      setState((prev: AppState) => ({ ...prev, selectedRow: searchMatches[FIRST_MATCH_INDEX] }));
    }
    event.preventDefault();
  } else if (event.key === 'Backspace') {
    setState((prev: AppState) => ({
      ...prev,
      search: {
        ...prev.search,
        query: prev.search.query.slice(SLICE_START_INDEX, -SLICE_REMOVE_LAST),
      },
    }));
    event.preventDefault();
  } else if (event.key.length === KEY_LENGTH_SINGLE && !event.ctrlKey && !event.metaKey) {
    setState((prev: AppState) => ({
      ...prev,
      search: {
        ...prev.search,
        query: prev.search.query + event.key,
      },
    }));
    event.preventDefault();
  }
};

interface SeriesPickerHandlers {
  chooseSeries: (series: string) => void;
  seriesPickerIndex: number;
  setState: UseAppStateReturn['setState'];
}

export const handleSeriesPickerKeydown = (event: KeyboardEvent, handlers: SeriesPickerHandlers): void => {
  const { chooseSeries, seriesPickerIndex, setState } = handlers;

  if (event.key === 'Escape') {
    setState((prev: AppState) => ({ ...prev, showSeriesPicker: false }));
    event.preventDefault();
  } else if (event.key === 'ArrowDown' || event.key === 'j') {
    setState((prev: AppState) => ({
      ...prev,
      seriesPickerIndex: (prev.seriesPickerIndex + INDEX_INCREMENT) % ALL_SERIES.length,
    }));
    event.preventDefault();
  } else if (event.key === 'ArrowUp' || event.key === 'k') {
    setState((prev: AppState) => ({
      ...prev,
      seriesPickerIndex:
        prev.seriesPickerIndex === DEFAULT_GROUP_PICKER_INDEX
          ? ALL_SERIES.length + INDEX_DECREMENT
          : prev.seriesPickerIndex + INDEX_DECREMENT,
    }));
    event.preventDefault();
  } else if (event.key === 'Enter') {
    chooseSeries(ALL_SERIES[seriesPickerIndex]);
    event.preventDefault();
  }
};

interface MainKeydownHandlers {
  activeEntriesLength: number;
  cycleView: () => void;
  jumpFavourite: () => void;
  jumpSearch: (delta: number) => void;
  refreshNlsLiveticker: () => Promise<void>;
  setState: UseAppStateReturn['setState'];
  shiftSelection: (delta: number) => void;
  showHelp: boolean;
  showNlsLiveticker: boolean;
  toggleDemoMode: () => Promise<void>;
  toggleFavourite: () => Promise<void>;
}

export const handleMainKeydown = (event: KeyboardEvent, handlers: MainKeydownHandlers): void => {
  const {
    activeEntriesLength,
    cycleView,
    jumpFavourite,
    jumpSearch,
    refreshNlsLiveticker,
    setState,
    shiftSelection,
    showHelp,
    showNlsLiveticker,
    toggleDemoMode,
    toggleFavourite,
  } = handlers;

  switch (event.key) {
    case 'Escape': {
      if (showHelp) {
        setState((prev: AppState) => ({ ...prev, showHelp: false }));
        event.preventDefault();
      }
      break;
    }
    case 'h':
    case '?': {
      setState((prev: AppState) => ({ ...prev, showHelp: !prev.showHelp }));
      event.preventDefault();
      break;
    }
    case 'g': {
      cycleView();
      event.preventDefault();
      break;
    }
    case 'G': {
      setState((prev: AppState) => ({
        ...prev,
        groupPickerIndex:
          prev.viewMode.kind === 'class' && prev.groups.length > ZERO_LENGTH
            ? Math.min(prev.viewMode.index, prev.groups.length + INDEX_DECREMENT)
            : DEFAULT_GROUP_PICKER_INDEX,
        showGroupPicker: true,
      }));
      event.preventDefault();
      break;
    }
    case 'o': {
      setState((prev: AppState) => ({
        ...prev,
        gapAnchorStableId: null,
        selectedRow: DEFAULT_SELECTED_ROW,
        viewMode: { kind: 'overall' },
      }));
      event.preventDefault();
      break;
    }
    case 't': {
      setState((prev: AppState) => ({
        ...prev,
        seriesPickerIndex: ALL_SERIES.indexOf(prev.activeSeries),
        showSeriesPicker: true,
      }));
      event.preventDefault();
      break;
    }
    case 'ArrowDown':
    case 'j': {
      shiftSelection(JUMP_FORWARD);
      event.preventDefault();
      break;
    }
    case 'ArrowUp':
    case 'k': {
      shiftSelection(JUMP_BACKWARD);
      event.preventDefault();
      break;
    }
    case 'PageDown': {
      shiftSelection(PAGE_JUMP_SIZE);
      event.preventDefault();
      break;
    }
    case 'PageUp': {
      shiftSelection(-PAGE_JUMP_SIZE);
      event.preventDefault();
      break;
    }
    case 'Home': {
      setState((prev: AppState) => ({ ...prev, selectedRow: DEFAULT_SELECTED_ROW }));
      event.preventDefault();
      break;
    }
    case 'End': {
      setState((prev: AppState) => ({
        ...prev,
        selectedRow: Math.max(activeEntriesLength + INDEX_DECREMENT, DEFAULT_SELECTED_ROW),
      }));
      event.preventDefault();
      break;
    }
    case ' ': {
      void toggleFavourite();
      event.preventDefault();
      break;
    }
    case 'f': {
      jumpFavourite();
      event.preventDefault();
      break;
    }
    case 's': {
      setState((prev: AppState) => ({
        ...prev,
        search: {
          currentMatch: 0,
          inputActive: true,
          matches: [],
          query: '',
        },
      }));
      event.preventDefault();
      break;
    }
    case 'n': {
      jumpSearch(JUMP_FORWARD);
      event.preventDefault();
      break;
    }
    case 'p': {
      jumpSearch(JUMP_BACKWARD);
      event.preventDefault();
      break;
    }
    case 'd': {
      void toggleDemoMode();
      event.preventDefault();
      break;
    }
    case 'l': {
      setState((prev: AppState) => ({
        ...prev,
        showHelp: false,
        showMessages: false,
        showNlsLiveticker: !prev.showNlsLiveticker,
      }));
      if (!showNlsLiveticker) {
        void refreshNlsLiveticker();
      }
      event.preventDefault();
      break;
    }
    case 'm': {
      setState((prev: AppState) => ({
        ...prev,
        showHelp: false,
        showMessages: !prev.showMessages,
        showNlsLiveticker: false,
      }));
      event.preventDefault();
      break;
    }
    default: {
      break;
    }
  }
};
