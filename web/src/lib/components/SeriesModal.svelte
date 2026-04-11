<script lang="ts">
  // Series chooser popup used by keyboard shortcut `t` and click fallback.
  import type { Series } from '$lib/types';

  export let open = false;
  export let selectedSeries: Series = 'imsa';
  export let onPick: (series: Series) => void;

  const seriesList: Series[] = ['imsa', 'nls', 'f1'];
</script>

{#if open}
  <div class="backdrop">
    <section class="modal">
      <h2>Select Series</h2>
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
