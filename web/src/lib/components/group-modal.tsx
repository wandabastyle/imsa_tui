// Group-modal.tsx - Group chooser popup for direct jump into class-specific view
import { useEffect, useRef, useCallback, type JSX } from 'react';

const EMPTY_LENGTH = 0;
const FOCUS_DELAY_MS = 0;
const INDEX_PREV_OFFSET = 1;
const INDEX_NEXT_OFFSET = 1;
const ZERO_INDEX = 0;

interface GroupModalProps {
  open: boolean;
  groups: string[];
  selectedIndex: number;
  onPick: (index: number) => void;
  onClose: () => void;
}

export const GroupModal = function GroupModal(props: GroupModalProps): JSX.Element | null {
  const { open, groups, selectedIndex, onPick, onClose } = props;
  const listRef = useRef<HTMLDivElement>(null);
  const previouslyFocusedRef = useRef<Element | null>(null);
  const wasOpenRef = useRef(false);

  const restoreFocus = useCallback((): void => {
    const prev = previouslyFocusedRef.current;
    if (prev && prev instanceof HTMLElement) {
      prev.focus();
    }
  }, []);

  const handlePick = useCallback(
    (index: number): void => {
      onPick(index);
      // Restore focus asynchronously (like Svelte's tick)
      setTimeout(() => {
        restoreFocus();
      }, FOCUS_DELAY_MS);
    },
    [onPick, restoreFocus],
  );

  const handleClose = useCallback((): void => {
    onClose();
    // Restore focus asynchronously (like Svelte's tick)
    setTimeout(() => {
      restoreFocus();
    }, FOCUS_DELAY_MS);
  }, [onClose, restoreFocus]);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent): void => {
      if (!open) {
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        handleClose();
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        // Navigate up
        const newIndex =
          selectedIndex > ZERO_INDEX
            ? selectedIndex - INDEX_PREV_OFFSET
            : groups.length - INDEX_PREV_OFFSET;
        // This would need to be handled by the parent since selectedIndex is read-only
        // But we can focus the button directly
        const buttons = listRef.current?.querySelectorAll('button');
        const buttonElement = buttons?.item(newIndex);
        if (buttonElement !== undefined) {
          buttonElement.focus();
        }
      } else if (event.key === 'ArrowDown') {
        event.preventDefault();
        // Navigate down
        const newIndex =
          selectedIndex < groups.length - INDEX_NEXT_OFFSET
            ? selectedIndex + INDEX_NEXT_OFFSET
            : ZERO_INDEX;
        const buttons = listRef.current?.querySelectorAll('button');
        const buttonElement = buttons?.item(newIndex);
        if (buttonElement !== undefined) {
          buttonElement.focus();
        }
      } else if (event.key === 'Enter') {
        event.preventDefault();
        handlePick(selectedIndex);
      }
    },
    [open, handleClose, handlePick, selectedIndex, groups.length],
  );

  useEffect(() => {
    if (!open) {
      wasOpenRef.current = false;
      return;
    }

    // Store the trigger element when opening
    if (!wasOpenRef.current) {
      wasOpenRef.current = true;
      previouslyFocusedRef.current = document.activeElement;

      // Focus the selected button
      const selected = listRef.current?.querySelector('button.selected');
      if (selected instanceof HTMLButtonElement) {
        selected.focus();
        selected.scrollIntoView({ block: 'nearest' });
      }
    } else if (listRef.current) {
      // Scroll selected into view when selection changes
      const selected = listRef.current.querySelector('button.selected');
      if (selected instanceof HTMLButtonElement) {
        selected.scrollIntoView({ block: 'nearest' });
      }
    }
  }, [open, selectedIndex]);

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
    <div className="backdrop" onClick={handleClose} role="presentation">
      <dialog
        className="modal group-modal"
        aria-labelledby="group-title"
        onClick={(event): void => {
          event.stopPropagation();
        }}
      >
        <h2 id="group-title">Select Group</h2>

        {groups.length === EMPTY_LENGTH ? (
          <p className="empty">No groups available for current series.</p>
        ) : (
          <>
            <div className="list" ref={listRef}>
              {groups.map((group, idx) => (
                <button
                  key={`${idx}-${group}`}
                  className={idx === selectedIndex ? 'selected' : ''}
                  onClick={(): void => {
                    handlePick(idx);
                  }}
                  type="button"
                >
                  {idx === selectedIndex ? '>' : ' '} {group}
                </button>
              ))}
            </div>
            <p className="hint">Use ↑/↓ to choose, Enter to switch, Esc to cancel.</p>
          </>
        )}
      </dialog>
    </div>
  );
};
