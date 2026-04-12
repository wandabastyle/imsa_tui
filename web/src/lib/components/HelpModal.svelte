<script lang="ts">
  // Keyboard reference popup; key map mirrors TUI/Web shared behavior.
  export let open = false;
  export let onClose: () => void = () => {};
</script>

{#if open}
  <div
    class="backdrop"
    role="button"
    tabindex="0"
    aria-label="Close help"
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
        <h2>Keyboard Help</h2>
        <button class="close-btn" on:click={() => onClose()} type="button" aria-label="Close help">Close</button>
      </div>
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
r demo flag
0 live flag
Esc close popup</pre>
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
    width: min(34rem, 90vw);
    padding: 0.75rem;
  }

  .title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
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
    margin: 0 0 0.45rem 0;
    font-size: 1rem;
  }

  pre {
    margin: 0;
    color: var(--text-dim);
    line-height: 1.5;
  }

  @media (max-width: 900px) {
    .modal {
      width: min(34rem, 96vw);
      max-height: 88dvh;
      overflow: auto;
    }
  }
</style>
