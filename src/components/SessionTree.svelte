<script>
  import { workspaces, activeWorkspaceId, atermConnected, atermClient, refreshWorkspaces,
           sessions, selectedSessionId, sessionTree, client, connected } from '../lib/stores.js';
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher();
  function onSettingsClick() { dispatch('settings'); }

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
    activeWorkspaceId.set(null);
  }

  function selectWorkspace(id) {
    activeWorkspaceId.set(id);
    selectedSessionId.set(null);
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
      activeWorkspaceId.update(cur => cur === id ? null : cur);
    } catch (err) {
      console.warn('[SessionTree] closeWorkspace failed:', err.message);
    }
  }

  function statusColor(ws) {
    if (ws.status === 'dead') return 'var(--status-danger)';
    return 'var(--status-active)';
  }

  function shortId(id) {
    if (id === 'default') return 'default';
    return id.replace(/^ws-/, '').replace(/-[^-]+$/, '');
  }

  function shortCwd(cwd) {
    if (!cwd) return '~';
    const home = '/Users/' + (cwd.split('/')[2] || '');
    return cwd.replace(home, '~');
  }

  // Map session name to Lucide SVG path(s) — 24x24 viewBox, stroke-width 1.5
  function sessionIconPaths(name) {
    const n = (name || '').toLowerCase();
    if (n === 'orchestrator') return 'orchestrator';
    if (n === 'amplify')      return 'amplify';
    if (n === 'brain')        return 'brain';
    if (n === 'deliberation') return 'deliberation';
    if (n === 'devkit')       return 'devkit';
    if (n === 'dustcraw')     return 'dustcraw';
    if (n === 'registry')     return 'registry';
    if (n === 'ssot')         return 'ssot';
    if (n === 'telepty')      return 'telepty';
    if (n === 'aterm')        return 'terminal';
    return 'terminal';
  }
</script>

<div class="tree">

  <!-- ── SESSIONS section ── -->
  <div class="section-label">Sessions</div>

  {#if $connected}
    {#each Object.entries($sessionTree) as [project, projectSessions]}
      <div class="group-label">{project}</div>
      {#each projectSessions as s}
        {@const name = sessionName(s)}
        {@const status = sessionStatus(s)}
        {@const icon = sessionIconPaths(name)}
        {@const isSelected = $selectedSessionId === s.id}
        <div
          class="session-row"
          class:selected={isSelected}
          class:state-running={status === 'active'}
          class:state-idle={status !== 'active'}
          on:click={() => selectSession(s.id)}
          on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectSession(s.id)}
          role="button"
          tabindex="0"
        >
          <span class="session-icon">
            {#if icon === 'orchestrator'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>
              </svg>
            {:else if icon === 'amplify'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="m3 11 18-5v12L3 13v-2z"/><path d="M11.6 16.8a3 3 0 1 1-5.8-1.6"/>
              </svg>
            {:else if icon === 'brain'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z"/>
                <path d="M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z"/>
                <path d="M12 5v13"/>
              </svg>
            {:else if icon === 'deliberation'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
              </svg>
            {:else if icon === 'devkit'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>
              </svg>
            {:else if icon === 'dustcraw'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="m8 2 1.88 1.88"/><path d="M14.12 3.88 16 2"/>
                <path d="M9 7.13v-1a3.003 3.003 0 1 1 6 0v1"/>
                <path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6"/>
                <path d="M12 20v-9"/><path d="M6.53 9C4.6 8.8 3 7.1 3 5"/>
                <path d="M6 13H2"/><path d="M3 21c0-2.1 1.7-3.9 3.8-4"/>
                <path d="M20.97 5c0 2.1-1.6 3.8-3.5 4"/><path d="M22 13h-4"/>
                <path d="M17.2 17c2.1.1 3.8 1.9 3.8 4"/>
              </svg>
            {:else if icon === 'registry'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M10 2v7.527a2 2 0 0 1-.211.896L4.72 20.55a1 1 0 0 0 .9 1.45h12.76a1 1 0 0 0 .9-1.45l-5.069-10.127A2 2 0 0 1 14 9.527V2"/>
                <path d="M8.5 2h7"/><path d="M7 16.5h10"/>
              </svg>
            {:else if icon === 'ssot'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <rect width="8" height="4" x="8" y="2" rx="1" ry="1"/>
                <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>
                <path d="m9 14 2 2 4-4"/>
              </svg>
            {:else if icon === 'telepty'}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>
              </svg>
            {:else}
              <!-- terminal (default) -->
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>
              </svg>
            {/if}
          </span>
          <div class="session-info">
            <span class="session-name">{name}</span>
            <span class="session-meta">{s.host || 'Local'}</span>
          </div>
        </div>
      {/each}
    {/each}
  {:else}
    <div class="offline-msg">telepty offline</div>
  {/if}

  <div class="section-spacer"></div>

  <!-- ── WORKSPACES section ── -->
  <div class="section-header-row">
    <span class="section-label" style="padding: 0; margin-bottom: 0;">Workspaces</span>
    <button
      class="add-btn"
      on:click={createWorkspace}
      title="New workspace"
      disabled={!$atermConnected}
      aria-label="Create workspace"
    >+</button>
  </div>

  {#each $workspaces as ws}
    <div
      class="workspace-row"
      class:selected={$activeWorkspaceId === ws.id}
      on:click={() => selectWorkspace(ws.id)}
      on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectWorkspace(ws.id)}
      role="button"
      tabindex="0"
    >
      <span class="workspace-icon">
        <!-- terminal icon -->
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>
        </svg>
      </span>
      <div class="workspace-info">
        <span class="workspace-name">{shortId(ws.id)}</span>
        <span class="workspace-meta">{ws.status === 'dead' ? 'Dead' : 'Active'}</span>
      </div>
      <button
        class="close-btn"
        on:click={(e) => closeWorkspace(ws.id, e)}
        title="Close workspace"
      >×</button>
    </div>
  {/each}

  {#if $workspaces.length === 0 && $atermConnected}
    <div class="empty-state">
      <span class="empty-text">No workspaces yet</span>
      <button class="empty-new-btn" on:click={createWorkspace}>
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <path d="M5 1V9M1 5H9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        Create workspace
      </button>
    </div>
  {/if}

  <!-- Sidebar bottom -->
  <div style="flex: 1;"></div>
  <div class="sidebar-bottom">
    <div class="sidebar-mascot">
      <span class="mascot mascot-sm pulse">
        <span class="mascot-eye">·</span><span class="mascot-core">⣿</span><span class="mascot-eye">·</span>
      </span>
    </div>
    <button class="sidebar-settings-btn" on:click={onSettingsClick} type="button" title="Settings">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
        <circle cx="12" cy="12" r="3"/>
      </svg>
    </button>
  </div>

</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    padding-top: 16px;
    min-height: 100%;
  }

  /* ── Section labels ── */
  .section-label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 0 16px;
    margin-bottom: 8px;
    user-select: none;
    -webkit-user-select: none;
  }

  .group-label {
    font-family: var(--font-sans);
    font-size: 10px;
    font-weight: 400;
    color: var(--text-disabled);
    padding: 4px 16px;
    user-select: none;
    -webkit-user-select: none;
  }

  .section-spacer { height: 20px; }

  .section-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    margin-bottom: 8px;
    user-select: none;
    -webkit-user-select: none;
  }

  /* ── Session rows ── */
  .session-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    cursor: pointer;
    transition: background 150ms ease;
    position: relative;
    outline: none;
  }

  .session-row:hover { background: var(--bg-sidebar-hover); }
  .session-row:active {
    background: var(--bg-sidebar-selected);
    transform: scale(0.995);
  }

  .session-row.selected {
    background: var(--bg-sidebar-selected);
    border-left: 2px solid var(--accent);
    padding-left: 14px;
  }

  .session-row.selected:hover { background: var(--bg-sidebar-selected); }

  /* Icon states */
  .session-icon {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: opacity 150ms ease;
  }

  .session-icon :global(svg) { width: 16px; height: 16px; }

  .state-idle .session-icon  { color: var(--text-disabled); opacity: 0.5; }
  .state-running .session-icon { color: var(--text-secondary); }
  .session-row.selected.state-running .session-icon { color: var(--text-primary); }

  .session-info {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .session-name {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 400;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.4;
  }

  .session-row.selected .session-name { color: var(--text-primary); }

  .session-meta {
    font-family: var(--font-sans);
    font-size: 10px;
    color: var(--text-disabled);
    flex-shrink: 0;
    margin-left: auto;
  }

  /* ── Workspace rows ── */
  .workspace-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    cursor: pointer;
    transition: background 150ms ease;
    position: relative;
    outline: none;
  }

  .workspace-row:hover { background: var(--bg-sidebar-hover); }
  .workspace-row:active {
    background: var(--bg-sidebar-selected);
    transform: scale(0.995);
  }

  .workspace-row.selected {
    background: var(--bg-sidebar-selected);
    border-left: 2px solid var(--accent);
    padding-left: 14px;
  }

  .workspace-row:hover .close-btn { opacity: 1; }

  .workspace-icon {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-tertiary);
  }

  .workspace-icon :global(svg) { width: 16px; height: 16px; }

  .workspace-info {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .workspace-name {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.4;
  }

  .workspace-row.selected .workspace-name { color: var(--text-primary); }

  .workspace-meta {
    font-family: var(--font-sans);
    font-size: 10px;
    color: var(--text-disabled);
    flex-shrink: 0;
    margin-left: auto;
  }

  /* ── Add button ── */
  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: var(--bg-button);
    border: 1px solid var(--bg-button-border);
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    transition: background 150ms ease, color 150ms ease;
  }

  .add-btn:hover:not(:disabled) {
    background: var(--bg-button-hover);
    color: var(--text-secondary);
  }

  .add-btn:active:not(:disabled) {
    background: var(--accent-subtle);
    color: var(--accent);
    transform: scale(0.9);
  }

  .add-btn:disabled { opacity: 0.3; cursor: default; }

  /* ── Close button ── */
  .close-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    border-radius: var(--radius-sm);
    opacity: 0;
    transition: opacity 150ms ease, background 150ms ease, color 150ms ease;
  }

  .close-btn:hover {
    background: rgba(217, 108, 108, 0.15);
    color: var(--status-danger);
  }

  /* ── Empty state ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 32px 16px;
    color: var(--text-disabled);
  }

  .empty-text {
    font-size: 11px;
    color: var(--text-disabled);
  }

  .empty-new-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    background: var(--bg-button);
    border: 1px solid var(--bg-button-border);
    border-radius: var(--radius-pill);
    color: var(--text-tertiary);
    font-family: var(--font-sans);
    font-size: 11px;
    padding: 5px 12px;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
  }

  .empty-new-btn:hover {
    background: var(--accent-subtle);
    color: var(--accent);
    border-color: var(--accent-muted);
  }

  /* ── Telepty offline ── */
  .offline-msg {
    font-size: 11px;
    color: var(--text-disabled);
    padding: 12px 16px;
    font-style: italic;
  }

  /* ── Bottom ── */
  .sidebar-bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    flex-shrink: 0;
    border-top: 1px solid var(--border-subtle);
  }

  .sidebar-mascot {
    text-align: center;
  }

  .sidebar-settings-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: var(--radius-md, 8px);
    cursor: pointer;
    color: var(--text-disabled);
    transition: background 150ms ease, color 150ms ease;
  }

  .sidebar-settings-btn:hover {
    background: var(--bg-sidebar-hover);
    color: var(--text-secondary);
  }

  .sidebar-settings-btn:active {
    transform: scale(0.92);
  }
</style>
