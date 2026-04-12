<script lang="ts">
  // Series chooser popup used by keyboard shortcut `t` and click fallback.
  import type { Series } from '$lib/types';

  export let open = false;
  export let selectedSeries: Series = 'imsa';
  export let onPick: (series: Series) => void;
  export let onClose: () => void = () => {};

  const seriesList: Series[] = ['imsa', 'nls', 'f1'];
</script>

{#if open}
  <div
    class="backdrop"
    role="button"
    tabindex="0"
    aria-label="Close series picker"
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
        <h2>Select Series</h2>
        <button class="close-btn" on:click={() => onClose()} type="button" aria-label="Close series picker">
          Close
        </button>
      </div>
      <div class="list">
        {#each seriesList as series (series)}
          <button class:selected={series === selectedSeries} on:click={() => onPick?.(series)}>
            {series.toUpperCase()}
          </button>
        {/each}
      </div>
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
    width: min(22rem, 90vw);
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
    gap: 0.5rem;
    overflow: auto;
  }

  button {
    background: #13263a;
    border: 1px solid #334965;
    color: var(--text);
    padding: 0.62rem;
    border-radius: 8px;
    cursor: pointer;
    min-height: 2.6rem;
  }

  button.selected {
    border-color: var(--accent);
    background: #1b3c62;
    font-weight: 700;
  }

  @media (max-width: 900px) {
    .modal {
      width: min(22rem, 96vw);
      max-height: 88dvh;
    }
  }
</style>
