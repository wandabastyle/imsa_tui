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
  export let showLogoutChip = false;
  export let onLogout: () => void | Promise<void> = () => {};

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
  <div class="header-main">
    <div class="identity">
      <h1 class="event-text">{eventText}</h1>
      <p class="session-text">{sessionDisplay}</p>
    </div>

    <div class="meta-side">
      <div class="chips" aria-label="Timing status chips">
        <span class="chip">Mode <strong>{viewModeLabel}</strong></span>
        <span class="chip">Upd <strong>{ageText.replace('Upd ', '')}</strong></span>
        <span class="chip">TTE <strong>{snapshot?.header.time_to_go || '-'}</strong></span>
        <span class="chip">Flag <strong>{displayFlag}</strong></span>
        <span class="chip">Favs <strong>{favCount}</strong></span>
        {#if showLogoutChip}
          <button class="chip chip-logout" on:click={() => void onLogout()} type="button">Logout</button>
        {/if}
      </div>
      <p class="connection-text"><span class="status-dot" aria-hidden="true"></span>{snapshot?.status || 'Starting live timing...'}</p>
    </div>
  </div>

  {#if searchLabel || demoLabel || errorText}
    <div class="aux-row">
      {#if searchLabel}
        <span class="aux-chip">{searchLabel}</span>
      {/if}
      {#if demoLabel}
        <span class="aux-chip">{demoLabel}</span>
      {/if}
      {#if errorText}
        <span class="aux-chip error">Error: {errorText}</span>
      {/if}
    </div>
  {/if}
</section>

<style>
  .header {
    --header-dim: #b7c8df;
    --header-chip-bg: #253750;
    --header-chip-border: #415a7c;
    --header-chip-strong: #f2f7ff;
    --header-aux-bg: rgb(26 39 61 / 88%);
    border: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
    border-radius: 14px;
    padding: 0.65rem;
    background: linear-gradient(180deg, #1a2841 0%, #132038 100%);
    color: var(--text);
    overflow: hidden;
  }

  .header-main {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.7rem;
  }

  .identity {
    min-width: 0;
  }

  .meta-side {
    min-width: 0;
    display: grid;
    justify-items: end;
    align-content: start;
  }

  .connection-text {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--header-dim);
    font-size: 0.73rem;
    margin: 0.24rem 0 0;
    white-space: nowrap;
  }

  .status-dot {
    width: 0.48rem;
    height: 0.48rem;
    border-radius: 999px;
    background: #7f9fc7;
  }

  .event-text {
    margin: 0;
    font-size: 1rem;
    line-height: 1.15;
    letter-spacing: 0.01em;
  }

  .session-text {
    margin: 0.18rem 0 0;
    color: var(--header-dim);
    font-size: 0.79rem;
  }

  .chips {
    margin-top: 0.02rem;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: max-content;
    gap: 0.32rem;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips::-webkit-scrollbar {
    display: none;
  }

  .chip {
    background: var(--header-chip-bg);
    border: 1px solid var(--header-chip-border);
    border-radius: 999px;
    padding: 0.2rem 0.5rem;
    font-size: 0.72rem;
    color: var(--header-dim);
    white-space: nowrap;
  }

  .chip strong {
    color: var(--header-chip-strong);
    font-weight: 650;
    margin-left: 0.14rem;
  }

  .chip-logout {
    font: inherit;
    cursor: pointer;
    color: #ffe7eb;
    background: #6c2634;
    border-color: #8f4050;
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
    background: var(--header-aux-bg);
    color: var(--header-dim);
    font-size: 0.7rem;
    white-space: nowrap;
  }

  .aux-chip.error {
    color: #ffd2d2;
    background: rgb(143 45 53 / 28%);
  }

  .header[data-flag*='green'] {
    background: linear-gradient(180deg, #1d9a4a 0%, #16763b 100%);
    color: #07140a;
    border-color: #0f7b36;
    --header-dim: #1b3d25;
    --header-chip-bg: rgb(15 81 40 / 42%);
    --header-chip-border: rgb(10 95 43 / 76%);
    --header-chip-strong: #eefaf2;
    --header-aux-bg: rgb(17 91 45 / 32%);
  }

  .header[data-flag*='green'] .status-dot {
    background: #67d889;
  }

  .header[data-flag*='yellow'],
  .header[data-flag*='code 60'],
  .header[data-flag*='safety'] {
    background: linear-gradient(180deg, #ebd34f 0%, #caab32 100%);
    color: #1c1500;
    border-color: #af9025;
    --header-dim: #44360b;
    --header-chip-bg: rgb(142 112 26 / 36%);
    --header-chip-border: rgb(124 96 17 / 70%);
    --header-chip-strong: #fff8d9;
    --header-aux-bg: rgb(154 126 39 / 24%);
  }

  .header[data-flag*='yellow'] .status-dot,
  .header[data-flag*='code 60'] .status-dot,
  .header[data-flag*='safety'] .status-dot {
    background: #f3d75b;
  }

  .header[data-flag*='red'] {
    background: linear-gradient(180deg, #9d2535 0%, #7d1b2a 100%);
    color: #ffe7eb;
    border-color: #6a1624;
    --header-dim: #ffd0d8;
    --header-chip-bg: rgb(97 18 31 / 40%);
    --header-chip-border: rgb(106 26 38 / 72%);
    --header-chip-strong: #fff2f5;
    --header-aux-bg: rgb(105 24 36 / 30%);
  }

  .header[data-flag*='red'] .status-dot {
    background: #ff7f92;
  }

  .header[data-flag*='white'] {
    background: linear-gradient(180deg, #ebf1f9 0%, #d4deee 100%);
    color: #131b28;
    border-color: #c3d0e0;
    --header-dim: #33475f;
    --header-chip-bg: rgb(120 145 176 / 34%);
    --header-chip-border: rgb(94 120 151 / 60%);
    --header-chip-strong: #0f1725;
    --header-aux-bg: rgb(161 181 206 / 34%);
  }

  .header[data-flag*='checkered'],
  .header[data-flag*='chequered'] {
    background: linear-gradient(180deg, #d8dee9 0%, #c3cfdf 100%);
    color: #121a26;
    border-color: #aebad0;
    --header-dim: #31445d;
    --header-chip-bg: rgb(118 140 170 / 34%);
    --header-chip-border: rgb(93 117 148 / 62%);
    --header-chip-strong: #101826;
    --header-aux-bg: rgb(144 160 184 / 36%);
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

    .header-main {
      display: block;
    }

    .meta-side {
      justify-items: start;
      margin-top: 0.4rem;
    }

    .event-text {
      font-size: 0.95rem;
    }

    .chips {
      margin-top: 0;
    }

    .connection-text {
      margin-top: 0.28rem;
    }

    .header[data-flag*='green'] {
      background: linear-gradient(180deg, #1f2d39 0%, #19232f 100%);
      color: var(--text);
      border-color: color-mix(in srgb, #2f8f57 45%, var(--border));
      --header-dim: #afc3dc;
      --header-chip-bg: rgb(47 143 87 / 26%);
      --header-chip-border: rgb(47 143 87 / 48%);
      --header-chip-strong: #f0f6ff;
      --header-aux-bg: rgb(47 143 87 / 16%);
    }

    .header[data-flag*='yellow'],
    .header[data-flag*='code 60'],
    .header[data-flag*='safety'] {
      background: linear-gradient(180deg, #272620 0%, #202017 100%);
      color: var(--text);
      border-color: color-mix(in srgb, #b59f40 45%, var(--border));
      --header-dim: #b9c8dc;
      --header-chip-bg: rgb(181 159 64 / 24%);
      --header-chip-border: rgb(181 159 64 / 46%);
      --header-chip-strong: #f0f6ff;
      --header-aux-bg: rgb(181 159 64 / 14%);
    }

    .header[data-flag*='red'] {
      background: linear-gradient(180deg, #2b1c23 0%, #24151d 100%);
      color: var(--text);
      border-color: color-mix(in srgb, #9d2535 45%, var(--border));
      --header-dim: #b9c7db;
      --header-chip-bg: rgb(157 37 53 / 24%);
      --header-chip-border: rgb(157 37 53 / 44%);
      --header-chip-strong: #f0f6ff;
      --header-aux-bg: rgb(157 37 53 / 16%);
    }

    .header[data-flag*='white'],
    .header[data-flag*='checkered'],
    .header[data-flag*='chequered'] {
      background: linear-gradient(180deg, #1b2230 0%, #151b28 100%);
      color: var(--text);
      border-color: color-mix(in srgb, #90a2bf 42%, var(--border));
      --header-dim: #b0c3dd;
      --header-chip-bg: rgb(144 162 191 / 24%);
      --header-chip-border: rgb(144 162 191 / 44%);
      --header-chip-strong: #f0f6ff;
      --header-aux-bg: rgb(144 162 191 / 14%);
    }
  }
</style>
