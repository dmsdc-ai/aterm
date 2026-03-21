<script>
  import { activeWorkspace } from '../lib/stores.js';

  // Empty event list (telepty bus removed)
  const busEvents = { subscribe: (fn) => { fn([]); return () => {}; } };
  import { afterUpdate } from 'svelte';

  let eventsEl;
  let expandedEvents = new Set();

  const NOISE_TYPES = new Set([
    'session_health', 'health', 'ping', 'pong', 'heartbeat', 'keepalive',
  ]);

  const SIGNAL_TYPES = new Set([
    'inject', 'injection', 'inject_written', 'message_routed',
    'synthesis', 'session_register', 'session_exit', 'status_change',
  ]);

  const TRUNCATE_LEN = 200;

  function isSignal(e) {
    if (NOISE_TYPES.has(e.type)) return false;
    if (SIGNAL_TYPES.has(e.type)) return true;
    return true;
  }

  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  }

  function eventKind(e) {
    if (e.type === 'inject_written' || e.type === 'injection' || e.type === 'inject') return 'inject';
    if (e.type === 'message_routed') return 'routed';
    if (e.type === 'synthesis') return 'synthesis';
    return 'system';
  }

  function eventAccent(e) {
    if (e.type === 'inject_written' || e.type === 'inject') return 'var(--accent)';
    if (e.type === 'injection')      return 'var(--status-active)';
    if (e.type === 'message_routed') return 'var(--event-routed)';
    if (e.type === 'synthesis')      return 'var(--event-synthesis)';
    if (e.type === 'session_register') return 'var(--status-active)';
    if (e.type === 'session_exit')   return 'var(--status-danger)';
    if (e.type === 'status_change')  return 'var(--status-warning)';
    return 'var(--text-disabled)';
  }

  function messageContent(e) {
    return e.prompt || e.data || e.content || e.message || null;
  }

  function systemLabel(e) {
    if (e.type === 'session_register') return `${e.session_id || e.id || 'session'} joined`;
    if (e.type === 'session_exit')     return `${e.session_id || e.id || 'session'} left`;
    if (e.type === 'status_change')    return `status → ${e.status || e.state || '?'}`;
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
    if (eventsEl) eventsEl.scrollTop = eventsEl.scrollHeight;
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
  {#if $activeWorkspace}
    <div class="session-info">
      <div class="info-grid">
        <span class="info-key">id</span>
        <span class="info-val">{$activeWorkspace.id}</span>

        <span class="info-key">cmd</span>
        <span class="info-val info-mono">{$activeWorkspace.command || '—'}</span>

        <span class="info-key">cwd</span>
        <span class="info-val info-mono info-truncate" title={$activeWorkspace.cwd}>{$activeWorkspace.cwd || '—'}</span>

        <span class="info-key">clients</span>
        <span class="info-val">{$activeWorkspace.active_clients ?? 0}</span>

        <span class="info-key">idle</span>
        <span class="info-val">{$activeWorkspace.idleSeconds ?? 0}s</span>

        <span class="info-key">machine</span>
        {#if $activeWorkspace.machine && $activeWorkspace.machine !== 'localhost' && $activeWorkspace.machine !== '127.0.0.1'}
          <span class="info-val remote-badge">{$activeWorkspace.machine}</span>
        {:else if $activeWorkspace.host && $activeWorkspace.host !== 'localhost' && $activeWorkspace.host !== '127.0.0.1'}
          <span class="info-val remote-badge">{$activeWorkspace.host}</span>
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
        <svg width="2" height="40" viewBox="0 0 2 40" fill="none" class="empty-line">
          <line x1="1" y1="0" x2="1" y2="40" stroke="currentColor" stroke-opacity="0.25" stroke-width="1" stroke-dasharray="2 4"/>
        </svg>
        <div class="empty-content">
          <div class="empty-text">Waiting for events</div>
          <div class="empty-sub">Events will appear as agents communicate</div>
        </div>
        <div class="empty-legend">
          <div class="legend-item">
            <span class="legend-dot" style="background: var(--accent)"></span>
            <span class="legend-label">inject</span>
          </div>
          <div class="legend-item">
            <span class="legend-dot" style="background: var(--event-routed)"></span>
            <span class="legend-label">routed</span>
          </div>
          <div class="legend-item">
            <span class="legend-dot" style="background: var(--event-synthesis)"></span>
            <span class="legend-label">synthesis</span>
          </div>
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
            <div class="sys-row">
              <div class="sys-pill">
                <span class="sys-dot" style="background: {accent}"></span>
                <span class="sys-text">{systemLabel(event)}</span>
                <span class="sys-time">{formatTime(event._ts)}</span>
              </div>
            </div>

          {:else if kind === 'synthesis'}
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
            <div class="event-row">
              <div class="tl-stem"></div>
              <div class="bubble inject-bubble" style="--ev-accent: {accent}">
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
        <div class="scroll-anchor"></div>
      </div>
    {/if}
  </div>
</div>

<style>
  @keyframes fadeInUp {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @keyframes fadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }
  @keyframes breathe {
    0%, 100% { opacity: 0.3; }
    50%       { opacity: 0.6; }
  }

  /* ── Layout ── */
  .timeline {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-timeline);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 12px;
    overflow: hidden;
  }

  /* ── Header ── */
  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
    user-select: none;
    -webkit-user-select: none;
  }

  .header-label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .header-count {
    font-size: 10px;
    color: var(--text-tertiary);
    background: var(--bg-button);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-pill);
    padding: 0 6px;
    line-height: 16px;
  }

  /* ── Session info ── */
  .session-info {
    padding: 8px 16px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .info-grid {
    display: grid;
    grid-template-columns: 56px 1fr;
    row-gap: 1px;
    column-gap: 8px;
  }

  .info-key {
    font-size: 10px;
    color: var(--text-disabled);
    padding: 2px 0;
    align-self: start;
    padding-top: 3px;
  }

  .info-val {
    font-size: 11px;
    color: var(--text-tertiary);
    padding: 2px 0;
    word-break: break-all;
  }

  .info-mono    { font-size: 10px; color: var(--text-muted); }
  .info-truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .remote-badge {
    color: var(--event-routed);
    background: rgba(184, 160, 216, 0.08);
    border: 1px solid rgba(184, 160, 216, 0.13);
    border-radius: var(--radius-md);
    padding: 0 5px;
    font-size: 10px;
    display: inline-block;
    line-height: 18px;
  }

  .local-badge { color: var(--text-disabled); font-size: 10px; }

  /* ── Section rule ── */
  .section-rule {
    height: 1px;
    background: var(--border-subtle);
    flex-shrink: 0;
  }

  /* ── Events scroll container ── */
  .events {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 8px 0 0;
  }

  .events::-webkit-scrollbar       { width: 4px; }
  .events::-webkit-scrollbar-track { background: transparent; }
  .events::-webkit-scrollbar-thumb { background: var(--border-subtle); border-radius: 3px; }

  .event-list {
    display: flex;
    flex-direction: column;
    padding-bottom: 16px;
  }

  .scroll-anchor { height: 1px; }

  /* ── Timeline event rows ── */
  .event-row {
    display: flex;
    align-items: stretch;
    padding: 2px 12px 2px 16px;
    position: relative;
    animation: fadeInUp 150ms cubic-bezier(0.16, 1, 0.3, 1) both;
  }

  .tl-stem {
    width: 1px;
    background: var(--border-subtle);
    margin-right: 12px;
    flex-shrink: 0;
    position: relative;
    min-height: 100%;
  }

  .tl-stem::before {
    content: '';
    position: absolute;
    top: 10px;
    left: -3px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--border-subtle);
    border: 1px solid var(--border-default);
  }

  /* ── Chat bubbles ── */
  .bubble {
    flex: 1;
    background: var(--bg-sidebar-hover);
    border: 1px solid var(--border-subtle);
    border-radius: 0 var(--radius-xl) var(--radius-xl) 0;
    padding: 7px 10px;
    margin-bottom: 4px;
    min-width: 0;
  }

  .inject-bubble {
    border-left: 2px solid var(--ev-accent, var(--accent));
    background: var(--bg-sidebar-hover);
  }

  .routed-bubble {
    border-left: 2px solid var(--event-routed);
    background: var(--event-routed-bg);
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

  .inject-to   { color: var(--text-muted); }
  .routed-from { color: var(--event-routed); }
  .routed-to   { color: var(--text-muted); }

  .route-arrow { color: var(--border-default); font-size: 11px; }

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
    color: var(--border-default);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .bubble-body {
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }

  /* ── Synthesis card ── */
  .event-synthesis {
    flex: 1;
    background: var(--event-synthesis-bg);
    border: 1px solid rgba(212, 168, 83, 0.13);
    border-left: 2px solid var(--event-synthesis);
    border-radius: 0 var(--radius-xl) var(--radius-xl) 0;
    padding: 8px 10px;
    margin-bottom: 4px;
    min-width: 0;
    overflow: hidden;
    box-shadow: inset 2px 0 8px rgba(212, 168, 83, 0.08);
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
    color: var(--event-synthesis);
    background: rgba(212, 168, 83, 0.09);
    border: 1px solid rgba(212, 168, 83, 0.2);
    border-radius: var(--radius-sm);
    padding: 0 5px;
    line-height: 15px;
  }

  .synthesis-from { font-size: 10px; color: var(--text-tertiary); }

  .synthesis-body {
    font-size: 11px;
    color: var(--event-synthesis-text);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }

  /* ── System messages ── */
  .sys-row {
    display: flex;
    justify-content: center;
    padding: 5px 16px;
  }

  .sys-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: var(--bg-timeline);
    border: 1px solid var(--border-subtle);
    border-radius: 24px;
    padding: 3px 10px;
    animation: fadeIn 100ms ease;
  }

  .sys-dot  { width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0; }
  .sys-text { font-size: 10px; color: var(--text-disabled); font-style: italic; }
  .sys-time { font-size: 9px; color: var(--border-default); }

  /* ── Expand button ── */
  .expand-btn {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 10px;
    font-family: inherit;
    cursor: pointer;
    padding: 3px 0 0;
    display: block;
    opacity: 0.7;
    transition: opacity 150ms ease;
  }

  .expand-btn:hover { opacity: 1; text-decoration: underline; }

  /* ── Empty state ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 24px;
    gap: 14px;
    animation: fadeIn 200ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .empty-line { color: var(--text-disabled); }

  .empty-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .empty-text {
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .empty-sub {
    font-family: var(--font-sans);
    font-size: 10px;
    color: var(--text-disabled);
    text-align: center;
    max-width: 180px;
    line-height: 1.4;
  }

  .empty-legend {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 6px;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .legend-dot {
    width: 5px;
    height: 5px;
    border-radius: 1px;
    flex-shrink: 0;
  }

  .legend-label {
    font-family: var(--font-sans);
    font-size: 9px;
    color: var(--text-disabled);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
</style>
