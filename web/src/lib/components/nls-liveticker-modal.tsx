// Nls-liveticker-modal.tsx - NLS liveticker modal
import { useEffect, useCallback, type JSX } from 'react';

import type { NlsLivetickerEntry } from '$lib/generated/web-shared';

const ZERO_LENGTH = 0;
const SLICE_START_INDEX = 0;

interface NlsLivetickerModalProps {
  entries: NlsLivetickerEntry[];
  lastError: string | null;
  lastUpdateUnixMs: bigint | null;
  onClose: () => void;
  open: boolean;
}

const MODAL_MAX_WIDTH = 640;
const MODAL_PADDING_REM = 1.5;
const MODAL_BACKDROP_Z_INDEX = 100;
const MODAL_CONTENT_Z_INDEX = 101;
const ENTRY_MESSAGE_MAX_LENGTH = 500;
const ENTRY_TIME_MAX_LENGTH = 8;
const MS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;

const formatAge = function formatAge(ms: number): string {
  const secs = Math.floor(ms / MS_PER_SECOND);
  if (secs < SECONDS_PER_MINUTE) {
    return `${secs}s ago`;
  }
  const mins = Math.floor(secs / SECONDS_PER_MINUTE);
  const remainingSecs = secs % SECONDS_PER_MINUTE;
  return `${mins}m ${remainingSecs}s ago`;
};

export const NlsLivetickerModal = function NlsLivetickerModal(
  props: NlsLivetickerModalProps,
): JSX.Element | null {
  const { entries, lastError, lastUpdateUnixMs, onClose, open } = props;

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        onClose();
      }
    },
    [onClose],
  );

  useEffect(() => {
    if (!open) {
      return (): void => {};
    }
    document.addEventListener('keydown', handleKeyDown);
    return (): void => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [open, handleKeyDown]);

  if (!open) {
    return null;
  }

  const now = Date.now();
  const lastUpdateMs =
    lastUpdateUnixMs === null ? null : Number(lastUpdateUnixMs);
  const updateAge = lastUpdateMs === null ? null : now - lastUpdateMs;

  return (
    <div
      className="nls-liveticker-modal-backdrop"
      onClick={onClose}
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
        className="nls-liveticker-modal-content"
        onClick={(event: React.MouseEvent<HTMLDivElement>): void => {
          event.stopPropagation();
        }}
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
            NLS Liveticker
          </h2>
          <button
            onClick={onClose}
            style={{
              backgroundColor: 'transparent',
              border: 'none',
              color: 'var(--text-dim)',
              cursor: 'pointer',
              fontSize: '1.25rem',
              lineHeight: 1,
              padding: '0.25rem',
            }}
            type="button"
          >
            ×
          </button>
        </div>

        {lastError !== null && lastError !== '' && (
          <div
            style={{
              backgroundColor: 'var(--danger)',
              borderRadius: '3px',
              color: '#fff',
              marginBottom: '1rem',
              padding: '0.5rem 0.75rem',
            }}
          >
            {lastError}
          </div>
        )}

        {entries.length === ZERO_LENGTH ? (
          <div
            style={{
              color: 'var(--text-dim)',
              padding: '2rem',
              textAlign: 'center',
            }}
          >
            {lastUpdateMs === null
              ? 'Waiting for NLS liveticker data…'
              : 'No entries available'}
          </div>
        ) : (
          <>
            <div
              style={{
                color: 'var(--text-dim)',
                fontSize: '0.75rem',
                marginBottom: '0.75rem',
              }}
            >
              {updateAge === null
                ? 'Last update: unknown'
                : `Last update: ${formatAge(updateAge)}`}
            </div>

            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '0.5rem',
              }}
            >
              {entries.map((entry) => {
                const truncatedDay = entry.day_label.slice(SLICE_START_INDEX, ENTRY_TIME_MAX_LENGTH);
                const truncatedTime = entry.time_text.slice(SLICE_START_INDEX, ENTRY_TIME_MAX_LENGTH);
                const truncatedMessage = entry.message.length > ENTRY_MESSAGE_MAX_LENGTH
                  ? `${entry.message.slice(SLICE_START_INDEX, ENTRY_MESSAGE_MAX_LENGTH)}…`
                  : entry.message;

                return (
                  <div
                    key={entry.id}
                    style={{
                      backgroundColor: 'var(--bg-panel)',
                      border: '1px solid var(--border)',
                      borderRadius: '3px',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '0.25rem',
                      padding: '0.5rem 0.75rem',
                    }}
                  >
                    <div
                      style={{
                        alignItems: 'center',
                        display: 'flex',
                        gap: '0.5rem',
                      }}
                    >
                      <span
                        style={{
                          color: 'var(--accent)',
                          fontFamily: 'monospace',
                          fontSize: '0.75rem',
                        }}
                      >
                        {truncatedDay}
                      </span>
                      <span
                        style={{
                          color: 'var(--text-dim)',
                          fontFamily: 'monospace',
                          fontSize: '0.75rem',
                        }}
                      >
                        {truncatedTime}
                      </span>
                    </div>
                    <span
                      style={{
                        color: 'var(--text)',
                        fontSize: '0.85rem',
                        lineHeight: 1.4,
                      }}
                    >
                      {truncatedMessage}
                    </span>
                  </div>
                );
              })}
            </div>
          </>
        )}

        <div
          style={{
            borderTop: '1px solid var(--border)',
            color: 'var(--text-dim)',
            fontSize: '0.75rem',
            marginTop: '1rem',
            paddingTop: '0.5rem',
          }}
        >
          Press Esc to close
        </div>
      </div>
    </div>
  );
};
