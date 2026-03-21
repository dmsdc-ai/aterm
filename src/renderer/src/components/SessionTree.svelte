<script>
  import { workspaces, activeWorkspaceId, atermConnected, atermClient, refreshWorkspaces,
           sessions, selectedSessionId, sessionTree, client, connected } from '../lib/stores.js';
  import { onMount, onDestroy } from 'svelte';

  let refreshInterval;

  onMount(() => {
    refreshTeleptySessions();
    refreshInterval = setInterval(refreshTeleptySessions, 5000);
  });

  onDestroy(() => {
    clearInterval(refreshInterval);
  });

  async function refreshTeleptySessions() {
    let c;
    const unsub = client.subscribe(v => { c = v; });
    unsub();
    if (!c) return;
    try {
      const list = await c.getSessions();
      sessions.set(Array.isArray(list) ? list : (list.sessions || []));
      connected.set(true);
    } catch {
      connected.set(false);
    }
  }

  function selectSession(id) {
    selectedSessionId.set(id);
    activeWorkspaceId.set(null); // deselect workspace
  }

  function selectWorkspace(id) {
    activeWorkspaceId.set(id);
    selectedSessionId.set(null); // deselect session
  }

  function sessionName(s) {
    return s.id || s.name || 'unknown';
  }

  function sessionStatus(s) {
    if (s.status === 'active' || s.pid) return 'active';
    return 'idle';
  }

  async function createWorkspace() {
    let ac;
    const unsub = atermClient.subscribe(v => { ac = v; });
    unsub();
    if (!ac) return;
    try {
      const id = await ac.newWorkspace({});
      await refreshWorkspaces(ac);
      activeWorkspaceId.set(id);
    } catch (e) {
      console.warn('[SessionTree] createWorkspace failed:', e.message);
    }
  }

  async function closeWorkspace(id, e) {
    e.stopPropagation();
    let ac;
    const unsub = atermClient.subscribe(v => { ac = v; });
    unsub();
    if (!ac) return;
    try {
      await ac.closeWorkspace(id);
      await refreshWorkspaces(ac);
      // If closed workspace was active, deselect
      activeWorkspaceId.update(cur => cur === id ? null : cur);
    } catch (err) {
      console.warn('[SessionTree] closeWorkspace failed:', err.message);
    }
  }

  function statusColor(ws) {
    if (ws.status === 'dead') return '#f85149';
    return '#3fb950';
  }

  function shortId(id) {
    // "ws-lkj9az-abc1" → "lkj9az" or keep full if "default"
    if (id === 'default') return 'default';
    return id.replace(/^ws-/, '').replace(/-[^-]+$/, '');
  }

  function shortCwd(cwd) {
    if (!cwd) return '~';
    const home = '/Users/' + (cwd.split('/')[2] || '');
    return cwd.replace(home, '~');
  }
</script>

<div class="tree">
  <!-- Telepty Sessions Section -->
  <div class="tree-header">
    <div class="tree-header-left">
      <svg class="tree-header-icon" width="12" height="12" viewBox="0 0 12 12" fill="none">
        <circle cx="6" cy="6" r="4.5" stroke="currentColor" stroke-width="1"/>
        <path d="M6 3V6L8 8" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
      </svg>
      <span class="tree-header-label">Sessions</span>
    </div>
    <div class="tree-header-right">
      <span class="tree-header-count" class:offline={!$connected}>{$connected ? $sessions.length : '—'}</span>
    </div>
  </div>

  {#if $connected}
    {#each Object.entries($sessionTree) as [project, projectSessions]}
      <div class="project-group">
        <div class="project-label">{project}</div>
        {#each projectSessions as s}
          <div
            class="workspace-row"
            class:selected={$selectedSessionId === s.id}
            on:click={() => selectSession(s.id)}
            on:keydown={(e) => e.key === 'Enter' || e.key === ' ' ? selectSession(s.id) : null}
            role="button"
            tabindex="0"
          >
            {#if $selectedSessionId === s.id}
              <span class="selection-bar"></span>
            {/if}
            <div class="ws-icon" class:active={sessionStatus(s) === 'active'}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                <circle cx="7" cy="7" r="5" stroke="currentColor" stroke-width="1"/>
                <path d="M5 7L7 5L9 7" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
                <line x1="7" y1="5" x2="7" y2="10" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
              </svg>
              <span
                class="ws-status-dot"
                style="background: {sessionStatus(s) === 'active' ? '#3fb950' : '#d29922'}; box-shadow: 0 0 3px {sessionStatus(s) === 'active' ? '#3fb95088' : '#d2992288'};"
              ></span>
            </div>
            <div class="ws-info">
              <span class="ws-name">{sessionName(s)}</span>
              <span class="ws-cwd">{s.host || 'Local'}{s.command ? ' · ' + s.command : ''}</span>
            </div>
          </div>
        {/each}
      </div>
    {/each}
  {:else}
    <div class="offline-msg">telepty offline</div>
  {/if}

  <!-- Workspaces Section -->
  <div class="tree-header">
    <div class="tree-header-left">
      <svg class="tree-header-icon" width="12" height="12" viewBox="0 0 12 12" fill="none">
        <rect x="1" y="1" width="4" height="4" rx="1" fill="currentColor" opacity="0.4"/>
        <rect x="7" y="1" width="4" height="4" rx="1" fill="currentColor" opacity="0.4"/>
        <rect x="1" y="7" width="4" height="4" rx="1" fill="currentColor" opacity="0.4"/>
        <rect x="7" y="7" width="4" height="4" rx="1" fill="currentColor" opacity="0.2"/>
      </svg>
      <span class="tree-header-label">Workspaces</span>
    </div>
    <div class="tree-header-right">
      <span class="tree-header-count">{$workspaces.length}</span>
      <button
        class="new-btn"
        on:click={createWorkspace}
        title="New workspace"
        disabled={!$atermConnected}
      >+</button>
    </div>
  </div>

  {#each $workspaces as ws}
    <div
      class="workspace-row"
      class:selected={$activeWorkspaceId === ws.id}
      on:click={() => selectWorkspace(ws.id)}
      on:keydown={(e) => e.key === 'Enter' || e.key === ' ' ? selectWorkspace(ws.id) : null}
      role="button"
      tabindex="0"
    >
      {#if $activeWorkspaceId === ws.id}
        <span class="selection-bar"></span>
      {/if}

      <div class="ws-icon" class:active={ws.status !== 'dead'} class:dead={ws.status === 'dead'}>
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <rect x="1" y="2" width="12" height="10" rx="1.5" stroke="currentColor" stroke-width="1"/>
          <path d="M3 6L5 8L3 10" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
          <line x1="6.5" y1="10" x2="10.5" y2="10" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
        </svg>
        <span
          class="ws-status-dot"
          style="background: {statusColor(ws)}; box-shadow: 0 0 3px {statusColor(ws)}88;"
        ></span>
      </div>

      <div class="ws-info">
        <span class="ws-name">{shortId(ws.id)}</span>
        <span class="ws-cwd">{shortCwd(ws.cwd)}</span>
      </div>

      <button
        class="close-btn"
        on:click={(e) => closeWorkspace(ws.id, e)}
        title="Close workspace"
      >×</button>
    </div>
  {/each}

  {#if $workspaces.length === 0}
    <div class="empty-state">
      <div class="empty-illustration">
        <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
          <rect x="4" y="6" width="24" height="20" rx="3" stroke="currentColor" stroke-width="1.2"/>
          <path d="M4 12H28" stroke="currentColor" stroke-width="1.2"/>
          <circle cx="8" cy="9" r="1" fill="currentColor"/>
          <circle cx="11" cy="9" r="1" fill="currentColor"/>
          <circle cx="14" cy="9" r="1" fill="currentColor"/>
          <path d="M10 18L14 22L10 26" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" opacity="0.5"/>
        </svg>
      </div>
      <span class="empty-text">
        {$atermConnected ? 'No workspaces yet' : 'Connecting...'}
      </span>
      {#if $atermConnected}
        <button class="empty-new-btn" on:click={createWorkspace}>
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
            <path d="M5 1V9M1 5H9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
          Create workspace
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tree {
    padding: var(--space-3) 0 var(--space-5);
    font-family: var(--font-mono);
  }

  /* ── Header ── */
  .tree-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) 10px var(--space-4) 14px;
    border-bottom: 1px solid var(--color-border-subtle);
    margin-bottom: var(--space-2);
  }

  .tree-header-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tree-header-icon {
    color: var(--color-text-disabled);
  }

  .tree-header-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-tertiary);
  }

  .tree-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .tree-header-count {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-text-disabled);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: 8px;
    padding: 0 var(--space-3);
    line-height: 16px;
  }

  .new-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-md);
    color: var(--color-text-tertiary);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    transition: background var(--duration-instant) ease, color var(--duration-instant) ease, border-color var(--duration-instant) ease;
  }

  .new-btn:hover:not(:disabled) {
    background: var(--color-accent-blue-subtle);
    color: var(--color-accent-blue);
    border-color: var(--color-accent-blue-muted);
  }

  .new-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  /* ── Workspace row ── */
  .workspace-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: 100%;
    padding: 7px 10px 7px 18px;
    border: none;
    background: transparent;
    color: var(--color-text-tertiary);
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    transition: background var(--duration-normal) ease, color var(--duration-normal) ease;
    outline: none;
    user-select: none;
    animation: fadeIn var(--duration-normal) var(--ease-out) both;
  }

  .workspace-row:hover {
    background: var(--color-bg-elevated);
    color: var(--color-text-secondary);
    padding-left: 16px;
  }

  .workspace-row:hover .close-btn {
    opacity: 1;
  }

  .workspace-row.selected {
    background: var(--color-accent-blue-subtle);
    color: var(--color-text-primary);
  }

  .workspace-row.selected:hover {
    background: var(--color-accent-blue-muted);
  }

  .selection-bar {
    position: absolute;
    left: 0;
    top: 3px;
    bottom: 3px;
    width: 3px;
    background: var(--color-accent-blue);
    border-radius: 0 var(--radius-xs) var(--radius-xs) 0;
    transition: all var(--duration-normal) var(--ease-out);
  }

  .ws-icon {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-lg);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    color: var(--color-text-disabled);
    flex-shrink: 0;
    transition: background var(--duration-normal) var(--ease-default),
                color var(--duration-normal) var(--ease-default),
                border-color var(--duration-normal) var(--ease-default);
  }

  .ws-icon.active {
    color: var(--color-text-tertiary);
    border-color: var(--color-border-default);
  }

  .ws-icon.dead {
    color: var(--color-text-disabled);
    opacity: 0.6;
  }

  .workspace-row.selected .ws-icon {
    background: var(--color-accent-blue-subtle);
    color: var(--color-accent-blue);
    border-color: var(--color-accent-blue-muted);
  }

  .workspace-row:hover .ws-icon {
    border-color: var(--color-border-strong);
    color: var(--color-text-tertiary);
  }

  .ws-status-dot {
    position: absolute;
    bottom: -1px;
    right: -1px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    border: 1.5px solid var(--color-bg-base);
  }

  .ws-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .ws-name {
    font-size: 12px;
    line-height: 1.3;
    color: inherit;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-cwd {
    font-size: 10px;
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.2;
  }

  .close-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    background: none;
    border: none;
    color: var(--color-text-muted);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    border-radius: var(--radius-sm);
    opacity: 0;
    transition: opacity var(--duration-instant) ease, background var(--duration-instant) ease, color var(--duration-instant) ease;
  }

  .close-btn:hover {
    background: var(--color-danger-muted);
    color: var(--color-danger);
  }

  /* ── Empty state ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 40px 16px;
    color: var(--color-text-disabled);
    animation: fadeIn var(--duration-smooth) var(--ease-out);
  }

  .empty-illustration {
    color: var(--color-border-default);
    opacity: 0.4;
    animation: breathe 3s ease-in-out infinite;
  }

  @keyframes breathe {
    0%, 100% { opacity: 0.3; transform: scale(1); }
    50%      { opacity: 0.5; transform: scale(1.04); }
  }

  .empty-text {
    font-size: 11px;
    color: var(--color-text-disabled);
  }

  .empty-new-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-top: var(--space-2);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-lg);
    color: var(--color-text-tertiary);
    font-family: inherit;
    font-size: 11px;
    padding: 5px 12px;
    cursor: pointer;
    transition: background var(--duration-instant) ease, color var(--duration-instant) ease, border-color var(--duration-instant) ease;
  }

  .empty-new-btn:hover {
    background: var(--color-accent-blue-subtle);
    color: var(--color-accent-blue);
    border-color: var(--color-accent-blue-muted);
  }

  /* ── Telepty sessions ── */
  .project-group {
    margin-bottom: 4px;
  }

  .project-label {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-text-disabled);
    padding: 6px 14px 2px;
    text-transform: lowercase;
  }

  .offline-msg {
    font-size: 11px;
    color: var(--color-text-disabled);
    padding: 12px 14px;
    font-style: italic;
  }

  .tree-header-count.offline {
    color: var(--color-danger);
  }
</style>
