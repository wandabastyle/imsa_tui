<script lang="ts">
  // Group chooser popup for direct jump into class-specific view.
  import { afterUpdate, tick } from 'svelte';

  export let open = false;
  export let groups: string[] = [];
  export let selectedIndex = 0;
  export let onPick: (index: number) => void = () => {};

  let listEl: HTMLDivElement | null = null;
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
      // Focus the first selected button
      const selected = listEl?.querySelector('button.selected') as HTMLButtonElement | null;
      if (selected) {
        selected.focus();
      }
    }

    if (listEl && groups.length > 0) {
      const selected = listEl.querySelector('button.selected') as HTMLButtonElement | null;
      if (selected) {
        selected.scrollIntoView({ block: 'nearest' });
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

  function handlePick(index: number) {
    onPick?.(index);
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
  >
    <dialog
      bind:this={modalEl}
      class="modal"
      aria-labelledby="group-title"
      on:click|stopPropagation
    >
      <h2 id="group-title">Select Group</h2>
      {#if groups.length === 0}
        <p class="empty">No groups available for current series.</p>
      {:else}
        <div class="list" bind:this={listEl}>
          {#each groups as group, idx (`${idx}-${group}`)}
            <button
              type="button"
              class:selected={idx === selectedIndex}
              on:click={() => handlePick(idx)}
            >
              {idx === selectedIndex ? '>' : ' '} {group}
            </button>
          {/each}
        </div>
        <p class="hint">Use ↑/↓ to choose, Enter to switch, Esc to cancel.</p>
      {/if}
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
    width: min(28rem, 90vw);
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
    gap: 0.45rem;
    max-height: 56vh;
    overflow: auto;
    padding-right: 0.15rem;
  }

  button {
    background: #13263a;
    border: 1px solid #334965;
    color: var(--text);
    padding: 0.42rem;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
  }

  button.selected {
    border-color: var(--accent);
    background: #1b3c62;
    font-weight: 700;
  }

  .hint,
  .empty {
    margin: 0.6rem 0 0 0;
    color: var(--text-dim);
    font-size: 0.84rem;
  }
</style>
