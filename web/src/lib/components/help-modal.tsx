// Help-modal.tsx - Keyboard shortcuts help modal
import { useEffect, useCallback, type JSX } from 'react';

interface HelpModalProps {
  onClose: () => void;
  open: boolean;
}

interface HelpItem {
  desc: string;
  key: string;
}

const HELP_ITEMS: HelpItem[] = [
  { desc: 'Toggle this help', key: 'h, ?' },
  { desc: 'Move selection up/down', key: '↑↓, jk' },
  { desc: 'Page up/down', key: 'PgUp/PgDn' },
  { desc: 'First/last row', key: 'Home/End' },
  { desc: 'Toggle favourite', key: 'Space' },
  { desc: 'Jump to next favourite', key: 'f' },
  { desc: 'Search', key: 's' },
  { desc: 'Next/previous match', key: 'n/p' },
  { desc: 'Cycle view mode', key: 'g' },
  { desc: 'Group picker', key: 'G' },
  { desc: 'Overall view', key: 'o' },
  { desc: 'Switch series', key: 't' },
  { desc: 'Show messages', key: 'm' },
  { desc: 'NLS liveticker', key: 'l' },
  { desc: 'Toggle demo mode', key: 'd' },
  { desc: 'Close modal', key: 'Esc' },
];

const MODAL_MAX_WIDTH = 480;
const MODAL_PADDING_REM = 1.5;
const MODAL_BACKDROP_Z_INDEX = 100;
const MODAL_CONTENT_Z_INDEX = 101;

export const HelpModal = function HelpModal(props: HelpModalProps): JSX.Element | null {
  const { onClose, open } = props;

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

  return (
    <div
      className="help-modal-backdrop"
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
        className="help-modal-content"
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
            Keyboard Shortcuts
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

        <div
          style={{
            display: 'grid',
            gap: '0.5rem',
            gridTemplateColumns: 'auto 1fr',
          }}
        >
          {HELP_ITEMS.map((item) => (
            <div
              key={item.key}
              style={{
                alignItems: 'center',
                display: 'contents',
              }}
            >
              <kbd
                style={{
                  backgroundColor: 'var(--bg-control)',
                  border: '1px solid var(--border)',
                  borderRadius: '3px',
                  color: 'var(--accent)',
                  fontFamily: 'monospace',
                  fontSize: '0.75rem',
                  padding: '0.125rem 0.375rem',
                  whiteSpace: 'nowrap',
                }}
              >
                {item.key}
              </kbd>
              <span
                style={{
                  color: 'var(--text-dim)',
                  fontSize: '0.85rem',
                  paddingLeft: '0.75rem',
                }}
              >
                {item.desc}
              </span>
            </div>
          ))}
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
          Press Esc to close
        </div>
      </div>
    </div>
  );
};
