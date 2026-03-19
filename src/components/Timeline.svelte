<script>
  import { busEvents, selectedSession } from '../lib/stores.js';

  function formatTime(ts) {
    return new Date(ts).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function eventLabel(e) {
    if (e.type === 'inject_written') return `inject → ${e.target || '?'}`;
    if (e.type === 'injection') return `broadcast → ${e.target_agent || 'all'}`;
    if (e.type === 'message_routed') return `${e.from || '?'} → ${e.to || '?'}`;
    if (e.type === 'session_register') return `+ ${e.session_id}`;
    return e.type || 'event';
  }

  function eventColor(e) {
    if (e.type === 'inject_written') return '#58a6ff';
    if (e.type === 'injection') return '#3fb950';
    if (e.type === 'message_routed') return '#d2a8ff';
    return '#8b949e';
  }
</script>

<div class="timeline">
  <div class="panel-header">Events</div>

  {#if $selectedSession}
    <div class="session-info">
      <div class="info-row"><span class="label">ID</span><span>{$selectedSession.id}</span></div>
      <div class="info-row"><span class="label">Command</span><span>{$selectedSession.command}</span></div>
      <div class="info-row"><span class="label">CWD</span><span class="mono">{$selectedSession.cwd}</span></div>
      <div class="info-row"><span class="label">Clients</span><span>{$selectedSession.active_clients}</span></div>
      <div class="info-row"><span class="label">Idle</span><span>{$selectedSession.idleSeconds}s</span></div>
    </div>
    <div class="divider"></div>
  {/if}

  <div class="events">
    {#each [...$busEvents].reverse().slice(0, 50) as event}
      <div class="event">
        <span class="time">{formatTime(event._ts)}</span>
        <span class="desc" style="color: {eventColor(event)}">{eventLabel(event)}</span>
      </div>
    {/each}
    {#if $busEvents.length === 0}
      <div class="empty-events">Waiting for events...</div>
    {/if}
  </div>
</div>

<style>
  .timeline { padding: 8px 0; height: 100%; display: flex; flex-direction: column; }
  .panel-header { padding: 8px 16px; font-size: 11px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; }
  .session-info { padding: 4px 16px; }
  .info-row { display: flex; justify-content: space-between; font-size: 12px; padding: 2px 0; }
  .label { color: #8b949e; }
  .mono { font-family: monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; max-width: 160px; }
  .divider { border-top: 1px solid #21262d; margin: 8px 16px; }
  .events { flex: 1; overflow-y: auto; padding: 0 16px; }
  .event { display: flex; gap: 8px; font-size: 12px; padding: 3px 0; border-bottom: 1px solid #161b22; }
  .time { color: #484f58; font-family: monospace; font-size: 11px; flex-shrink: 0; }
  .desc { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty-events { color: #484f58; font-size: 12px; padding: 16px 0; text-align: center; }
</style>
