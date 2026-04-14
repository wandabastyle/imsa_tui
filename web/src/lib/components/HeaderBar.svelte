<script lang="ts">
  // Compact status header matching TUI metadata order and flag-driven theming.

  import type { Series, SeriesSnapshot } from '$lib/types';

  export let series: Series;
  export let snapshot: SeriesSnapshot | null;
  export let viewModeLabel = 'Overall';
  export let favCount = 0;
  export let searchLabel = '';
  export let demoLabel = '';
  export let errorText = '';
  export let displayFlag = '-';

  $: ageText = snapshot?.last_update_unix_ms
    ? `Upd ${Math.max(0, Math.floor((Date.now() - snapshot.last_update_unix_ms) / 1000))}s`
    : 'Upd -';
  function displayEventName(raw: string): string {
    if (!raw || raw.trim() === '') {
      return '-';
    }
    return raw.trim();
  }

  function displaySessionName(activeSeries: Series, raw: string): string {
    if (!raw || raw.trim() === '') {
      return '-';
    }
    if (activeSeries === 'imsa') {
      const cleaned = normalizeImsaLabel(raw);
      return cleaned || raw;
    }
    return raw;
  }

  function normalizeImsaLabel(raw: string): string {
    const lower = raw.toLowerCase();
    let cleaned = raw.trim();
    if (lower.includes('weathertech')) {
      const idx = Math.max(raw.lastIndexOf('-'), raw.lastIndexOf('–'), raw.lastIndexOf('—'));
      if (idx >= 0) {
        cleaned = raw.slice(idx + 1).trim();
      }
    }
    return cleaned;
  }

  $: eventText = displayEventName(snapshot?.header.event_name || '-');
  $: sessionText = displaySessionName(series, snapshot?.header.session_name || '-');
  $: sessionDisplay = sessionText;
</script>

<section class="header" data-flag={displayFlag.toLowerCase()}>
  <div class="line">
    {snapshot?.status || 'Starting live timing...'} | {eventText} | {sessionDisplay} | TTE {snapshot?.header.time_to_go || '-'} | Mode {viewModeLabel} | <strong>{displayFlag}</strong> | {ageText} | Favs {favCount}
  </div>
  <div class="line dim">Keys: h help | d demo | {searchLabel || 'Search: -'} {demoLabel}{errorText ? ` | Error: ${errorText}` : ''}</div>
</section>

<style>
  .header {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.28rem 0.48rem;
    position: relative;
    background: #10243a;
    color: #e7eef7;
    --header-dim: #afc0d6;
    margin-bottom: 0;
    overflow: hidden;
  }

  .header[data-flag*='green'] {
    background: #11a13a;
    color: #08170c;
    --header-dim: #112515;
    border-color: #0e8b32;
  }

  .header[data-flag*='yellow'],
  .header[data-flag*='code 60'],
  .header[data-flag*='safety'] {
    background: #f5dd08;
    color: #131100;
    --header-dim: #2f2a00;
    border-color: #d6bf00;
  }

  .header[data-flag*='red'] {
    background: #8e1e2b;
    color: #ffe6ea;
    --header-dim: #ffd0d7;
    border-color: #741522;
  }

  .header[data-flag*='white'] {
    background: #f2f5f9;
    color: #111821;
    --header-dim: #2f3f53;
    border-color: #cfd9e4;
  }

  .header[data-flag*='checkered'],
  .header[data-flag*='chequered'] {
    background: #dadfe8;
    color: #101722;
    --header-dim: #33465f;
    border-color: #bcc7d6;
  }

  .line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.75rem;
    line-height: 1.25;
  }

  .dim {
    color: var(--header-dim);
    margin-top: 0.08rem;
  }
</style>
