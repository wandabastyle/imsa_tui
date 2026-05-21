// Header-bar.tsx - Status header with series info, timing, flag, search
import type { JSX } from 'react';

import type { SeriesSnapshot, Series } from '../generated/web-shared';

const DEMO_LABEL_MAX_LENGTH = 20;
const HEADER_UPDATE_AGE_THRESHOLD_MS = 10_000;
const SEARCH_LABEL_MAX_LENGTH = 40;
const SERIES_LABEL_MAX_LENGTH = 10;
const MINIMUM_FAV_COUNT = 1;
const MINIMUM_SEARCH_MATCHES = 1;
const SLICE_START_INDEX = 0;

interface HeaderBarProps {
  demoLabel: string;
  errorText: string;
  favCount: number;
  searchCurrentMatch: number;
  searchInputActive: boolean;
  searchLabel: string;
  searchMatches: number;
  searchQuery: string;
  series: Series;
  snapshot: SeriesSnapshot | null;
  viewModeLabel: string;
}

const MS_PER_SECOND = 1000;
const SECS_PER_MINUTE = 60;

const formatAge = function formatAge(ms: number): string {
  const secs = Math.floor(ms / MS_PER_SECOND);
  if (secs < SECS_PER_MINUTE) {
    return `${secs}s`;
  }
  const mins = Math.floor(secs / SECS_PER_MINUTE);
  const remainingSecs = secs % SECS_PER_MINUTE;
  return `${mins}m ${remainingSecs}s`;
};

const getFlagTheme = function getFlagTheme(flag: string): { bg: string; fg: string } {
  const upperFlag = flag.toUpperCase();
  switch (upperFlag) {
    case 'CHECKERED': {
      return { bg: '#eee', fg: '#000' };
    }
    case 'GREEN': {
      return { bg: 'var(--ok)', fg: '#000' };
    }
    case 'RED': {
      return { bg: 'var(--danger)', fg: '#fff' };
    }
    case 'SC':
    case 'VSC':
    case 'YELLOW': {
      return { bg: 'var(--warn)', fg: '#000' };
    }
    default: {
      return { bg: 'var(--bg-panel)', fg: 'var(--text)' };
    }
  }
};

const buildSearchDisplay = function buildSearchDisplay(
  searchInputActive: boolean,
  searchQuery: string,
  searchLabel: string,
): string {
  if (searchInputActive) {
    return `/${searchQuery}`;
  }
  if (searchLabel) {
    return searchLabel.slice(SLICE_START_INDEX, SEARCH_LABEL_MAX_LENGTH);
  }
  return '';
};

const calculateUpdateAge = function calculateUpdateAge(snapshot: SeriesSnapshot | null): {
  ageMs: number | null;
  isStale: boolean;
} {
  const now = Date.now();
  const lastUpdateUnixMsValue = snapshot?.last_update_unix_ms;
  const lastUpdateMs =
    lastUpdateUnixMsValue !== undefined && lastUpdateUnixMsValue !== null
      ? Number(lastUpdateUnixMsValue)
      : null;
  const ageMs = lastUpdateMs === null ? null : now - lastUpdateMs;
  const isStale = ageMs === null ? false : ageMs > HEADER_UPDATE_AGE_THRESHOLD_MS;
  return { ageMs, isStale };
};

const DemoBadge = function DemoBadge(): JSX.Element {
  return (
    <span
      style={{
        backgroundColor: 'var(--warn)',
        borderRadius: '2px',
        color: '#000',
        fontSize: '0.75rem',
        fontWeight: 700,
        marginLeft: '0.5rem',
        padding: '0 0.25rem',
      }}
    >
      DEMO
    </span>
  );
};

const FlagIndicator = function FlagIndicator({
  flag,
  theme,
}: {
  flag: string;
  theme: { bg: string; fg: string };
}): JSX.Element {
  return (
    <span
      style={{
        backgroundColor: theme.bg,
        border: `1px solid ${theme.fg}`,
        borderRadius: '3px',
        fontSize: '0.8rem',
        fontWeight: 700,
        minWidth: '5ch',
        padding: '0.125rem 0.5rem',
        textAlign: 'center',
        textTransform: 'uppercase',
      }}
    >
      {flag || '—'}
    </span>
  );
};

const TimerDisplay = function TimerDisplay({ time }: { time: string }): JSX.Element {
  return (
    <span
      style={{
        fontFamily: 'monospace',
        fontSize: '0.9rem',
        fontWeight: 600,
        minWidth: '8ch',
        textAlign: 'right',
      }}
    >
      {time}
    </span>
  );
};

const AgeDisplay = function AgeDisplay({
  ageMs,
  isStale,
}: {
  ageMs: number | null;
  isStale: boolean;
}): JSX.Element {
  const ageText = ageMs === null ? '—' : `${formatAge(ageMs)} ago`;
  return (
    <span
      style={{
        color: isStale ? 'var(--danger)' : 'var(--text-dim)',
        fontSize: '0.75rem',
      }}
    >
      {ageText}
    </span>
  );
};

const ViewModeBadge = function ViewModeBadge({ label }: { label: string }): JSX.Element {
  return (
    <span
      style={{
        backgroundColor: 'var(--bg-muted)',
        borderRadius: '2px',
        color: 'var(--text-dim)',
        fontSize: '0.75rem',
        padding: '0.125rem 0.375rem',
      }}
    >
      {label}
    </span>
  );
};

const FavIndicator = function FavIndicator({ count }: { count: number }): JSX.Element | null {
  if (count < MINIMUM_FAV_COUNT) {
    return null;
  }
  return (
    <span
      style={{
        color: 'var(--accent)',
        fontSize: '0.75rem',
      }}
    >
      ★ {count}
    </span>
  );
};

const SearchIndicator = function SearchIndicator({
  display,
  matches,
}: {
  display: string;
  matches: number;
}): JSX.Element | null {
  if (!display) {
    return null;
  }
  return (
    <span
      style={{
        backgroundColor: 'var(--bg-search)',
        borderRadius: '2px',
        color: 'var(--text)',
        fontSize: '0.8rem',
        padding: '0.125rem 0.375rem',
      }}
    >
      {display}
      {matches >= MINIMUM_SEARCH_MATCHES && (
        <span
          style={{
            color: 'var(--text-dim)',
            fontSize: '0.7rem',
            marginLeft: '0.25rem',
          }}
        >
          ({matches})
        </span>
      )}
    </span>
  );
};

const ErrorText = function ErrorText({ text }: { text: string }): JSX.Element | null {
  if (!text) {
    return null;
  }
  return (
    <span
      style={{
        color: 'var(--danger)',
        fontSize: '0.75rem',
      }}
    >
      {text}
    </span>
  );
};

export const HeaderBar = function HeaderBar(props: HeaderBarProps): JSX.Element {
  const {
    demoLabel,
    errorText,
    favCount,
    searchInputActive,
    searchLabel,
    searchMatches,
    searchQuery,
    series,
    snapshot,
    viewModeLabel,
  } = props;

  const sessionName = snapshot?.header.session_name ?? '-';
  const eventName = snapshot?.header.event_name ?? '-';
  const flag = snapshot?.header.flag ?? '';
  const timeToGo = snapshot?.header.time_to_go ?? '-';
  const trackName = snapshot?.header.track_name ?? '-';
  const dayTime = snapshot?.header.day_time ?? '-';

  const flagTheme = getFlagTheme(flag);
  const { ageMs, isStale } = calculateUpdateAge(snapshot);
  const searchDisplay = buildSearchDisplay(searchInputActive, searchQuery, searchLabel);

  // Truncate values
  const truncatedSeries = series.slice(SLICE_START_INDEX, SERIES_LABEL_MAX_LENGTH).toUpperCase();
  const truncatedDemo = demoLabel.slice(SLICE_START_INDEX, DEMO_LABEL_MAX_LENGTH);

  return (
    <div
      className="header-bar"
      style={{
        backgroundColor: flagTheme.bg,
        borderBottom: '1px solid var(--border)',
        color: flagTheme.fg,
        display: 'flex',
        flexDirection: 'column',
        fontSize: '0.85rem',
        fontWeight: 500,
        padding: '0.5rem 0.75rem',
      }}
    >
      <div
        style={{
          alignItems: 'center',
          display: 'flex',
          gap: '1rem',
          justifyContent: 'space-between',
        }}
      >
        <div style={{ alignItems: 'center', display: 'flex', gap: '0.75rem' }}>
          <span style={{ fontWeight: 700 }}>{truncatedSeries}</span>
          <span style={{ opacity: 0.9 }}>{eventName}</span>
          <span style={{ opacity: 0.8 }}>|</span>
          <span>{sessionName}</span>
          {truncatedDemo && <DemoBadge />}
        </div>

        <div style={{ alignItems: 'center', display: 'flex', gap: '1rem' }}>
          <span>{trackName}</span>
          <span style={{ opacity: 0.8 }}>{dayTime}</span>
          <FlagIndicator flag={flag} theme={flagTheme} />
          <TimerDisplay time={timeToGo} />
        </div>
      </div>

      <div
        style={{
          alignItems: 'center',
          display: 'flex',
          gap: '1rem',
          justifyContent: 'space-between',
          marginTop: '0.25rem',
        }}
      >
        <div style={{ alignItems: 'center', display: 'flex', gap: '0.75rem' }}>
          <AgeDisplay ageMs={ageMs} isStale={isStale} />
          <ViewModeBadge label={viewModeLabel} />
          <FavIndicator count={favCount} />
        </div>

        <div style={{ alignItems: 'center', display: 'flex', gap: '0.75rem' }}>
          <ErrorText text={errorText} />
          <SearchIndicator display={searchDisplay} matches={searchMatches} />
          <span
            style={{
              color: 'var(--text-dim)',
              fontSize: '0.7rem',
            }}
          >
            h:help
          </span>
        </div>
      </div>
    </div>
  );
};
