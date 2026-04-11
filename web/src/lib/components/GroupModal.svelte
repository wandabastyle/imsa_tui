<script lang="ts">
  // Group chooser popup for direct jump into class-specific view.
  import { afterUpdate } from 'svelte';

  export let open = false;
  export let groups: string[] = [];
  export let selectedIndex = 0;
  export let onPick: (index: number) => void;

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
  <div class="backdrop">
    <section class="modal">
      <h2>Select Group</h2>
      {#if groups.length === 0}
        <p class="empty">No groups available for current series.</p>
      {:else}
        <div class="list" bind:this={listEl}>
          {#each groups as group, idx}
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
