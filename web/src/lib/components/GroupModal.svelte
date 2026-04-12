<script lang="ts">
  // Group chooser popup for direct jump into class-specific view.
  import { afterUpdate } from 'svelte';

  export let open = false;
  export let groups: string[] = [];
  export let selectedIndex = 0;
  export let onPick: (index: number) => void;
  export let onClose: () => void = () => {};

  let listEl: HTMLDivElement | null = null;

  afterUpdate(() => {
    if (!open || !listEl || groups.length === 0) {
      return;
    }

    const selected = listEl.querySelector('button.selected') as HTMLButtonElement | null;
    if (!selected) {
      return;
    }

    selected.scrollIntoView({ block: 'nearest' });
  });
</script>

{#if open}
  <div
    class="backdrop"
    role="button"
    tabindex="0"
    aria-label="Close group picker"
    on:click|self={() => onClose()}
    on:keydown={(event) => {
      if (event.key === 'Escape' || event.key === 'Enter' || event.key === ' ') {
        onClose();
        event.preventDefault();
      }
    }}
  >
    <section class="modal">
      <div class="title-row">
        <h2>Select Group</h2>
        <button class="close-btn" on:click={() => onClose()} type="button" aria-label="Close group picker">
          Close
        </button>
      </div>
      {#if groups.length === 0}
        <p class="empty">No groups available for current series.</p>
      {:else}
        <div class="list" bind:this={listEl}>
          {#each groups as group, idx (`${idx}-${group}`)}
            <button class:selected={idx === selectedIndex} on:click={() => onPick?.(idx)}>
              {idx === selectedIndex ? '>' : ' '} {group}
            </button>
          {/each}
        </div>
        <p class="hint">Use ↑/↓ to choose, Enter to switch, Esc to cancel.</p>
      {/if}
    </section>
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
    padding: 0.8rem;
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

  .title-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .close-btn {
    font: inherit;
    min-height: 2.2rem;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: #13263a;
    color: var(--text);
    padding: 0.35rem 0.62rem;
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
    padding: 0.62rem;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    min-height: 2.6rem;
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

  @media (max-width: 900px) {
    .modal {
      width: min(28rem, 96vw);
      max-height: 88dvh;
    }
  }
</style>
