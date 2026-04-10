// Legacy keybinding utility kept for parity experiments and simple pages.

export interface KeyActions {
  onQuitLikeEscape: () => void;
  onToggleHelp: () => void;
  onCycleView: () => void;
  onOverallView: () => void;
  onToggleSeriesPicker: () => void;
  onMoveDown: () => void;
  onMoveUp: () => void;
  onPageDown: () => void;
  onPageUp: () => void;
  onHome: () => void;
  onEnd: () => void;
  onToggleFavourite: () => void;
  onJumpFavourite: () => void;
  onStartSearch: () => void;
  onNextSearch: () => void;
  onPrevSearch: () => void;
  onDemoFlagNext: () => void;
  onDemoFlagLive: () => void;
}

export function installKeyBindings(actions: KeyActions): () => void {
  const handler = (event: KeyboardEvent) => {
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) {
      return;
    }

    switch (event.key) {
      case 'Escape':
        actions.onQuitLikeEscape();
        break;
      case 'h':
        actions.onToggleHelp();
        break;
      case 'g':
        actions.onCycleView();
        break;
      case 'o':
        actions.onOverallView();
        break;
      case 't':
        actions.onToggleSeriesPicker();
        break;
      case 'ArrowDown':
      case 'j':
        actions.onMoveDown();
        break;
      case 'ArrowUp':
      case 'k':
        actions.onMoveUp();
        break;
      case 'PageDown':
        actions.onPageDown();
        break;
      case 'PageUp':
        actions.onPageUp();
        break;
      case 'Home':
        actions.onHome();
        break;
      case 'End':
        actions.onEnd();
        break;
      case ' ':
        actions.onToggleFavourite();
        break;
      case 'f':
        actions.onJumpFavourite();
        break;
      case 's':
        actions.onStartSearch();
        break;
      case 'n':
        actions.onNextSearch();
        break;
      case 'p':
        actions.onPrevSearch();
        break;
      case 'r':
        actions.onDemoFlagNext();
        break;
      case '0':
        actions.onDemoFlagLive();
        break;
      default:
        return;
    }

    event.preventDefault();
  };

  window.addEventListener('keydown', handler);
  return () => window.removeEventListener('keydown', handler);
}
