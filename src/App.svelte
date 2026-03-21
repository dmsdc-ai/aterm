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
        <span class="logo-bolt">⚡</span>aterm
      </span>
    </div>

    <div class="header-center"></div>

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
            <p>Select a workspace or press <kbd>+</kbd> to create one</p>
          {:else}
            <p>Connecting to aterm server...</p>
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
    height: var(--header-height);
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

  .header-center {
    flex: 1;
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

  .logo-bolt {
    font-size: 14px;
    line-height: 1;
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
</style>
