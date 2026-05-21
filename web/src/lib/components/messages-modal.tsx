// Messages-modal.tsx - Race messages modal
import { useEffect, useCallback, type JSX } from 'react';

import type { TimingNotice } from '../generated/web-shared';

const ZERO_LENGTH = 0;
const SLICE_START_INDEX = 0;

interface MessagesModalProps {
  notices: TimingNotice[];
  onClose: () => void;
  open: boolean;
}

const MODAL_MAX_WIDTH = 640;
const MODAL_PADDING_REM = 1.5;
const MODAL_BACKDROP_Z_INDEX = 100;
const MODAL_CONTENT_Z_INDEX = 101;
const NOTICE_TIME_MAX_LENGTH = 12;
const NOTICE_TEXT_MAX_LENGTH = 500;

export const MessagesModal = function MessagesModal(
  props: MessagesModalProps,
): JSX.Element | null {
  const { notices, onClose, open } = props;

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

  // Using sort on a spread copy is equivalent to toSorted() for ES2024
  const sortedNotices: TimingNotice[] = [...notices].sort(
    (left: TimingNotice, right: TimingNotice): number =>
      right.time.localeCompare(left.time),
  );

  return (
    <div
      className="messages-modal-backdrop"
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
        className="messages-modal-content"
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
            Race Messages
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

        {sortedNotices.length === ZERO_LENGTH ? (
          <div
            style={{
              color: 'var(--text-dim)',
              padding: '2rem',
              textAlign: 'center',
            }}
          >
            No messages available
          </div>
        ) : (
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: '0.5rem',
            }}
          >
            {sortedNotices.map((notice) => {
              const truncatedTime = notice.time.slice(SLICE_START_INDEX, NOTICE_TIME_MAX_LENGTH);
              const truncatedText = notice.text.length > NOTICE_TEXT_MAX_LENGTH
                ? `${notice.text.slice(SLICE_START_INDEX, NOTICE_TEXT_MAX_LENGTH)}…`
                : notice.text;

              return (
                <div
                  key={notice.id}
                  style={{
                    backgroundColor: 'var(--bg-panel)',
                    border: '1px solid var(--border)',
                    borderRadius: '3px',
                    display: 'flex',
                    gap: '0.75rem',
                    padding: '0.5rem 0.75rem',
                  }}
                >
                  <span
                    style={{
                      color: 'var(--accent)',
                      fontFamily: 'monospace',
                      fontSize: '0.8rem',
                      minWidth: '5ch',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {truncatedTime}
                  </span>
                  <span
                    style={{
                      color: 'var(--text)',
                      fontSize: '0.85rem',
                      lineHeight: 1.4,
                    }}
                  >
                    {truncatedText}
                  </span>
                </div>
              );
            })}
          </div>
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
