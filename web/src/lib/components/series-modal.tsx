// Series-modal.tsx - Series picker modal
import { useEffect, useRef, useCallback, type JSX } from 'react';

import type { Series } from '../generated/web-shared';
import { ALL_SERIES } from '../types';

interface SeriesModalProps {
  open: boolean;
  selectedSeries: Series;
  onPick: (series: Series) => void;
  onClose: () => void;
}

const FOCUS_DELAY_MS = 0;

const handleDialogClick = (event: React.MouseEvent<HTMLDialogElement>): void => {
  event.stopPropagation();
};

export const SeriesModal = function SeriesModal(props: SeriesModalProps): JSX.Element | null {
  const { open, selectedSeries, onPick, onClose } = props;
  const modalElRef = useRef<HTMLDialogElement | null>(null);
  const previouslyFocusedRef = useRef<Element | null>(null);
  const wasOpenRef = useRef(false);

  const restoreFocus = useCallback((): void => {
    const previouslyFocused = previouslyFocusedRef.current;
    if (previouslyFocused instanceof HTMLElement) {
      previouslyFocused.focus();
    }
  }, []);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      if (!open) {
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
        // Restore focus after closing
        setTimeout(restoreFocus, FOCUS_DELAY_MS);
      }
    },
    [open, onClose, restoreFocus],
  );

  useEffect(() => {
    if (!open) {
      wasOpenRef.current = false;
      return function noop(): void {
        // Intentionally empty
      };
    }
    document.addEventListener('keydown', handleKeyDown);
    return (): void => {
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [open, handleKeyDown]);

  useEffect(() => {
    if (open && !wasOpenRef.current) {
      wasOpenRef.current = true;
      // Store the trigger element when opening
      previouslyFocusedRef.current = document.activeElement;
      // Focus the selected button when modal opens
      const selectedEl = modalElRef.current?.querySelector('button.selected');
      if (selectedEl instanceof HTMLButtonElement) {
        selectedEl.focus();
      }
    }
  }, [open]);

  const handleBackdropClick = (): void => {
    onClose();
    // Restore focus after closing
    setTimeout(restoreFocus, FOCUS_DELAY_MS);
  };

  const handlePick = (series: Series): void => {
    onPick(series);
    // Restore focus after picking
    setTimeout(restoreFocus, FOCUS_DELAY_MS);
  };

  if (!open) {
    return null;
  }

  return (
    <div className="backdrop" role="presentation" onClick={handleBackdropClick} tabIndex={-1}>
      <dialog
        ref={modalElRef}
        className="modal series-modal"
        aria-labelledby="series-title"
        onClick={handleDialogClick}
      >
        <h2 id="series-title">Select Series</h2>
        <div className="list">
          {ALL_SERIES.map((series) => (
            <button
              key={series}
              type="button"
              className={series === selectedSeries ? 'selected' : ''}
              onClick={(): void => {
                handlePick(series);
              }}
            >
              {series.toUpperCase()}
            </button>
          ))}
        </div>
      </dialog>
    </div>
  );
};
