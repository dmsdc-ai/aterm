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
    <div class="header-left">
      <svg class="header-icon" width="12" height="12" viewBox="0 0 12 12" fill="none">
        <circle cx="6" cy="6" r="4.5" stroke="currentColor" stroke-width="1"/>
        <path d="M6 3.5V6L7.5 7.5" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
      </svg>
      <span class="header-label">Timeline</span>
    </div>
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
        <div class="empty-visual">
          <svg class="empty-illustration" width="40" height="40" viewBox="0 0 40 40" fill="none">
            <path d="M20 4V36" stroke="currentColor" stroke-width="1" stroke-dasharray="2 3"/>
            <circle cx="20" cy="10" r="4" stroke="currentColor" stroke-width="1"/>
            <circle cx="20" cy="22" r="4" stroke="currentColor" stroke-width="1" opacity="0.5"/>
            <circle cx="20" cy="34" r="4" stroke="currentColor" stroke-width="1" opacity="0.25"/>
          </svg>
        </div>
        <div class="empty-content">
          <div class="empty-text">Waiting for events</div>
          <div class="empty-sub">Events from inject, synthesis, and routing will appear here</div>
        </div>
        <div class="empty-legend">
          <span class="legend-item">
            <span class="legend-dot" style="background: var(--color-accent-blue)"></span>
            inject
          </span>
          <span class="legend-item">
            <span class="legend-dot" style="background: var(--color-event-routed)"></span>
            routed
          </span>
          <span class="legend-item">
            <span class="legend-dot" style="background: var(--color-event-synthesis)"></span>
            synthesis
          </span>
        </div>
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
  /* ── Keyframes ───────────────────────────────────────── */
  @keyframes fadeInUp {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @keyframes breathe {
    0%, 100% { opacity: 0.3; transform: scale(1); }
    50%       { opacity: 0.6; transform: scale(1.08); }
  }

  /* ── Layout ─────────────────────────────────────────── */
  .timeline {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--color-bg-base);
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
    font-size: 12px;
    overflow: hidden;
  }

  /* ── Header ──────────────────────────────────────────── */
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 16px 8px;
    border-bottom: 1px solid var(--color-border-subtle);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .header-icon {
    color: var(--color-text-disabled);
  }

  .header-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.8px;
  }

  .header-count {
    font-size: 10px;
    color: var(--color-text-tertiary);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-pill);
    padding: 0 6px;
    line-height: 16px;
  }

  /* ── Session info grid ───────────────────────────────── */
  .session-info {
    padding: 8px 16px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }

  .info-grid {
    display: grid;
    grid-template-columns: 56px 1fr;
    row-gap: 1px;
    column-gap: 8px;
  }

  .info-key {
    font-size: 10px;
    color: var(--color-border-strong);
    padding: 2px 0;
    align-self: start;
    padding-top: 3px;
  }

  .info-val {
    font-size: 11px;
    color: var(--color-text-tertiary);
    padding: 2px 0;
    word-break: break-all;
  }

  .info-mono {
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .info-truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .remote-badge {
    color: var(--color-event-routed);
    background: rgba(210, 168, 255, 0.08);
    border: 1px solid rgba(210, 168, 255, 0.13);
    border-radius: var(--radius-md);
    padding: 0 5px;
    font-size: 10px;
    display: inline-block;
    line-height: 18px;
  }

  .local-badge {
    color: var(--color-text-disabled);
    font-size: 10px;
  }

  /* ── Section divider ─────────────────────────────────── */
  .section-rule {
    height: 1px;
    background: var(--color-border-subtle);
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
    background: var(--color-border-subtle);
    border-radius: var(--radius-xs);
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
    animation-name: fadeInUp;
    animation-duration: 150ms;
    animation-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
    animation-fill-mode: both;
  }

  /* Vertical line on the left */
  .tl-stem {
    width: 1px;
    background: var(--color-border-subtle);
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
    background: var(--color-border-subtle);
    border: 1px solid var(--color-border-default);
    transition: background var(--duration-normal) var(--ease-default),
                border-color var(--duration-normal) var(--ease-default);
  }

  /* ── Chat bubbles ────────────────────────────────────── */
  .bubble {
    flex: 1;
    background: var(--color-bg-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: 0 var(--radius-xl) var(--radius-xl) 0;
    padding: 7px 10px;
    margin-bottom: 4px;
    min-width: 0;
  }

  .inject-bubble {
    border-left: 2px solid var(--accent, var(--color-accent-blue));
    background: var(--color-bg-elevated);
  }

  .routed-bubble {
    border-left: 2px solid var(--color-event-routed);
    background: var(--color-event-routed-bg);
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
    color: var(--color-text-muted);
  }

  .routed-from {
    color: var(--color-event-routed);
  }

  .routed-to {
    color: var(--color-text-muted);
  }

  .route-arrow {
    color: var(--color-border-default);
    font-size: 11px;
  }

  .type-chip {
    font-size: 9px;
    border: 1px solid;
    border-radius: var(--radius-sm);
    padding: 0 4px;
    line-height: 14px;
    opacity: 0.8;
    margin-left: 2px;
  }

  .ev-time {
    margin-left: auto;
    font-size: 9px;
    color: var(--color-border-default);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .bubble-body {
    font-size: 11px;
    color: var(--color-text-tertiary);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }

  /* ── Synthesis card ──────────────────────────────────── */
  .event-synthesis {
    flex: 1;
    background: var(--color-event-synthesis-bg);
    border: 1px solid rgba(240, 136, 62, 0.13);
    border-left: 2px solid var(--color-event-synthesis);
    border-radius: 0 var(--radius-xl) var(--radius-xl) 0;
    padding: 8px 10px;
    margin-bottom: 4px;
    min-width: 0;
    overflow: hidden;
    box-shadow: inset 2px 0 8px rgba(240, 136, 62, 0.1);
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
    color: var(--color-event-synthesis);
    background: rgba(240, 136, 62, 0.09);
    border: 1px solid rgba(240, 136, 62, 0.2);
    border-radius: var(--radius-sm);
    padding: 0 5px;
    line-height: 15px;
  }

  .synthesis-from {
    font-size: 10px;
    color: var(--color-text-tertiary);
  }

  .synthesis-body {
    font-size: 11px;
    color: var(--color-event-synthesis-text);
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
    background: var(--color-bg-base);
    border: 1px solid var(--color-bg-overlay);
    border-radius: var(--radius-3xl);
    padding: 3px 10px;
    animation-name: fadeIn;
    animation-duration: 100ms;
    animation-timing-function: ease;
  }

  .sys-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .sys-text {
    font-size: 10px;
    color: var(--color-text-disabled);
    font-style: italic;
  }

  .sys-time {
    font-size: 9px;
    color: var(--color-border-default);
  }

  /* ── Expand button ───────────────────────────────────── */
  .expand-btn {
    background: none;
    border: none;
    color: var(--color-accent-blue);
    font-size: 10px;
    font-family: inherit;
    cursor: pointer;
    padding: 3px 0 0;
    display: block;
    opacity: 0.7;
    transition: opacity var(--duration-normal) var(--ease-default);
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
    padding: 48px 24px;
    gap: 16px;
    animation-name: fadeIn;
    animation-duration: 200ms;
    animation-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
  }

  .empty-visual {
    color: var(--color-border-default);
    animation: breathe 3s ease-in-out infinite;
  }

  .empty-illustration {
    opacity: 0.5;
  }

  .empty-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .empty-text {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .empty-sub {
    font-size: 10px;
    color: var(--color-text-disabled);
    text-align: center;
    max-width: 200px;
    line-height: 1.4;
  }

  .empty-legend {
    display: flex;
    gap: 12px;
    padding: 6px 12px;
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-pill);
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 9px;
    color: var(--color-text-disabled);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .legend-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }
</style>
