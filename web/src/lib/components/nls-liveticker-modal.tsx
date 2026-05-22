// Nls-liveticker-modal.tsx - NLS liveticker modal
import { useEffect, useRef, useCallback, type JSX } from 'react';

import type { NlsLivetickerEntry } from '../generated/web-shared';

interface NlsLivetickerModalProps {
  entries: NlsLivetickerEntry[];
  lastError: string | null;
  lastUpdateUnixMs: bigint | null;
  onClose: () => void;
  open: boolean;
}

const MS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const MINUTES_PER_HOUR = 60;
const ZERO = 0;
const SCROLL_UNIT = 1;
const SCROLL_PAGE = 10;

const formatAge = function formatAge(ms: number): string {
  const seconds = Math.floor(ms / MS_PER_SECOND);
  if (seconds < SECONDS_PER_MINUTE) {
    return `${seconds}s ago`;
  }
  const minutes = Math.floor(seconds / SECONDS_PER_MINUTE);
  if (minutes < MINUTES_PER_HOUR) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / MINUTES_PER_HOUR);
  return `${hours}h ago`;
};

const handleModalClick = (event: React.MouseEvent<HTMLDialogElement>): void => {
  event.stopPropagation();
};

export const NlsLivetickerModal = function NlsLivetickerModal(
  props: NlsLivetickerModalProps,
): JSX.Element | null {
  const { entries, lastError, lastUpdateUnixMs, onClose, open } = props;
  const scrollContainerRef = useRef<HTMLDivElement>(null);

  const scrollUp = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop = Math.max(
        ZERO,
        scrollContainerRef.current.scrollTop - SCROLL_UNIT,
      );
    }
  }, []);

  const scrollDown = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop += SCROLL_UNIT;
    }
  }, []);

  const scrollPageUp = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop = Math.max(
        ZERO,
        scrollContainerRef.current.scrollTop - SCROLL_PAGE,
      );
    }
  }, []);

  const scrollPageDown = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop += SCROLL_PAGE;
    }
  }, []);

  const scrollHome = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop = ZERO;
    }
  }, []);

  const scrollEnd = useCallback((): void => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop = scrollContainerRef.current.scrollHeight;
    }
  }, []);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      switch (event.key) {
        case 'ArrowUp':
        case 'k': {
          scrollUp();
          break;
        }
        case 'ArrowDown':
        case 'j': {
          scrollDown();
          break;
        }
        case 'PageUp': {
          scrollPageUp();
          break;
        }
        case 'PageDown': {
          scrollPageDown();
          break;
        }
        case 'Home': {
          scrollHome();
          break;
        }
        case 'End': {
          scrollEnd();
          break;
        }
        case 'Escape': {
          onClose();
          break;
        }
        default: {
          // No action for other keys
          break;
        }
      }
    },
    [onClose, scrollUp, scrollDown, scrollPageUp, scrollPageDown, scrollHome, scrollEnd],
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

  const now = Date.now();
  const ageText = lastUpdateUnixMs === null ? '-' : formatAge(now - Number(lastUpdateUnixMs));

  const handleBackdropClick = (event: React.MouseEvent<HTMLDivElement>): void => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  return (
    <div className="backdrop" role="presentation" onClick={handleBackdropClick}>
      <dialog
        className="modal liveticker-modal"
        aria-labelledby="liveticker-title"
        onClick={handleModalClick}
      >
        <h2 id="liveticker-title">NLS Liveticker</h2>
        <div className="meta">
          {entries.length} entries | updated {ageText}
          {lastError !== null && lastError !== '' ? (
            <span className="error"> | Error: {lastError}</span>
          ) : null}
        </div>
        <div className="entries" ref={scrollContainerRef}>
          {entries.length === ZERO ? (
            <p className="empty">No liveticker entries yet.</p>
          ) : (
            entries.map((entry) => (
              <div className="entry" key={entry.id}>
                <div className="time">
                  {entry.day_label} {entry.time_text} Uhr
                </div>
                <div className="message">
                  {entry.message === '' ? (
                    <span className="empty-msg">-</span>
                  ) : (
                    entry.message
                      .split('\n')
                      .map((line, lineIdx) => <div key={lineIdx}>{line}</div>)
                  )}
                </div>
              </div>
            ))
          )}
        </div>
        <div className="footer">
          ↑/↓ scroll | PgUp/PgDn fast scroll | Home/End jump | Esc or l close
        </div>
      </dialog>
    </div>
  );
};
