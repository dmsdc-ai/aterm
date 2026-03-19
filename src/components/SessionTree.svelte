<script>
  import { sessionTree, selectedSessionId, sessions } from '../lib/stores.js';

  function selectSession(id) {
    selectedSessionId.set(id);
  }

  function statusIcon(s) {
    if (!s.active_clients) return '✕';
    if (s.idleSeconds > 120) return '○';
    if (s.idleSeconds < 10) return '●';
    return '●';
  }

  function statusColor(s) {
    if (!s.active_clients) return '#f85149';
    if (s.idleSeconds > 120) return '#d29922';
    if (s.idleSeconds < 10) return '#3fb950';
    return '#8b949e';
  }

  function shortName(id) {
    return id.replace(/^aigentry-/, '').replace(/-claude$/, '');
  }

  function sessionHost(s) {
    const host = s.machine || s.host;
    if (!host || host === 'localhost' || host === '127.0.0.1') return null;
    return host;
  }
</script>

<div class="tree">
  <div class="tree-header">Sessions ({$sessions.length})</div>
  {#each Object.entries($sessionTree) as [project, items]}
    <div class="project">{project}</div>
    {#each items as s}
      <button
        class="session"
        class:selected={$selectedSessionId === s.id}
        on:click={() => selectSession(s.id)}
      >
        <span class="icon" style="color: {statusColor(s)}">{statusIcon(s)}</span>
        <span class="name">{shortName(s.id)}</span>
        {#if sessionHost(s)}
          <span class="remote-badge">@{sessionHost(s)}</span>
        {/if}
        <span class="clients">{s.active_clients}</span>
      </button>
    {/each}
  {/each}
</div>

<style>
  .tree { padding: 8px 0; }
  .tree-header { padding: 8px 16px; font-size: 11px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; }
  .project { padding: 6px 16px; font-size: 11px; color: #58a6ff; font-weight: 600; margin-top: 4px; }
  .session {
    display: flex; align-items: center; gap: 8px; width: 100%;
    padding: 4px 16px 4px 24px; border: none; background: none;
    color: #e6edf3; font-size: 13px; cursor: pointer; text-align: left;
  }
  .session:hover { background: #161b22; }
  .session.selected { background: #1f6feb33; }
  .icon { font-size: 10px; flex-shrink: 0; }
  .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .clients { font-size: 10px; color: #484f58; }
  .remote-badge { font-size: 10px; color: #d2a8ff; flex-shrink: 0; }
</style>
