<script>
  import { workspaces, activeWorkspaceId, atermConnected, atermClient, refreshWorkspaces } from '../lib/stores.js';
  import { onDestroy, createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher();
  function onSettingsClick() { dispatch('settings'); }

  async function openFolderDialog() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const folders = await open({ directory: true, multiple: true, title: 'Select project folders' });
      if (!folders || folders.length === 0) return;

      const folderList = Array.isArray(folders) ? folders : [folders];
      let firstNewId = null;

      for (const folderPath of folderList) {
        const id = await createWorkspaceFromPath(folderPath);
        if (!firstNewId) firstNewId = id;
      }

      if (firstNewId) {
        activeWorkspaceId.set(firstNewId);
      }
    } catch (e) {
      console.warn('[SessionTree] folder dialog failed:', e.message);
      // Fallback: simple prompt for non-Tauri environments
      const path = window.prompt('Enter project path:');
      if (path) {
        const id = await createWorkspaceFromPath(path);
        if (id) activeWorkspaceId.set(id);
      }
    }
  }

  async function createWorkspaceFromPath(folderPath) {
    dispatch('create-workspace', { cwd: folderPath });
    // Return a predictable id based on path so we can auto-select
    // The actual id is assigned server-side; we'll just let App.svelte
    // refresh the list and pick the newest workspace.
    return null;
  }

  function selectWorkspace(id) {
    activeWorkspaceId.set(id);
  }

  async function closeWorkspace(id, e) {
    e.stopPropagation();
    let ac;
    const unsub = atermClient.subscribe(v => { ac = v; });
    unsub();
    if (!ac) return;
    try {
      await ac.closeWorkspace(id);
      let ac2;
      const unsub2 = atermClient.subscribe(v => { ac2 = v; });
      unsub2();
      await refreshWorkspaces(ac2);
      activeWorkspaceId.update(cur => cur === id ? null : cur);
    } catch (err) {
      console.warn('[SessionTree] closeWorkspace failed:', err.message);
    }
  }

  function statusColor(ws) {
    if (ws.status === 'dead') return 'var(--status-danger)';
    return 'var(--status-active)';
  }

  function sessionName(ws) {
    if (ws.cwd) {
      const parts = ws.cwd.replace(/\/+$/, '').split('/');
      return parts[parts.length - 1] || ws.id;
    }
    return ws.id.replace(/^ws-/, '').replace(/-[^-]+$/, '') || ws.id;
  }

  function shortCwd(cwd) {
    if (!cwd) return '~';
    const home = '/Users/' + (cwd.split('/')[2] || '');
    return cwd.replace(home, '~');
  }
</script>

<div class="tree">

  <!-- ── SESSIONS section ── -->
  <div class="section-header-row">
    <span class="section-label">Sessions</span>
    <button
      class="add-btn"
      on:click={openFolderDialog}
      title="New session"
      disabled={!$atermConnected}
      aria-label="Create session"
    >+</button>
  </div>

  {#each $workspaces as ws}
    <div
      class="session-row"
      class:selected={$activeWorkspaceId === ws.id}
      class:state-running={ws.status !== 'dead'}
      class:state-idle={ws.status === 'dead'}
      on:click={() => selectWorkspace(ws.id)}
      on:keydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectWorkspace(ws.id)}
      role="button"
      tabindex="0"
    >
      <span class="session-icon">
        <!-- terminal icon -->
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>
        </svg>
      </span>
      <div class="session-info">
        <span class="session-name">{sessionName(ws)}</span>
        <span class="session-meta">{ws.status === 'dead' ? 'dead' : 'active'}</span>
      </div>
      <button
        class="close-btn"
        on:click={(e) => closeWorkspace(ws.id, e)}
        title="Close session"
      >×</button>
    </div>
  {/each}

  {#if $workspaces.length === 0 && $atermConnected}
    <div class="empty-state">
      <span class="empty-text">No sessions yet</span>
      <button class="empty-new-btn" on:click={openFolderDialog}>
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <path d="M5 1V9M1 5H9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        New session
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

  /* ── Section label ── */
  .section-label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    user-select: none;
    -webkit-user-select: none;
  }

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

  .session-row:hover .close-btn { opacity: 1; }

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
