// Series-modal.tsx - Series picker modal
import { useEffect, useCallback, type JSX } from 'react';

import type { Series } from '../generated/web-shared';
import { ALL_SERIES } from '../types';

interface SeriesModalProps {
  onPick: (series: Series) => void;
  open: boolean;
  selectedIndex: number;
  selectedSeries: Series;
}

const MODAL_MAX_WIDTH = 320;
const MODAL_PADDING_REM = 1.5;
const MODAL_BACKDROP_Z_INDEX = 100;
const MODAL_CONTENT_Z_INDEX = 101;

interface SeriesOption {
  label: string;
  series: Series;
}

const SERIES_OPTIONS: SeriesOption[] = ALL_SERIES.map((series) => ({
  label: series.toUpperCase(),
  series,
}));

export const SeriesModal = function SeriesModal(
  props: SeriesModalProps,
): JSX.Element | null {
  const { onPick, open, selectedIndex, selectedSeries } = props;

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        // Let App.tsx handle the escape
      }
    },
    [],
  );

  useEffect(() => {
    if (!open) {
      return function noop(): void {
        // Intentionally empty
      };
    }
    document.addEventListener('keydown', handleKeyDown);
    return (): void => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [open, handleKeyDown]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="series-modal-backdrop"
      style={{
        alignItems: 'center',
        backgroundColor: 'rgba(0, 0, 0, 0.7)',
        bottom: 0,
        display: 'flex',
        justifyContent: 'center',
        left: 0,
        position: 'fixed',
        right: 0,
        top: 0,
        zIndex: MODAL_BACKDROP_Z_INDEX,
      }}
    >
      <div
        className="series-modal-content"
        style={{
          backgroundColor: 'var(--bg-modal)',
          border: '1px solid var(--border)',
          borderRadius: '4px',
          maxHeight: '80vh',
          maxWidth: MODAL_MAX_WIDTH,
          overflow: 'auto',
          padding: `${MODAL_PADDING_REM}rem`,
          width: '90vw',
          zIndex: MODAL_CONTENT_Z_INDEX,
        }}
      >
        <div
          style={{
            alignItems: 'center',
            borderBottom: '1px solid var(--border)',
            display: 'flex',
            justifyContent: 'space-between',
            marginBottom: '1rem',
            paddingBottom: '0.5rem',
          }}
        >
          <h2
            style={{
              color: 'var(--text)',
              fontSize: '1rem',
              fontWeight: 600,
              margin: 0,
            }}
          >
            Select Series
          </h2>
        </div>

        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '0.25rem',
          }}
        >
          {SERIES_OPTIONS.map((option, index) => {
            const isSelected = index === selectedIndex;
            const isCurrent = option.series === selectedSeries;

            return (
              <button
                key={option.series}
                onClick={(): void => {
                  onPick(option.series);
                }}
                style={{
                  alignItems: 'center',
                  backgroundColor: isSelected
                    ? 'var(--bg-selected)'
                    : 'var(--bg-panel)',
                  border: isSelected
                    ? '1px solid var(--accent)'
                    : '1px solid var(--border)',
                  borderRadius: '3px',
                  color: isCurrent ? 'var(--accent)' : 'var(--text)',
                  cursor: 'pointer',
                  display: 'flex',
                  fontFamily: 'inherit',
                  fontSize: '0.9rem',
                  justifyContent: 'space-between',
                  padding: '0.5rem 0.75rem',
                  textAlign: 'left',
                }}
                type="button"
              >
                <span>{option.label}</span>
                {isCurrent && (
                  <span
                    style={{
                      color: 'var(--text-dim)',
                      fontSize: '0.7rem',
                    }}
                  >
                    current
                  </span>
                )}
              </button>
            );
          })}
        </div>

        <div
          style={{
            borderTop: '1px solid var(--border)',
            color: 'var(--text-dim)',
            fontSize: '0.75rem',
            marginTop: '1rem',
            paddingTop: '0.5rem',
          }}
        >
          ↑↓ or jk to navigate, Enter to select, Esc to cancel
        </div>
      </div>
    </div>
  );
};
