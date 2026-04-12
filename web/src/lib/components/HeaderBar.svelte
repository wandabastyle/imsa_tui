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
  <div class="context-row">
    <span class="status-dot" aria-hidden="true"></span>
    <span class="status-text">{snapshot?.status || 'Starting live timing...'}</span>
  </div>

  <h1 class="event-text">{eventText}</h1>
  <p class="session-text">{sessionDisplay}</p>

  <div class="chips" aria-label="Timing status chips">
    <span class="chip">Mode <strong>{viewModeLabel}</strong></span>
    <span class="chip">Upd <strong>{ageText.replace('Upd ', '')}</strong></span>
    <span class="chip">TTE <strong>{snapshot?.header.time_to_go || '-'}</strong></span>
    <span class="chip">Flag <strong>{displayFlag}</strong></span>
    <span class="chip">Favs <strong>{favCount}</strong></span>
  </div>

  <div class="aux-row">
    <span class="aux-chip">{searchLabel || 'Search: -'}</span>
    {#if demoLabel}
      <span class="aux-chip">{demoLabel}</span>
    {/if}
    {#if errorText}
      <span class="aux-chip error">Error: {errorText}</span>
    {/if}
  </div>
</section>

<style>
  .header {
    border: 1px solid color-mix(in srgb, var(--border) 72%, transparent);
    border-radius: 14px;
    padding: 0.65rem;
    background: linear-gradient(180deg, #141f34 0%, #111a2d 100%);
    color: var(--text);
    overflow: hidden;
  }

  .context-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--text-dim);
    font-size: 0.73rem;
  }

  .status-dot {
    width: 0.48rem;
    height: 0.48rem;
    border-radius: 999px;
    background: #7f9fc7;
  }

  .event-text {
    margin: 0.4rem 0 0;
    font-size: 1rem;
    line-height: 1.15;
    letter-spacing: 0.01em;
  }

  .session-text {
    margin: 0.18rem 0 0;
    color: var(--text-dim);
    font-size: 0.79rem;
  }

  .chips {
    margin-top: 0.52rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.32rem;
  }

  .chip {
    background: var(--chip-bg);
    border: 1px solid var(--chip-border);
    border-radius: 999px;
    padding: 0.2rem 0.5rem;
    font-size: 0.72rem;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .chip strong {
    color: var(--text);
    font-weight: 650;
    margin-left: 0.14rem;
  }

  .aux-row {
    margin-top: 0.42rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.28rem;
  }

  .aux-chip {
    border-radius: 999px;
    padding: 0.14rem 0.42rem;
    background: rgb(19 30 49 / 78%);
    color: var(--text-dim);
    font-size: 0.7rem;
    white-space: nowrap;
  }

  .aux-chip.error {
    color: #ffd2d2;
    background: rgb(143 45 53 / 28%);
  }

  .header[data-flag*='green'] .status-dot {
    background: #67d889;
  }

  .header[data-flag*='yellow'] .status-dot,
  .header[data-flag*='code 60'] .status-dot,
  .header[data-flag*='safety'] .status-dot {
    background: #f3d75b;
  }

  .header[data-flag*='red'] .status-dot {
    background: #ff7f92;
  }

  .header[data-flag*='white'] .status-dot,
  .header[data-flag*='checkered'] .status-dot,
  .header[data-flag*='chequered'] .status-dot {
    background: #d9e3f2;
  }

  @media (max-width: 900px) {
    .header {
      border-radius: 12px;
      padding: 0.55rem;
    }

    .event-text {
      font-size: 0.95rem;
    }
  }
</style>
