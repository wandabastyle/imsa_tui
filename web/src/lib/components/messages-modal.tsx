// Messages-modal.tsx - Race messages modal
import { useEffect, useRef, useCallback, useState, type JSX } from 'react';

import type { TimingNotice } from '../generated/web-shared';

interface MessagesModalProps {
  notices: TimingNotice[];
  onClose: () => void;
  open: boolean;
}

const ZERO = 0;
const ONE = 1;
const NEGATIVE_ONE = -1;
const INITIAL_INDEX = 0;
const TIMEOUT_DELAY = 0;
const NO_NOTICES = 0;

export const MessagesModal = function MessagesModal(props: MessagesModalProps): JSX.Element | null {
  const { notices, onClose, open } = props;
  const [selectedIdx, setSelectedIdx] = useState(INITIAL_INDEX);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const modalElRef = useRef<HTMLDialogElement>(null);
  const previouslyFocusedRef = useRef<Element | null>(null);
  const wasOpenRef = useRef(false);

  const scrollToSelected = useCallback((): void => {
    if (scrollContainerRef.current && notices.length > NO_NOTICES) {
      const element = scrollContainerRef.current.children[selectedIdx];
      if (element instanceof HTMLElement) {
        element.scrollIntoView({ block: 'nearest' });
      }
    }
  }, [selectedIdx, notices.length]);

  const moveSelection = useCallback(
    (delta: number): void => {
      if (notices.length === NO_NOTICES) {
        return;
      }
      setSelectedIdx((prev) => {
        const newIdx = (prev + delta + notices.length) % notices.length;
        return newIdx;
      });
    },
    [notices.length],
  );

  const closeModal = useCallback((): void => {
    onClose();
    // Restore focus after modal closes
    setTimeout((): void => {
      if (previouslyFocusedRef.current && 'focus' in previouslyFocusedRef.current) {
        const prevElement = previouslyFocusedRef.current;
        if (prevElement instanceof HTMLElement) {
          prevElement.focus();
        }
      }
    }, TIMEOUT_DELAY);
  }, [onClose]);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      if (!open) {
        return;
      }

      switch (event.key) {
        case 'ArrowUp':
        case 'k': {
          event.preventDefault();
          moveSelection(NEGATIVE_ONE);
          break;
        }
        case 'ArrowDown':
        case 'j': {
          event.preventDefault();
          moveSelection(ONE);
          break;
        }
        case 'Home': {
          event.preventDefault();
          setSelectedIdx(ZERO);
          break;
        }
        case 'End': {
          event.preventDefault();
          setSelectedIdx(notices.length > NO_NOTICES ? notices.length - ONE : ZERO);
          break;
        }
        case 'Escape': {
          closeModal();
          break;
        }
        default: {
          // No action for other keys
          break;
        }
      }
    },
    [open, moveSelection, notices.length, closeModal],
  );

  useEffect((): (() => void) => {
    document.addEventListener('keydown', handleKeyDown);
    return (): void => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [handleKeyDown]);

  useEffect((): void => {
    if (!open) {
      wasOpenRef.current = false;
      return;
    }

    if (!wasOpenRef.current) {
      wasOpenRef.current = true;
      // Store the trigger element when opening
      previouslyFocusedRef.current = document.activeElement;
      // Focus the first item
      setTimeout((): void => {
        const firstItem = modalElRef.current?.querySelector('.entry');
        if (firstItem instanceof HTMLElement) {
          firstItem.focus();
        }
      }, TIMEOUT_DELAY);
    }
  }, [open]);

  // Scroll to selected when selectedIdx changes
  useEffect((): void => {
    if (open) {
      scrollToSelected();
    }
  }, [selectedIdx, open, scrollToSelected]);

  const onBackdropClick = useCallback(
    (event: React.MouseEvent<HTMLDivElement>): void => {
      if (event.target === event.currentTarget) {
        closeModal();
      }
    },
    [closeModal],
  );

  const stopPropagation = useCallback((event: React.MouseEvent): void => {
    event.stopPropagation();
  }, []);

  if (!open) {
    return null;
  }

  return (
    <div className="backdrop" role="presentation" onClick={onBackdropClick}>
      <dialog
        ref={modalElRef}
        className="modal"
        aria-labelledby="messages-title"
        onClick={stopPropagation}
      >
        <h2 id="messages-title">Race Messages</h2>
        <div className="entries" ref={scrollContainerRef}>
          {notices.length === NO_NOTICES ? (
            <p className="empty">No active race messages.</p>
          ) : (
            notices.map((notice, idx) => (
              <div
                key={notice.id}
                className={`entry ${idx === selectedIdx ? 'selected' : ''}`}
                role="button"
                tabIndex={ZERO}
              >
                <span className="marker">{idx === selectedIdx ? '>' : ' '}</span>
                <span className="time">{notice.time.trim() || '--:--:--'}</span>
                <span className="text">{notice.text.trim()}</span>
              </div>
            ))
          )}
        </div>
        <div className="footer">↑/↓ select | Esc or m close</div>
      </dialog>
    </div>
  );
};
