<script>
  import { busEvents, selectedSession } from '../lib/stores.js';
  import { afterUpdate } from 'svelte';

  let eventsEl;
  let expandedEvents = new Set();

  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function eventColor(e) {
    if (e.type === 'inject_written') return '#58a6ff';
    if (e.type === 'injection') return '#3fb950';
    if (e.type === 'message_routed') return '#d2a8ff';
    if (e.type === 'synthesis') return '#f0883e';
    return '#8b949e';
  }

  function eventKind(e) {
    if (e.type === 'inject_written' || e.type === 'injection') return 'inject';
    if (e.type === 'message_routed') return 'routed';
    if (e.type === 'synthesis') return 'synthesis';
    return 'system';
  }

  function messageContent(e) {
    return e.prompt || e.data || e.content || e.message || null;
  }

  function systemLabel(e) {
    if (e.type === 'session_register') return `${e.session_id || 'session'} registered`;
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

  const TRUNCATE_LEN = 180;

  afterUpdate(() => {
    if (eventsEl) {
      eventsEl.scrollTop = eventsEl.scrollHeight;
    }
  });
</script>

<div class="timeline">
  <div class="panel-header">Timeline</div>

  {#if $selectedSession}
    <div class="session-info">
      <div class="info-row"><span class="label">ID</span><span>{$selectedSession.id}</span></div>
      <div class="info-row"><span class="label">Command</span><span>{$selectedSession.command}</span></div>
      <div class="info-row"><span class="label">CWD</span><span class="mono">{$selectedSession.cwd}</span></div>
      <div class="info-row"><span class="label">Clients</span><span>{$selectedSession.active_clients}</span></div>
      <div class="info-row"><span class="label">Idle</span><span>{$selectedSession.idleSeconds}s</span></div>
      <div class="info-row">
        <span class="label">Machine</span>
        {#if $selectedSession.machine && $selectedSession.machine !== 'localhost' && $selectedSession.machine !== '127.0.0.1'}
          <span class="remote-host">{$selectedSession.machine}</span>
        {:else if $selectedSession.host && $selectedSession.host !== 'localhost' && $selectedSession.host !== '127.0.0.1'}
          <span class="remote-host">{$selectedSession.host}</span>
        {:else}
          <span>local</span>
        {/if}
      </div>
    </div>
    <div class="divider"></div>
  {/if}

  <div class="events" bind:this={eventsEl}>
    {#each [...$busEvents].slice(-50) as event, i}
      {@const kind = eventKind(event)}
      {@const color = eventColor(event)}
      {@const content = messageContent(event)}
      {@const eventId = event._ts + '-' + i}
      {@const isExpanded = expandedEvents.has(eventId)}
      {@const needsTruncate = content && content.length > TRUNCATE_LEN}
      {@const displayContent = content && !isExpanded && needsTruncate
        ? content.slice(0, TRUNCATE_LEN) + '…'
        : content}

      {#if kind === 'inject'}
        <div class="event-bubble" style="border-left-color: {color}">
          <div class="bubble-header">
            <span class="bubble-from" style="color: {color}">
              {event.from || event.source || event.type}
            </span>
            {#if event.target || event.target_agent}
              <span class="bubble-arrow">→</span>
              <span class="bubble-to">{event.target || event.target_agent}</span>
            {/if}
            <span class="time">{formatTime(event._ts)}</span>
          </div>
          {#if displayContent}
            <div class="bubble-body">{displayContent}</div>
            {#if needsTruncate}
              <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                {isExpanded ? 'show less' : 'show more'}
              </button>
            {/if}
          {/if}
        </div>

      {:else if kind === 'routed'}
        <div class="event-routed" style="border-left-color: {color}">
          <div class="routed-header">
            <span class="routed-from" style="color: {color}">{event.from || '?'}</span>
            <span class="routed-arrow" style="color: {color}">→</span>
            <span class="routed-to">{event.to || '?'}</span>
            <span class="time">{formatTime(event._ts)}</span>
          </div>
          {#if displayContent}
            <div class="routed-body">{displayContent}</div>
            {#if needsTruncate}
              <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                {isExpanded ? 'show less' : 'show more'}
              </button>
            {/if}
          {/if}
        </div>

      {:else if kind === 'synthesis'}
        <div class="event-synthesis">
          <div class="synthesis-label" style="color: {color}">synthesis</div>
          {#if displayContent}
            <div class="synthesis-body">{displayContent}</div>
            {#if needsTruncate}
              <button class="expand-btn" on:click={() => toggleExpand(eventId)}>
                {isExpanded ? 'show less' : 'show more'}
              </button>
            {/if}
          {/if}
          <div class="synthesis-meta">
            {#if event.from}<span>{event.from}</span>{/if}
            <span class="time">{formatTime(event._ts)}</span>
          </div>
        </div>

      {:else}
        <div class="event-system">
          <span class="system-text">{systemLabel(event)}</span>
          <span class="time system-time">{formatTime(event._ts)}</span>
        </div>
      {/if}
    {/each}

    {#if $busEvents.length === 0}
      <div class="empty-events">Waiting for events...</div>
    {/if}
  </div>
</div>

<style>
  .timeline {
    padding: 8px 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: #0d1117;
    color: #e6edf3;
  }

  .panel-header {
    padding: 8px 16px;
    font-size: 11px;
    color: #8b949e;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    flex-shrink: 0;
  }

  .session-info { padding: 4px 16px; flex-shrink: 0; }
  .info-row { display: flex; justify-content: space-between; font-size: 12px; padding: 2px 0; }
  .label { color: #8b949e; }
  .mono { font-family: monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; max-width: 160px; }
  .remote-host { color: #d2a8ff; }

  .divider { border-top: 1px solid #21262d; margin: 8px 16px; flex-shrink: 0; }

  .events {
    flex: 1;
    overflow-y: auto;
    padding: 4px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .time {
    color: #484f58;
    font-family: monospace;
    font-size: 10px;
    flex-shrink: 0;
    margin-left: auto;
  }

  /* Inject / Injection: chat bubble */
  .event-bubble {
    background: #161b22;
    border-left: 3px solid #58a6ff;
    padding: 8px 12px;
    border-radius: 0 8px 8px 0;
  }

  .bubble-header {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 4px;
  }

  .bubble-from {
    font-size: 11px;
    font-weight: 600;
  }

  .bubble-arrow {
    color: #484f58;
    font-size: 11px;
  }

  .bubble-to {
    font-size: 11px;
    color: #8b949e;
  }

  .bubble-body {
    font-size: 12px;
    color: #cdd9e5;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
  }

  /* Message routed */
  .event-routed {
    background: #161b22;
    border-left: 3px solid #d2a8ff;
    padding: 8px 12px;
    border-radius: 0 8px 8px 0;
  }

  .routed-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .routed-from {
    font-size: 11px;
    font-weight: 600;
  }

  .routed-arrow {
    font-size: 13px;
    font-weight: bold;
  }

  .routed-to {
    font-size: 11px;
    color: #8b949e;
  }

  .routed-body {
    margin-top: 4px;
    font-size: 12px;
    color: #cdd9e5;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
  }

  /* Synthesis: highlighted card */
  .event-synthesis {
    background: #1c1a14;
    border: 1px solid #3d2b1a;
    border-left: 3px solid #f0883e;
    padding: 8px 12px;
    border-radius: 0 8px 8px 0;
  }

  .synthesis-label {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    margin-bottom: 4px;
  }

  .synthesis-body {
    font-size: 12px;
    color: #e6cba0;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
  }

  .synthesis-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 4px;
    font-size: 10px;
    color: #8b949e;
  }

  /* System messages: centered, italic */
  .event-system {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 2px 0;
  }

  .system-text {
    font-size: 11px;
    color: #484f58;
    font-style: italic;
  }

  .system-time {
    margin-left: 0;
  }

  /* Expand button */
  .expand-btn {
    background: none;
    border: none;
    color: #58a6ff;
    font-size: 11px;
    cursor: pointer;
    padding: 2px 0;
    margin-top: 2px;
    display: block;
  }

  .expand-btn:hover {
    text-decoration: underline;
  }

  .empty-events {
    color: #484f58;
    font-size: 12px;
    padding: 24px 0;
    text-align: center;
    font-style: italic;
  }
</style>
