// Help-modal.tsx - Keyboard shortcuts help modal
import { useEffect, useRef, useCallback, type JSX } from 'react';

interface HelpModalProps {
  onClose: () => void;
  open: boolean;
}

const FOCUS_RESTORE_DELAY_MS = 0;

export const HelpModal = function HelpModal(props: HelpModalProps): JSX.Element | null {
  const { onClose, open } = props;
  const modalRef = useRef<HTMLDialogElement>(null);
  const previouslyFocusedRef = useRef<Element | null>(null);
  const wasOpenRef = useRef(false);

  const closeModal = useCallback(() => {
    onClose();
    // Restore focus after state update
    setTimeout(() => {
      const prev = previouslyFocusedRef.current;
      if (prev instanceof HTMLElement) {
        prev.focus();
      }
    }, FOCUS_RESTORE_DELAY_MS);
  }, [onClose]);

  useEffect(() => {
    if (!open) {
      wasOpenRef.current = false;
      return;
    }

    if (!wasOpenRef.current) {
      wasOpenRef.current = true;
      // Store the trigger element when opening
      previouslyFocusedRef.current = document.activeElement;
      // Focus the close hint button if it exists
      const closeBtn = modalRef.current?.querySelector('[data-close-hint]');
      if (closeBtn instanceof HTMLElement) {
        closeBtn.focus();
      }
    }
  }, [open]);

  useEffect(() => {
    const handleKeydown = (event: KeyboardEvent): void => {
      if (!open) {
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        closeModal();
      }
    };

    document.addEventListener('keydown', handleKeydown);
    return (): void => {
      document.removeEventListener('keydown', handleKeydown);
    };
  }, [open, closeModal]);

  if (!open) {
    return null;
  }

  return (
    <div className="backdrop" role="presentation" onClick={closeModal}>
      <dialog
        ref={modalRef}
        className="modal help-modal"
        aria-labelledby="help-title"
        onClick={(event: React.MouseEvent<HTMLDialogElement>): void => {
          event.stopPropagation();
        }}
      >
        <h2 id="help-title">Keyboard Help</h2>
        <pre>{`h toggle help (? also works)
g cycle views
G open group picker
o overall view
t series picker
arrows/j/k move
PgUp/PgDn fast scroll
space toggle favourite
f jump favourite
s search mode (type, Enter apply, Esc cancel)
n/p next/prev match
d toggle demo/live data source
Esc close popup`}</pre>
        <button className="close-hint" data-close-hint onClick={closeModal}>
          Close (Esc)
        </button>
      </dialog>
    </div>
  );
};
