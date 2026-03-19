<script>
  import { onMount, onDestroy } from 'svelte';
  import { TeleptClient } from './lib/telepty-client.js';
  import { client, sessions, connected, addBusEvent, selectedSessionId } from './lib/stores.js';
  import SessionTree from './components/SessionTree.svelte';
  import Timeline from './components/Timeline.svelte';
  import Terminal from './components/Terminal.svelte';
  import CommandPalette from './components/CommandPalette.svelte';

  let tc = null;
  let pollTimer = null;
  let showPalette = false;

  onMount(async () => {
    tc = new TeleptClient({ token: '' });
    try {
      const res = await fetch('http://localhost:3848/api/meta');
      if (res.ok) {
        const meta = await res.json();
        tc.token = meta.auth_token || '';
      }
    } catch {}

    client.set(tc);

    try {
      const list = await tc.getSessions();
      sessions.set(list);
      connected.set(true);
    } catch {
      connected.set(false);
    }

    pollTimer = setInterval(async () => {
      try {
        const list = await tc.getSessions();
        sessions.set(list);
        connected.set(true);
      } catch {
        connected.set(false);
      }
    }, 3000);

    tc.connectBus((event) => addBusEvent(event));
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
    if (tc) tc.destroy();
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
    <span class="logo">⚡ aterm</span>
    <span class="status" class:online={$connected} class:offline={!$connected}>
      {$connected ? 'Connected' : 'Disconnected'}
    </span>
    <button class="palette-btn" on:click={() => showPalette = true}>⌘K</button>
  </header>

  <div class="panels">
    <aside class="sidebar">
      <SessionTree />
    </aside>

    <main class="center">
      {#if $selectedSessionId}
        <Terminal sessionId={$selectedSessionId} />
      {:else}
        <div class="empty">
          <p>Select a session or press <kbd>⌘K</kbd></p>
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
    display: flex; align-items: center; gap: 12px;
    padding: 8px 16px; background: #161b22;
    border-bottom: 1px solid #30363d; font-size: 13px;
  }
  .logo { font-weight: 700; font-size: 15px; }
  .status { font-size: 11px; padding: 2px 8px; border-radius: 10px; }
  .online { background: #1a4d2e; color: #3fb950; }
  .offline { background: #4d1a1a; color: #f85149; }
  .palette-btn {
    margin-left: auto; background: #21262d; border: 1px solid #30363d;
    color: #8b949e; padding: 4px 10px; border-radius: 6px; cursor: pointer; font-size: 12px;
  }
  .palette-btn:hover { background: #30363d; }
  .panels { display: flex; flex: 1; overflow: hidden; }
  .sidebar { width: 240px; border-right: 1px solid #30363d; overflow-y: auto; }
  .center { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
  .inspector { width: 280px; border-left: 1px solid #30363d; overflow-y: auto; }
  .empty { display: flex; align-items: center; justify-content: center; height: 100%; color: #484f58; }
  kbd { background: #21262d; border: 1px solid #30363d; border-radius: 4px; padding: 2px 6px; font-size: 12px; }
</style>
