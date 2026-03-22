<script>
  import { onMount, onDestroy } from 'svelte';
  import Terminal from './Terminal.svelte';
  import { atermClient, pendingInjects, teleptySessions, workspaces, refreshWorkspaces } from '../lib/stores.js';
  import { teleptyAttachWorkspaceId, workspaceDisplayName } from '../lib/workspace-labels.js';

  export let sessionId;

  let mounted = false;
  let createInFlight = null;
  let ac = null;
  let session = null;
  let attachWorkspace = null;
  let attachWorkspaceId = teleptyAttachWorkspaceId('session');

  $: ac = $atermClient;
  $: session = $teleptySessions.find((item) => item.id === sessionId) ?? null;
  $: attachWorkspaceId = teleptyAttachWorkspaceId(session?.remoteId || (sessionId ? sessionId.replace(/^telepty:/, '') : 'session'));
  $: attachWorkspace = $workspaces.find((item) => item.id === attachWorkspaceId) ?? null;

  function remoteStatus() {
    if (!session?.status || session.status === 'discovered') return 'active';
    return session.status;
  }

  function statusColor() {
    if (!attachWorkspace) {
      if (createInFlight) return 'var(--accent)';
      return remoteStatus() === 'dead' ? 'var(--status-danger)' : 'var(--status-active)';
    }
    if (attachWorkspace.status === 'dead') return 'var(--status-danger)';
    if (attachWorkspace.status === 'restarting') return 'var(--accent)';
    return 'var(--status-active)';
  }

  function statusLabel() {
    if (!session) return 'missing';
    if (!attachWorkspace) return createInFlight ? 'connecting' : remoteStatus();
    if (attachWorkspace.status === 'dead') return 'dead';
    if (attachWorkspace.status === 'restarting') return 'restarting';
    return remoteStatus();
  }

  function remoteTarget() {
    if (!session) return sessionId.replace(/^telepty:/, '');
    return session.host && session.host !== 'Local'
      ? `${session.remoteId}@${session.host}`
      : session.remoteId;
  }

  async function ensureAttachWorkspace() {
    if (!ac || !session) return null;

    if (attachWorkspace && attachWorkspace.id === attachWorkspaceId && attachWorkspace.status !== 'dead') {
      return attachWorkspaceId;
    }
    if (createInFlight) {
      return createInFlight;
    }

    createInFlight = (async () => {
      if (attachWorkspace?.status === 'dead') {
        await ac.closeWorkspace(attachWorkspaceId).catch(() => {});
        await refreshWorkspaces(ac);
      }

      await ac.newWorkspace({
        id: attachWorkspaceId,
        cwd: session.cwd || '',
        command: 'telepty',
        args: ['attach', remoteTarget()],
        ephemeral: true,
      }).catch((error) => {
        const message = String(error?.message || error || '');
        if (!message.includes('already exists')) {
          throw error;
        }
      });

      await refreshWorkspaces(ac);
      return attachWorkspaceId;
    })().finally(() => {
      createInFlight = null;
    });

    return createInFlight;
  }

  async function bootstrap() {
    if (!mounted || !ac || !sessionId || !session) return;
    await ensureAttachWorkspace();
  }

  onMount(() => {
    mounted = true;
    bootstrap();
  });

  onDestroy(() => {
    mounted = false;
  });

  $: terminalKey = attachWorkspace
    ? `${attachWorkspace.id}:${attachWorkspace.created_at}:${attachWorkspace.status}`
    : attachWorkspaceId;

  $: if (mounted && sessionId && ac && session) {
    bootstrap();
  }
</script>

<div class="terminal-panel">
  <div class="session-header">
    <div class="session-identity">
      <span class="status-dot" style="background: {statusColor()}; box-shadow: 0 0 5px {statusColor()}88;"></span>
      <span class="session-name">{workspaceDisplayName(session || { id: sessionId, cwd: '', command: 'telepty' })}</span>
      {#if attachWorkspace?.id && $pendingInjects[attachWorkspace.id] > 0}
        <span class="inject-badge" title="{$pendingInjects[attachWorkspace.id]} pending inject(s)">
          {$pendingInjects[attachWorkspace.id]}
        </span>
      {/if}
    </div>
    <div class="session-meta">
      {#if attachWorkspace?.cwd || session?.cwd}
        <span class="session-cwd">{attachWorkspace?.cwd || session?.cwd}</span>
      {/if}
      <span class="session-status-label" style="color: {statusColor()};">{statusLabel()}</span>
    </div>
  </div>
  <div class="terminal-body">
    {#if attachWorkspace?.id && attachWorkspace.status !== 'dead'}
      {#key terminalKey}
        <Terminal workspaceId={attachWorkspace.id} embedded />
      {/key}
    {:else}
      <div class="terminal-empty">
        <span class="terminal-empty-copy">{session ? 'Connecting to telepty session...' : 'Telepty session unavailable'}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .terminal-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-inset);
  }

  .session-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 14px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
    user-select: none;
    -webkit-user-select: none;
  }

  .session-identity {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--status-active);
    box-shadow: 0 0 5px rgba(92, 185, 122, 0.53);
  }

  .session-name {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
  }

  .inject-badge {
    font-family: var(--font-mono);
    font-size: 9px;
    font-weight: 600;
    min-width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--accent, #d97706);
    color: #fff;
    border-radius: 8px;
    padding: 0 4px;
    line-height: 1;
    flex-shrink: 0;
  }

  .session-meta {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .session-cwd {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-disabled);
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-status-label {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--status-active);
  }

  .terminal-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    min-height: 0;
  }

  .terminal-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-inset, #131010);
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .terminal-empty-copy {
    opacity: 0.8;
  }
</style>
