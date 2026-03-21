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

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background var(--duration-slow) ease, box-shadow var(--duration-slow) ease;
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
    gap: var(--space-3);
    padding: 32px 16px;
    color: var(--color-text-disabled);
    animation: fadeIn var(--duration-smooth) var(--ease-out);
  }

  .empty-icon {
    font-size: 20px;
    opacity: 0.4;
  }

  .empty-text {
    font-size: 11px;
    color: var(--color-text-disabled);
  }

  .empty-new-btn {
    margin-top: var(--space-2);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-lg);
    color: var(--color-text-tertiary);
    font-family: inherit;
    font-size: 11px;
    padding: var(--space-2) 12px;
    cursor: pointer;
    transition: background var(--duration-instant) ease, color var(--duration-instant) ease;
  }

  .empty-new-btn:hover {
    background: var(--color-accent-blue-subtle);
    color: var(--color-accent-blue);
    border-color: var(--color-accent-blue-muted);
  }
</style>
