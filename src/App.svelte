<script>
  import { onMount, onDestroy } from 'svelte';
  import { AtermClient } from './lib/aterm-client.js';
  import {
    atermClient, atermConnected, workspaces, activeWorkspaceId,
    refreshWorkspaces, addBusEvent,
  } from './lib/stores.js';
  import SessionTree from './components/SessionTree.svelte';
  import Timeline from './components/Timeline.svelte';
  import Terminal from './components/Terminal.svelte';
  import CommandPalette from './components/CommandPalette.svelte';

  let ac = null;
  let showPalette = false;
  let refreshTimer = null;

  onMount(() => {
    ac = new AtermClient();
    atermClient.set(ac);

    ac.on('connected', async () => {
      atermConnected.set(true);
      await refreshWorkspaces(ac);
      // Auto-select first workspace if none selected
      activeWorkspaceId.update(cur => {
        if (cur) return cur;
        let list;
        const unsub = workspaces.subscribe(v => { list = v; });
        unsub();
        return list?.[0]?.id ?? null;
      });
    });

    ac.on('disconnected', () => {
      atermConnected.set(false);
    });

    // Workspace lifecycle events from server
    ac.on('created', async () => {
      await refreshWorkspaces(ac);
    });
    ac.on('closed', async () => {
      await refreshWorkspaces(ac);
    });

    // Poll workspace list every 5s to catch external changes
    refreshTimer = setInterval(() => refreshWorkspaces(ac), 5000);
  });

  onDestroy(() => {
    if (refreshTimer) clearInterval(refreshTimer);
    if (ac) ac.destroy();
  });

  function handleKeydown(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      showPalette = !showPalette;
    }
    if (e.key === 'Escape') showPalette = false;
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="app">
  <header class="header">
    <div class="header-left">
      <span class="logo">
        <svg class="logo-icon" width="16" height="16" viewBox="0 0 16 16" fill="none">
          <rect x="1" y="2" width="14" height="12" rx="2" stroke="currentColor" stroke-width="1.2"/>
          <path d="M4 7L6.5 9.5L4 12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
          <line x1="8" y1="12" x2="12" y2="12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        <span class="logo-text">aterm</span>
        <span class="logo-version">v1</span>
      </span>
    </div>

    <div class="header-center">
      {#if $activeWorkspaceId}
        <div class="breadcrumb">
          <span class="breadcrumb-icon">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <rect x="1" y="1.5" width="10" height="9" rx="1.5" stroke="currentColor" stroke-width="1"/>
              <path d="M1 4.5H11" stroke="currentColor" stroke-width="1"/>
            </svg>
          </span>
          <span class="breadcrumb-text">{$activeWorkspaceId === 'default' ? 'default' : $activeWorkspaceId.replace(/^ws-/, '').replace(/-[^-]+$/, '')}</span>
        </div>
      {/if}
    </div>

    <div class="header-right">
      <span class="status" class:online={$atermConnected} class:offline={!$atermConnected}>
        <span class="status-dot-indicator"></span>
        {$atermConnected ? 'Connected' : 'Disconnected'}
      </span>
      <button class="palette-btn" on:click={() => showPalette = true}>
        <span class="palette-key">⌘K</span>
      </button>
    </div>
  </header>

  <div class="panels">
    <aside class="sidebar">
      <SessionTree />
    </aside>

    <main class="center">
      {#if $activeWorkspaceId}
        <Terminal workspaceId={$activeWorkspaceId} />
      {:else}
        <div class="empty">
          {#if $atermConnected}
            <div class="empty-visual">
              <svg class="empty-terminal-icon" width="48" height="48" viewBox="0 0 48 48" fill="none">
                <rect x="4" y="8" width="40" height="32" rx="4" stroke="currentColor" stroke-width="1.5"/>
                <path d="M4 16H44" stroke="currentColor" stroke-width="1.5"/>
                <circle cx="10" cy="12" r="1.5" fill="currentColor"/>
                <circle cx="15" cy="12" r="1.5" fill="currentColor"/>
                <circle cx="20" cy="12" r="1.5" fill="currentColor"/>
                <path d="M12 24L18 30L12 36" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                <line x1="22" y1="36" x2="34" y2="36" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
              </svg>
              <p class="empty-title">No workspace selected</p>
              <p class="empty-hint">Select a workspace or press <kbd>+</kbd> to create one</p>
            </div>
          {:else}
            <div class="empty-visual">
              <div class="connecting-dots">
                <span class="dot"></span><span class="dot"></span><span class="dot"></span>
              </div>
              <p class="empty-title">Connecting to aterm...</p>
            </div>
          {/if}
        </div>
      {/if}
    </main>

    <aside class="inspector">
      <Timeline />
    </aside>
  </div>

  {#if showPalette}
    <CommandPalette on:close={() => showPalette = false} />
  {/if}
</div>

<style>
  :global(body) {
    margin: 0;
    background: var(--color-bg-base);
    color: var(--color-text-primary);
    font-family: var(--font-sans);
  }
  .app { display: flex; flex-direction: column; height: 100vh; }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 44px;
    padding: 0 16px;
    background: linear-gradient(180deg, var(--color-header-gradient-start) 0%, var(--color-header-gradient-end) 100%);
    border-bottom: 1px solid var(--color-border-default);
    box-shadow: 0 1px 0 rgba(88, 166, 255, 0.05);
    flex-shrink: 0;
    user-select: none;
  }

  .header-left {
    display: flex;
    align-items: center;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 5px;
    font-family: var(--font-sans);
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-primary);
    letter-spacing: -0.01em;
  }

  .logo-icon {
    color: var(--color-accent-blue);
    flex-shrink: 0;
  }

  .logo-text {
    background: linear-gradient(135deg, var(--color-text-primary) 0%, var(--color-accent-blue) 100%);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .logo-version {
    font-size: 9px;
    font-weight: 500;
    color: var(--color-text-disabled);
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-sm);
    padding: 0 4px;
    line-height: 14px;
    letter-spacing: 0.02em;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 5px;
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    padding: 3px 9px 3px 7px;
    border-radius: var(--radius-pill);
    letter-spacing: 0.01em;
    transition: background var(--duration-smooth) ease, color var(--duration-smooth) ease;
  }

  .status-dot-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background var(--duration-smooth) ease, box-shadow var(--duration-smooth) ease;
  }

  .online {
    background: var(--color-success-subtle);
    color: var(--color-success);
    border: 1px solid var(--color-success-muted);
  }

  .online .status-dot-indicator {
    background: var(--color-success);
    box-shadow: var(--shadow-glow-green);
    animation: statusPulse 2s ease-in-out infinite;
  }

  @keyframes statusPulse {
    0%, 100% { box-shadow: 0 0 4px rgba(63, 185, 80, 0.4); }
    50%      { box-shadow: 0 0 8px rgba(63, 185, 80, 0.7); }
  }

  .offline {
    background: var(--color-danger-subtle);
    color: var(--color-danger);
    border: 1px solid var(--color-danger-muted);
  }

  .offline .status-dot-indicator {
    background: var(--color-danger);
    box-shadow: var(--shadow-glow-red);
  }

  .palette-btn {
    display: flex;
    align-items: center;
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-bottom-color: var(--color-border-strong);
    color: var(--color-text-tertiary);
    padding: 3px 9px;
    border-radius: var(--radius-lg);
    cursor: pointer;
    font-family: var(--font-sans);
    transition: background var(--duration-fast) ease, color var(--duration-fast) ease, border-color var(--duration-fast) ease;
  }

  .palette-btn:hover {
    background: var(--color-border-default);
    color: var(--color-text-secondary);
    border-color: var(--color-border-strong);
  }

  .palette-key {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .panels { display: flex; flex: 1; overflow: hidden; }
  .sidebar { width: var(--sidebar-width); border-right: 1px solid var(--color-border-default); overflow-y: auto; }
  .center { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
  .inspector { width: var(--inspector-width); border-left: 1px solid var(--color-border-default); overflow-y: auto; }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--color-text-disabled);
    font-family: var(--font-sans);
    font-size: 13px;
  }
  kbd {
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-md);
    padding: 2px 6px;
    font-size: 12px;
    color: var(--color-text-tertiary);
  }

  .header-center {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-lg);
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--color-text-tertiary);
    max-width: 300px;
  }

  .breadcrumb-icon {
    display: flex;
    align-items: center;
    color: var(--color-text-disabled);
    flex-shrink: 0;
  }

  .breadcrumb-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty-visual {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .empty-terminal-icon {
    color: var(--color-border-default);
    opacity: 0.5;
  }

  .empty-title {
    margin: 0;
    font-family: var(--font-sans);
    font-size: 14px;
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .empty-hint {
    margin: 0;
    font-family: var(--font-sans);
    font-size: 12px;
    color: var(--color-text-disabled);
  }

  .connecting-dots {
    display: flex;
    gap: 6px;
  }

  .connecting-dots .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-text-disabled);
    animation: dotBounce 1.4s ease-in-out infinite;
  }

  .connecting-dots .dot:nth-child(2) { animation-delay: 0.16s; }
  .connecting-dots .dot:nth-child(3) { animation-delay: 0.32s; }

  @keyframes dotBounce {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
    40% { opacity: 1; transform: scale(1.2); }
  }
</style>
