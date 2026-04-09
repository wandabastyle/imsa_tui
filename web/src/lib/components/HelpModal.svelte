<script lang="ts">
  export let open = false;
</script>

{#if open}
  <div class="backdrop">
    <section class="modal">
      <h2>Keyboard Help</h2>
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
Esc close popup / quit</pre>
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
    width: min(34rem, 90vw);
    padding: 0.75rem;
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
</style>
