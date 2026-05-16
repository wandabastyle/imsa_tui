<script lang="ts">
  // Keyboard reference popup; key map mirrors TUI/Web shared behavior.
  import { tick, afterUpdate } from 'svelte';

  export let open = false;

  let modalEl: HTMLElement | null = null;
  let triggerEl: HTMLElement | null = null;
  let previouslyFocused: Element | null = null;
  let wasOpen = false;

  afterUpdate(() => {
    if (!open) {
      wasOpen = false;
      return;
    }

    if (!wasOpen) {
      wasOpen = true;
      // Store the trigger element when opening
      previouslyFocused = document.activeElement;
      // Focus the close hint button if it exists
      const closeBtn = modalEl?.querySelector('[data-close-hint]');
      if (closeBtn) {
        (closeBtn as HTMLElement).focus();
      }
    }
  });

  function handleKeydown(event: KeyboardEvent) {
    if (!open) return;

    if (event.key === 'Escape') {
      open = false;
      event.preventDefault();
      // Restore focus
      tick().then(() => {
        if (previouslyFocused && 'focus' in previouslyFocused) {
          (previouslyFocused as HTMLElement).focus();
        }
      });
    }
  }

  function closeModal() {
    open = false;
    tick().then(() => {
      if (previouslyFocused && 'focus' in previouslyFocused) {
        (previouslyFocused as HTMLElement).focus();
      }
    });
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if open}
  <div
    class="backdrop"
    role="presentation"
    on:click={closeModal}
  >
    <dialog
      bind:this={modalEl}
      class="modal"
      aria-labelledby="help-title"
      on:click|stopPropagation
    >
      <h2 id="help-title">Keyboard Help</h2>
      <pre>h toggle help (? also works)
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
Esc close popup</pre>
      <button data-close-hint class="close-hint" on:click={closeModal}>Close (Esc)</button>
    </dialog>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 45%);
    display: grid;
    place-items: center;
    z-index: 220;
  }

  .modal {
    background: #0d1b2c;
    border: 1px solid var(--border);
    border-radius: 8px;
    width: min(34rem, 90vw);
    padding: 0.75rem;
  }

  h2 {
    margin: 0 0 0.45rem 0;
    font-size: 1rem;
  }

  pre {
    margin: 0 0 0.75rem 0;
    color: var(--text-dim);
    line-height: 1.5;
  }

  .close-hint {
    background: #13263a;
    border: 1px solid var(--border);
    color: var(--text);
    padding: 0.35rem 0.75rem;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .close-hint:hover,
  .close-hint:focus {
    border-color: var(--accent);
    background: #1b3c62;
  }
</style>
