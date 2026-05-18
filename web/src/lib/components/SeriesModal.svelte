<script lang="ts">
  // Series chooser popup used by keyboard shortcut `t` and click fallback.
  import type { Series } from '$lib/types';
  import { tick, afterUpdate } from 'svelte';

  export let open = false;
  export let selectedSeries: Series = 'imsa';
  export let onPick: (series: Series) => void = () => {};

  const seriesList: Series[] = ['imsa', 'nls', 'f1', 'wec', 'dhlm'];
  let modalEl: HTMLElement | null = null;
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
      // Focus the selected button
      const selected = modalEl?.querySelector('button.selected') as HTMLButtonElement | null;
      if (selected) {
        selected.focus();
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

  function handlePick(series: Series) {
    onPick?.(series);
    tick().then(() => {
      if (previouslyFocused && 'focus' in previouslyFocused) {
        (previouslyFocused as HTMLElement).focus();
      }
    });
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
    on:keydown={(e) => e.key === 'Enter' && closeModal()}
    tabindex="-1"
  >
    <dialog
      bind:this={modalEl}
      class="modal"
      aria-labelledby="series-title"
      on:click|stopPropagation
    >
      <h2 id="series-title">Select Series</h2>
      <div class="list">
        {#each seriesList as series (series)}
          <button
            type="button"
            class:selected={series === selectedSeries}
            on:click={() => handlePick(series)}
          >
            {series.toUpperCase()}
          </button>
        {/each}
      </div>
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
    width: min(22rem, 90vw);
    max-height: 72vh;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
  }

  h2 {
    margin: 0 0 0.55rem 0;
    font-size: 1rem;
  }

  .list {
    display: grid;
    gap: 0.5rem;
    overflow: auto;
  }

  button {
    background: #13263a;
    border: 1px solid #334965;
    color: var(--text);
    padding: 0.5rem;
    border-radius: 8px;
    cursor: pointer;
  }

  button.selected {
    border-color: var(--accent);
    background: #1b3c62;
    font-weight: 700;
  }
</style>
