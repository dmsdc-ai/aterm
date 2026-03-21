<script>
  import { workspaces, activeWorkspaceId, atermConnected, atermClient, refreshWorkspaces } from '../lib/stores.js';

  function selectWorkspace(id) {
    activeWorkspaceId.set(id);
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
  <div class="tree-header">
    <span class="tree-header-label">Workspaces</span>
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

      <span
        class="status-dot"
        style="background: {statusColor(ws)}; box-shadow: 0 0 4px {statusColor(ws)}88;"
        title={ws.status}
      ></span>

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
      <span class="empty-icon">⚡</span>
      <span class="empty-text">
        {$atermConnected ? 'No workspaces' : 'Connecting...'}
      </span>
      {#if $atermConnected}
        <button class="empty-new-btn" on:click={createWorkspace}>New workspace</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tree {
    padding: 6px 0 12px;
    font-family: 'JetBrains Mono', 'Cascadia Code', 'Fira Code', ui-monospace, monospace;
  }

  /* ── Header ── */
  .tree-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px 8px 14px;
    border-bottom: 1px solid #21262d;
    margin-bottom: 4px;
  }

  .tree-header-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #8b949e;
  }

  .tree-header-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tree-header-count {
    font-size: 10px;
    font-weight: 500;
    color: #484f58;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 0 6px;
    line-height: 16px;
  }

  .new-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 4px;
    color: #8b949e;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    transition: background 80ms ease, color 80ms ease, border-color 80ms ease;
  }

  .new-btn:hover:not(:disabled) {
    background: #388bfd22;
    color: #58a6ff;
    border-color: #388bfd44;
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
    gap: 8px;
    width: 100%;
    padding: 7px 10px 7px 18px;
    border: none;
    background: transparent;
    color: #8b949e;
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s ease, color 0.15s ease;
    outline: none;
    user-select: none;
  }

  .workspace-row:hover {
    background: #161b22;
    color: #c9d1d9;
  }

  .workspace-row:hover .close-btn {
    opacity: 1;
  }

  .workspace-row.selected {
    background: rgba(88, 166, 255, 0.08);
    color: #e6edf3;
  }

  .workspace-row.selected:hover {
    background: rgba(88, 166, 255, 0.11);
  }

  .selection-bar {
    position: absolute;
    left: 0;
    top: 3px;
    bottom: 3px;
    width: 3px;
    background: #58a6ff;
    border-radius: 0 2px 2px 0;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background 300ms ease, box-shadow 300ms ease;
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
    color: #6e7681;
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
    color: #6e7681;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    border-radius: 3px;
    opacity: 0;
    transition: opacity 80ms ease, background 80ms ease, color 80ms ease;
  }

  .close-btn:hover {
    background: rgba(248, 81, 73, 0.15);
    color: #f85149;
  }

  /* ── Empty state ── */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 32px 16px;
    color: #484f58;
  }

  .empty-icon {
    font-size: 20px;
    opacity: 0.4;
  }

  .empty-text {
    font-size: 11px;
    color: #484f58;
  }

  .empty-new-btn {
    margin-top: 4px;
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #8b949e;
    font-family: inherit;
    font-size: 11px;
    padding: 4px 12px;
    cursor: pointer;
    transition: background 80ms ease, color 80ms ease;
  }

  .empty-new-btn:hover {
    background: #388bfd22;
    color: #58a6ff;
    border-color: #388bfd44;
  }
</style>
