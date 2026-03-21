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
    background: #0d1117;
    color: #e6edf3;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  }
  .app { display: flex; flex-direction: column; height: 100vh; }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 40px;
    padding: 0 16px;
    background: linear-gradient(180deg, #1c2128 0%, #161b22 100%);
    border-bottom: 1px solid #30363d;
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
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: 13px;
    font-weight: 600;
    color: #e6edf3;
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
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: 11px;
    font-weight: 500;
    padding: 3px 9px 3px 7px;
    border-radius: 20px;
    letter-spacing: 0.01em;
    transition: background 200ms ease, color 200ms ease;
  }

  .status-dot-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background 200ms ease, box-shadow 200ms ease;
  }

  .online {
    background: rgba(63, 185, 80, 0.1);
    color: #3fb950;
    border: 1px solid rgba(63, 185, 80, 0.2);
  }

  .online .status-dot-indicator {
    background: #3fb950;
    box-shadow: 0 0 5px rgba(63, 185, 80, 0.6);
  }

  .offline {
    background: rgba(248, 81, 73, 0.1);
    color: #f85149;
    border: 1px solid rgba(248, 81, 73, 0.2);
  }

  .offline .status-dot-indicator {
    background: #f85149;
    box-shadow: 0 0 5px rgba(248, 81, 73, 0.5);
  }

  .palette-btn {
    display: flex;
    align-items: center;
    background: #21262d;
    border: 1px solid #30363d;
    border-bottom-color: #484f58;
    color: #8b949e;
    padding: 3px 9px;
    border-radius: 6px;
    cursor: pointer;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }

  .palette-btn:hover {
    background: #30363d;
    color: #c9d1d9;
    border-color: #484f58;
  }

  .palette-key {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .panels { display: flex; flex: 1; overflow: hidden; }
  .sidebar { width: 240px; border-right: 1px solid #30363d; overflow-y: auto; }
  .center { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
  .inspector { width: 280px; border-left: 1px solid #30363d; overflow-y: auto; }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: #484f58;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: 13px;
  }
  kbd {
    background: #21262d;
    border: 1px solid #30363d;
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 12px;
    color: #8b949e;
  }
</style>
