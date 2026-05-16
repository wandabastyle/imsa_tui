<script lang="ts">
  // Race messages/notices modal - shows timing notices from the snapshot.
  import type { TimingNotice } from '$lib/types';
  import { tick, afterUpdate } from 'svelte';

  export let open = false;
  export let notices: TimingNotice[] = [];

  let selectedIdx = 0;
  let scrollContainer: HTMLDivElement | null = null;
  let modalEl: HTMLDialogElement | null = null;
  let previouslyFocused: Element | null = null;
  let wasOpen = false;

  function scrollToSelected() {
    if (scrollContainer && notices.length > 0) {
      const selectedElement = scrollContainer.children[selectedIdx] as HTMLElement;
      if (selectedElement) {
        selectedElement.scrollIntoView({ block: 'nearest' });
      }
    }
  }

  export function moveSelection(delta: number) {
    if (notices.length === 0) return;
    selectedIdx = (selectedIdx + delta + notices.length) % notices.length;
    scrollToSelected();
  }

  export function handleKey(event: KeyboardEvent): boolean {
    if (!open) return false;
    switch (event.key) {
      case 'ArrowUp':
      case 'k':
        moveSelection(-1);
        return true;
      case 'ArrowDown':
      case 'j':
        moveSelection(1);
        return true;
      case 'Home':
        selectedIdx = 0;
        scrollToSelected();
        return true;
      case 'End':
        selectedIdx = notices.length > 0 ? notices.length - 1 : 0;
        scrollToSelected();
        return true;
      case 'Escape':
        closeModal();
        return true;
    }
    return false;
  }

  export function getSelectedNotice(): TimingNotice | null {
    return notices[selectedIdx] ?? null;
  }

  export function resetSelection() {
    selectedIdx = 0;
  }

  afterUpdate(() => {
    if (!open) {
      wasOpen = false;
      return;
    }

    if (!wasOpen) {
      wasOpen = true;
      // Store the trigger element when opening
      previouslyFocused = document.activeElement;
      // Focus the first item
      const firstItem = modalEl?.querySelector('.entry') as HTMLElement | null;
      if (firstItem) {
        firstItem.focus();
      }
    }
  });

  function closeModal() {
    open = false;
    tick().then(() => {
      if (previouslyFocused && 'focus' in previouslyFocused) {
        (previouslyFocused as HTMLElement).focus();
      }
    });
  }

  function onBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      closeModal();
    }
  }
</script>

{#if open}
  <div
    class="backdrop"
    role="presentation"
    on:click={onBackdropClick}
  >
    <dialog
      bind:this={modalEl}
      class="modal"
      aria-labelledby="messages-title"
      on:click|stopPropagation
    >
      <h2 id="messages-title">Race Messages</h2>
      <div class="entries" bind:this={scrollContainer}>
        {#if notices.length === 0}
          <p class="empty">No active race messages.</p>
        {:else}
          {#each notices as notice, idx (notice.id)}
            <div
              class="entry"
              class:selected={idx === selectedIdx}
              role="button"
              tabindex="0"
            >
              <span class="marker">{idx === selectedIdx ? '>' : ' '}</span>
              <span class="time">{notice.time.trim() || '--:--:--'}</span>
              <span class="text">{notice.text.trim()}</span>
            </div>
          {/each}
        {/if}
      </div>
      <div class="footer">↑/↓ select | Esc or m close</div>
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
    width: min(50rem, 90vw);
    max-height: 80vh;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
  }

  h2 {
    margin: 0 0 0.45rem 0;
    font-size: 1rem;
  }

  .entries {
    flex: 1;
    overflow-y: auto;
    max-height: 60vh;
  }

  .empty {
    color: var(--text-dim);
    text-align: center;
    padding: 1rem;
  }

  .entry {
    display: flex;
    gap: 0.5rem;
    padding: 0.25rem 0;
    font-family: monospace;
  }

  .entry.selected {
    color: #f5dd08;
    font-weight: bold;
  }

  .marker {
    flex-shrink: 0;
    width: 1rem;
  }

  .time {
    flex-shrink: 0;
    width: 8ch;
    color: var(--text-dim);
  }

  .text {
    flex: 1;
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .footer {
    color: var(--text-dim);
    font-size: 0.75rem;
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }
</style>
