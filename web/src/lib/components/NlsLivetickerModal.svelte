<script lang="ts">
  // NLS Liveticker modal - shows live updates from the NLS liveticker feed.
  import type { NlsLivetickerEntry } from '$lib/types';
  import { tick, afterUpdate } from 'svelte';

  export let open = false;
  export let entries: NlsLivetickerEntry[] = [];
  export let lastUpdateUnixMs: bigint | null = null;
  export let lastError: string | null = null;

  let scrollContainer: HTMLDivElement | null = null;
  let modalEl: HTMLDialogElement | null = null;
  let previouslyFocused: Element | null = null;
  let wasOpen = false;

  function formatAge(ms: number): string {
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ago`;
  }

  $: ageText = lastUpdateUnixMs
    ? formatAge(Number(Date.now()) - Number(lastUpdateUnixMs))
    : '-';

  function scrollUp() {
    if (scrollContainer) {
      scrollContainer.scrollTop = Math.max(0, scrollContainer.scrollTop - 1);
    }
  }

  function scrollDown() {
    if (scrollContainer) {
      scrollContainer.scrollTop += 1;
    }
  }

  function scrollPageUp() {
    if (scrollContainer) {
      scrollContainer.scrollTop = Math.max(0, scrollContainer.scrollTop - 10);
    }
  }

  function scrollPageDown() {
    if (scrollContainer) {
      scrollContainer.scrollTop += 10;
    }
  }

  function scrollHome() {
    if (scrollContainer) {
      scrollContainer.scrollTop = 0;
    }
  }

  function scrollEnd() {
    if (scrollContainer) {
      scrollContainer.scrollTop = scrollContainer.scrollHeight;
    }
  }

  export function handleKey(event: KeyboardEvent): boolean {
    if (!open) return false;
    switch (event.key) {
      case 'ArrowUp':
      case 'k':
        scrollUp();
        return true;
      case 'ArrowDown':
      case 'j':
        scrollDown();
        return true;
      case 'PageUp':
        scrollPageUp();
        return true;
      case 'PageDown':
        scrollPageDown();
        return true;
      case 'Home':
        scrollHome();
        return true;
      case 'End':
        scrollEnd();
        return true;
      case 'Escape':
        closeModal();
        return true;
    }
    return false;
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
      aria-labelledby="liveticker-title"
      on:click|stopPropagation
    >
      <h2 id="liveticker-title">NLS Liveticker</h2>
      <div class="meta">
        {entries.length} entries | updated {ageText}
        {#if lastError}
          <span class="error">| Error: {lastError}</span>
        {/if}
      </div>
      <div class="entries" bind:this={scrollContainer}>
        {#if entries.length === 0}
          <p class="empty">No liveticker entries yet.</p>
        {:else}
          {#each entries as entry (entry.id)}
            <div class="entry">
              <div class="time">{entry.day_label} {entry.time_text} Uhr</div>
              <div class="message">
                {#if entry.message}
                  {#each entry.message.split('\n') as line, lineIdx (lineIdx)}
                    <div>{line}</div>
                  {/each}
                {:else}
                  <span class="empty-msg">-</span>
                {/if}
              </div>
            </div>
          {/each}
        {/if}
      </div>
      <div class="footer">↑/↓ scroll | PgUp/PgDn fast scroll | Home/End jump | Esc or l close</div>
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

  .meta {
    color: var(--text-dim);
    font-size: 0.8rem;
    margin-bottom: 0.5rem;
  }

  .error {
    color: #ff8a8a;
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
    border-bottom: 1px solid var(--border);
    padding: 0.5rem 0;
  }

  .entry:last-child {
    border-bottom: none;
  }

  .time {
    color: #f5dd08;
    font-weight: bold;
    margin-bottom: 0.25rem;
  }

  .message {
    color: var(--text);
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .empty-msg {
    color: var(--text-dim);
  }

  .footer {
    color: var(--text-dim);
    font-size: 0.75rem;
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }
</style>
