<script>
  import { busEvents, selectedSession } from '../lib/stores.js';
  import { afterUpdate } from 'svelte';

  let eventsEl;
  let expandedEvents = new Set();

  // Events that spam the timeline with no signal value
  const NOISE_TYPES = new Set([
    'session_health',
    'health',
    'ping',
    'pong',
    'heartbeat',
    'keepalive',
  ]);

  // Only these event types are meaningful enough to display
  const SIGNAL_TYPES = new Set([
    'inject',
    'injection',
    'inject_written',
    'message_routed',
    'synthesis',
    'session_register',
    'session_exit',
    'status_change',
  ]);

  const TRUNCATE_LEN = 200;

  function isSignal(e) {
    if (NOISE_TYPES.has(e.type)) return false;
    // If we have an explicit allowlist hit, always show
    if (SIGNAL_TYPES.has(e.type)) return true;
    // For unknown types: show them — they're not noise we know about
    return true;
  }

  // HH:MM only — clean, no seconds
  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString('en-GB', {
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function eventKind(e) {
    if (e.type === 'inject_written' || e.type === 'injection' || e.type === 'inject') return 'inject';
    if (e.type === 'message_routed') return 'routed';
    if (e.type === 'synthesis') return 'synthesis';
    return 'system';
  }

  function eventAccent(e) {
    if (e.type === 'inject_written' || e.type === 'inject') return '#58a6ff';
    if (e.type === 'injection') return '#3fb950';
    if (e.type === 'message_routed') return '#d2a8ff';
    if (e.type === 'synthesis') return '#f0883e';
    if (e.type === 'session_register') return '#3fb950';
    if (e.type === 'session_exit') return '#f85149';
    if (e.type === 'status_change') return '#d29922';
    return '#484f58';
  }

  function messageContent(e) {
    return e.prompt || e.data || e.content || e.message || null;
  }

  function systemLabel(e) {
    if (e.type === 'session_register') return `${e.session_id || e.id || 'session'} joined`;
    if (e.type === 'session_exit') return `${e.session_id || e.id || 'session'} left`;
    if (e.type === 'status_change') return `status → ${e.status || e.state || '?'}`;
    return e.type || 'event';
  }

  function toggleExpand(id) {
    if (expandedEvents.has(id)) {
      expandedEvents.delete(id);
    } else {
      expandedEvents.add(id);
    }
    expandedEvents = expandedEvents;
  }

  $: filteredEvents = $busEvents.filter(isSignal).slice(-60);

  afterUpdate(() => {
    if (eventsEl) {
      eventsEl.scrollTop = eventsEl.scrollHeight;
    }
  });
</script>

<div class="timeline">
  <!-- Header -->
  <div class="panel-header">
    <span class="header-label">Timeline</span>
    {#if filteredEvents.length > 0}
      <span class="header-count">{filteredEvents.length}</span>
    {/if}
  </div>

  <!-- Session info -->
  {#if $selectedSession}
    <div class="session-info">
      <div class="info-grid">
        <span class="info-key">id</span>
        <span class="info-val">{$selectedSession.id}</span>

        <span class="info-key">cmd</span>
        <span class="info-val info-mono">{$selectedSession.command || '—'}</span>

        <span class="info-key">cwd</span>
        <span class="info-val info-mono info-truncate" title={$selectedSession.cwd}>{$selectedSession.cwd || '—'}</span>

        <span class="info-key">clients</span>
        <span class="info-val">{$selectedSession.active_clients ?? 0}</span>

        <span class="info-key">idle</span>
        <span class="info-val">{$selectedSession.idleSeconds ?? 0}s</span>

        <span class="info-key">machine</span>
        {#if $selectedSession.machine && $selectedSession.machine !== 'localhost' && $selectedSession.machine !== '127.0.0.1'}
          <span class="info-val remote-badge">{$selectedSession.machine}</span>
        {:else if $selectedSession.host && $selectedSession.host !== 'localhost' && $selectedSession.host !== '127.0.0.1'}
          <span class="info-val remote-badge">{$selectedSession.host}</span>
        {:else}
          <span class="info-val local-badge">local</span>
        {/if}
      </div>
    </div>
  {/if}

  <div class="section-rule"></div>

  <!-- Event feed -->
  <div class="events" bind:this={eventsEl}>
    {#if filteredEvents.length === 0}
      <div class="empty-state">
        <div class="empty-icon">◌</div>
        <div class="empty-text">Waiting for events</div>
        <div class="empty-sub">session_health and noise are filtered</div>
      </div>
    {:else}
      <div class="event-list">
        {#each filteredEvents as event, i}
          {@const kind = eventKind(event)}
          {@const accent = eventAccent(event)}
          {@const content = messageContent(event)}
          {@const eventId = event._ts + '-' + i}
          {@const isExpanded = expandedEvents.has(eventId)}
          {@const needsTruncate = content && content.length > TRUNCATE_LEN}
          {@const displayContent = content && !isExpanded && needsTruncate
            ? content.slice(0, TRUNCATE_LEN) + '…'
            : content}

          {#if kind === 'system'}
            <!-- Telegram-style centered system message -->
            <div class="sys-row">
              <div class="sys-pill">
                <span class="sys-dot" style="background: {accent}"></span>
                <span class="sys-text">{systemLabel(event)}</span>
                <span class="sys-time">{formatTime(event._ts)}</span>
              </div>
            </div>

          {:else if kind === 'synthesis'}
            <!-- Synthesis: full-width highlighted card -->
            <div class="event-row">
              <div class="tl-stem"></div>
              <div class="event-synthesis">
                <div class="synthesis-header">
                  <span class="synthesis-badge">synthesis</span>
                  {#if event.from}
                    <span class="synthesis-from">{event.from}</span>
                  {/if}
                  <span class="ev-time">{formatTime(event._ts)}</span>
                </div>
                {#if displayContent}
                  <div class="synthesis-body">{displayContent}</div>
                  {#if needsTruncate}
                    <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                      {isExpanded ? '↑ less' : '↓ more'}
                    </button>
                  {/if}
                {/if}
              </div>
            </div>

          {:else if kind === 'routed'}
            <!-- Message routed: purple bubble -->
            <div class="event-row">
              <div class="tl-stem"></div>
              <div class="bubble routed-bubble">
                <div class="bubble-header">
                  <span class="agent-tag routed-from">{event.from || '?'}</span>
                  <span class="route-arrow">→</span>
                  <span class="agent-tag routed-to">{event.to || '?'}</span>
                  <span class="ev-time">{formatTime(event._ts)}</span>
                </div>
                {#if displayContent}
                  <div class="bubble-body">{displayContent}</div>
                  {#if needsTruncate}
                    <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                      {isExpanded ? '↑ less' : '↓ more'}
                    </button>
                  {/if}
                {/if}
              </div>
            </div>

          {:else}
            <!-- inject / inject_written / injection: blue/green bubble -->
            <div class="event-row">
              <div class="tl-stem"></div>
              <div class="bubble inject-bubble" style="--accent: {accent}">
                <div class="bubble-header">
                  <span class="agent-tag inject-from" style="color: {accent}">
                    {event.from || event.source || event.type}
                  </span>
                  {#if event.target || event.target_agent}
                    <span class="route-arrow">→</span>
                    <span class="agent-tag inject-to">{event.target || event.target_agent}</span>
                  {/if}
                  <span class="type-chip" style="color: {accent}; border-color: {accent}22">
                    {event.type}
                  </span>
                  <span class="ev-time">{formatTime(event._ts)}</span>
                </div>
                {#if displayContent}
                  <div class="bubble-body">{displayContent}</div>
                  {#if needsTruncate}
                    <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                      {isExpanded ? '↑ less' : '↓ more'}
                    </button>
                  {/if}
                {/if}
              </div>
            </div>
          {/if}
        {/each}

        <!-- Bottom anchor for scroll -->
        <div class="scroll-anchor"></div>
      </div>
    {/if}
  </div>
</div>

<style>
  /* ── Layout ─────────────────────────────────────────── */
  .timeline {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: #0d1117;
    color: #c9d1d9;
    font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', ui-monospace, monospace;
    font-size: 12px;
    overflow: hidden;
  }

  /* ── Header ──────────────────────────────────────────── */
  .panel-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px 8px;
    border-bottom: 1px solid #21262d;
    flex-shrink: 0;
  }

  .header-label {
    font-size: 10px;
    font-weight: 600;
    color: #6e7681;
    text-transform: uppercase;
    letter-spacing: 0.8px;
  }

  .header-count {
    font-size: 10px;
    color: #8b949e;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 10px;
    padding: 0 6px;
    line-height: 16px;
  }

  /* ── Session info grid ───────────────────────────────── */
  .session-info {
    padding: 8px 16px;
    flex-shrink: 0;
    border-bottom: 1px solid #21262d;
  }

  .info-grid {
    display: grid;
    grid-template-columns: 56px 1fr;
    row-gap: 1px;
    column-gap: 8px;
  }

  .info-key {
    font-size: 10px;
    color: #484f58;
    padding: 2px 0;
    align-self: start;
    padding-top: 3px;
  }

  .info-val {
    font-size: 11px;
    color: #8b949e;
    padding: 2px 0;
    word-break: break-all;
  }

  .info-mono {
    font-size: 10px;
    color: #6e7681;
  }

  .info-truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .remote-badge {
    color: #d2a8ff;
    background: #d2a8ff14;
    border: 1px solid #d2a8ff22;
    border-radius: 4px;
    padding: 0 5px;
    font-size: 10px;
    display: inline-block;
    line-height: 18px;
  }

  .local-badge {
    color: #484f58;
    font-size: 10px;
  }

  /* ── Section divider ─────────────────────────────────── */
  .section-rule {
    height: 1px;
    background: #21262d;
    margin: 0;
    flex-shrink: 0;
  }

  /* ── Event scroll container ──────────────────────────── */
  .events {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 8px 0 0;
  }

  .events::-webkit-scrollbar {
    width: 4px;
  }

  .events::-webkit-scrollbar-track {
    background: transparent;
  }

  .events::-webkit-scrollbar-thumb {
    background: #21262d;
    border-radius: 2px;
  }

  .event-list {
    display: flex;
    flex-direction: column;
    padding-bottom: 16px;
  }

  .scroll-anchor {
    height: 1px;
  }

  /* ── Timeline spine: each event row ─────────────────── */
  .event-row {
    display: flex;
    align-items: stretch;
    gap: 0;
    padding: 2px 12px 2px 16px;
    position: relative;
  }

  /* Vertical line on the left */
  .tl-stem {
    width: 1px;
    background: #21262d;
    margin-right: 12px;
    flex-shrink: 0;
    position: relative;
    min-height: 100%;
  }

  /* Dot on the stem — pseudo element attached to the bubble via sibling */
  .tl-stem::before {
    content: '';
    position: absolute;
    top: 10px;
    left: -3px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #21262d;
    border: 1px solid #30363d;
  }

  /* ── Chat bubbles ────────────────────────────────────── */
  .bubble {
    flex: 1;
    background: #161b22;
    border: 1px solid #21262d;
    border-radius: 0 8px 8px 0;
    padding: 7px 10px;
    margin-bottom: 4px;
    min-width: 0;
  }

  .inject-bubble {
    border-left: 2px solid var(--accent, #58a6ff);
    background: #161b22;
  }

  .routed-bubble {
    border-left: 2px solid #d2a8ff;
    background: #17141f;
  }

  .bubble-header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 5px;
    margin-bottom: 5px;
  }

  .agent-tag {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.2px;
    white-space: nowrap;
  }

  .inject-from {
    /* color set inline */
  }

  .inject-to {
    color: #6e7681;
  }

  .routed-from {
    color: #d2a8ff;
  }

  .routed-to {
    color: #6e7681;
  }

  .route-arrow {
    color: #30363d;
    font-size: 11px;
  }

  .type-chip {
    font-size: 9px;
    border: 1px solid;
    border-radius: 3px;
    padding: 0 4px;
    line-height: 14px;
    opacity: 0.8;
    margin-left: 2px;
  }

  .ev-time {
    margin-left: auto;
    font-size: 9px;
    color: #30363d;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .bubble-body {
    font-size: 11px;
    color: #8b949e;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }

  /* ── Synthesis card ──────────────────────────────────── */
  .event-synthesis {
    flex: 1;
    background: #16120a;
    border: 1px solid #f0883e22;
    border-left: 2px solid #f0883e;
    border-radius: 0 8px 8px 0;
    padding: 8px 10px;
    margin-bottom: 4px;
    min-width: 0;
  }

  .synthesis-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }

  .synthesis-badge {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: #f0883e;
    background: #f0883e18;
    border: 1px solid #f0883e33;
    border-radius: 3px;
    padding: 0 5px;
    line-height: 15px;
  }

  .synthesis-from {
    font-size: 10px;
    color: #8b949e;
  }

  .synthesis-body {
    font-size: 11px;
    color: #d4a96a;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }

  /* ── System messages (centered pill) ────────────────── */
  .sys-row {
    display: flex;
    justify-content: center;
    padding: 5px 16px;
  }

  .sys-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: #0d1117;
    border: 1px solid #1c2128;
    border-radius: 12px;
    padding: 3px 10px;
  }

  .sys-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .sys-text {
    font-size: 10px;
    color: #484f58;
    font-style: italic;
  }

  .sys-time {
    font-size: 9px;
    color: #30363d;
  }

  /* ── Expand button ───────────────────────────────────── */
  .expand-btn {
    background: none;
    border: none;
    color: #58a6ff;
    font-size: 10px;
    font-family: inherit;
    cursor: pointer;
    padding: 3px 0 0;
    display: block;
    opacity: 0.7;
    transition: opacity 0.15s;
  }

  .expand-btn:hover {
    opacity: 1;
    text-decoration: underline;
  }

  /* ── Empty state ─────────────────────────────────────── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 40px 24px;
    gap: 8px;
  }

  .empty-icon {
    font-size: 24px;
    color: #21262d;
    line-height: 1;
  }

  .empty-text {
    font-size: 12px;
    color: #484f58;
  }

  .empty-sub {
    font-size: 10px;
    color: #30363d;
    text-align: center;
  }
</style>
