// Header-bar.tsx - Status header matching TUI metadata order and flag-driven theming
import type { JSX } from 'react';

import type { SeriesSnapshot } from '../generated/web-shared';

interface HeaderBarProps {
  demoLabel: string;
  favCount: number;
  searchCurrentMatch: number;
  searchInputActive: boolean;
  searchLabel: string;
  searchMatches: number;
  searchQuery: string;
  snapshot: SeriesSnapshot | null;
  viewModeLabel: string;
}

const MS_PER_SECOND = 1000;
const INDEX_OFFSET = 1;
const MIN_VALUE = 0;
const SLICE_OFFSET = 1;

const normalizeImsaLabel = function normalizeImsaLabel(raw: string): string {
  const trimmed = raw.trim();
  const lower = trimmed.toLowerCase();
  if (lower.includes('weathertech')) {
    const idx = Math.max(
      trimmed.lastIndexOf('-'),
      trimmed.lastIndexOf('–'),
      trimmed.lastIndexOf('—'),
    );
    if (idx >= MIN_VALUE) {
      return trimmed.slice(idx + SLICE_OFFSET).trim();
    }
  }
  return trimmed;
};

export const HeaderBar = function HeaderBar(props: HeaderBarProps): JSX.Element {
  const {
    demoLabel,
    favCount,
    searchCurrentMatch,
    searchInputActive,
    searchLabel,
    searchMatches,
    searchQuery,
    snapshot,
    viewModeLabel,
  } = props;

  const statusText = snapshot?.status ?? 'Starting live timing...';

  // Event name formatting
  const rawEventName = snapshot?.header.event_name;
  const eventText =
    rawEventName === undefined || rawEventName.trim() === '' ? '-' : rawEventName.trim();

  // Session name formatting with IMSA normalization
  const rawSessionName = snapshot?.header.session_name;
  const sessionText =
    rawSessionName === undefined || rawSessionName.trim() === ''
      ? '-'
      : normalizeImsaLabel(rawSessionName);

  const rawTimeToGo = snapshot?.header.time_to_go;
  const timeToGo = rawTimeToGo === undefined || rawTimeToGo === '' ? '-' : rawTimeToGo;
  const flag = snapshot?.header.flag ?? '';
  const displayFlag = flag.length > MIN_VALUE ? flag : '-';

  // Age calculation from last_update_unix_ms
  const rawUpdateMs = snapshot?.last_update_unix_ms;
  const ageText =
    rawUpdateMs !== undefined && rawUpdateMs !== null
      ? `Upd ${Math.max(
          MIN_VALUE,
          Math.floor((Date.now() - Number(rawUpdateMs)) / MS_PER_SECOND),
        )}s`
      : 'Upd -';

  // Error from snapshot.last_error
  const rawError = snapshot?.last_error;
  const errorText = rawError ?? '';

  // Search count display logic
  const searchCount = ((): JSX.Element | null => {
    if (searchMatches > MIN_VALUE) {
      return (
        <span className="search-count">
          ({searchCurrentMatch + INDEX_OFFSET}/{searchMatches})
        </span>
      );
    }
    if (searchQuery) {
      return <span className="search-count">(0/0)</span>;
    }
    return null;
  })();

  return (
    <section className="header" data-flag={displayFlag.toLowerCase()}>
      <div className="line">
        {statusText} | {eventText} | {sessionText} | TTE {timeToGo} | Mode {viewModeLabel} |{' '}
        <strong>{displayFlag}</strong> | {ageText} | Favs {favCount}
      </div>
      <div className="line dim">
        Keys: h help | d demo | {searchLabel || 'Search: -'} {demoLabel}
        {errorText ? ` | Error: ${errorText}` : ''}
      </div>

      {(searchInputActive || searchQuery.length > MIN_VALUE) && (
        <div className="search-strip" role="search" aria-label="Search">
          <span className="search-label">Search:</span>
          <input
            aria-label="Search query"
            className="search-input"
            placeholder="Type to search..."
            readOnly={!searchInputActive}
            type="text"
            value={searchQuery}
          />
          {searchCount}
          {searchInputActive && <span className="search-cursor">_</span>}
        </div>
      )}
    </section>
  );
};
