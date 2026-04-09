<script lang="ts">
  import type { SeriesSnapshot } from '$lib/types';

  export let snapshot: SeriesSnapshot | null;
  export let viewModeLabel = 'Overall';
  export let favCount = 0;
  export let searchLabel = '';
  export let demoLabel = '';
  export let errorText = '';

  $: ageText = snapshot?.last_update_unix_ms
    ? `Upd ${Math.max(0, Math.floor((Date.now() - snapshot.last_update_unix_ms) / 1000))}s`
    : 'Upd -';
</script>

<section class="header" data-flag={(snapshot?.header.flag || '-').toLowerCase()}>
  <div class="line">
    {snapshot?.status || 'Starting live timing...'} | {snapshot?.header.event_name || '-'} | {snapshot?.header.session_name || '-'} | {snapshot?.header.track_name || '-'} | TTE {snapshot?.header.time_to_go || '-'} | Mode {viewModeLabel} | <strong>{snapshot?.header.flag || '-'}</strong> | Day {snapshot?.header.day_time || '-'} | {ageText} | Favs {favCount}
  </div>
  <div class="line dim">Keys: h help | q quit | {searchLabel || 'Search: -'} {demoLabel}{errorText ? ` | Error: ${errorText}` : ''}</div>
</section>

<style>
  .header {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.28rem 0.48rem;
    background: #10243a;
    margin-bottom: 0.35rem;
    overflow: hidden;
  }

  .header::after {
    content: '';
    position: absolute;
    inset: auto 0 0 0;
    height: 2px;
    background: #c23b46;
    transition: background 250ms ease;
  }

  .header[data-flag*='green']::after {
    background: var(--ok);
  }

  .header[data-flag*='yellow']::after,
  .header[data-flag*='code 60']::after,
  .header[data-flag*='safety']::after {
    background: var(--warn);
  }

  .header[data-flag*='red']::after {
    background: var(--danger);
  }

  .line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.75rem;
    line-height: 1.25;
  }

  .dim {
    color: var(--text-dim);
    margin-top: 0.08rem;
  }
</style>
