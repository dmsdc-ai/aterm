<script>
  import { onMount, onDestroy } from 'svelte';
  import { AtermClient } from './lib/aterm-client.js';
  import { TeleptClient } from './lib/telepty-client.js';
  import {
    atermClient, atermConnected, workspaces, activeWorkspaceId,
    refreshWorkspaces, addBusEvent,
    client, sessions, teleptConnected, selectedSessionId, viewMode,
  } from './lib/stores.js';
  import SessionTree from './components/SessionTree.svelte';
  import Timeline from './components/Timeline.svelte';
  import Terminal from './components/Terminal.svelte';
  import CommandPalette from './components/CommandPalette.svelte';

  let ac = null;
  let tc = null;
  let showPalette = false;
  let refreshTimer = null;
  let theme = 'light';

  // Resize state
  const SIDEBAR_DEFAULT = 250;
  const SIDEBAR_MIN = 150;
  const SIDEBAR_MAX = 400;
  const TIMELINE_DEFAULT = 280;
  const TIMELINE_MIN = 200;
  const TIMELINE_MAX = 500;

  let sidebarWidth = SIDEBAR_DEFAULT;
  let timelineWidth = TIMELINE_DEFAULT;
  let draggingSidebar = false;
  let draggingTimeline = false;

  function startSidebarResize(e) {
    e.preventDefault();
    draggingSidebar = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    function onMouseMove(e) {
      const delta = e.clientX - startX;
      sidebarWidth = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + delta));
    }

    function onMouseUp() {
      draggingSidebar = false;
      localStorage.setItem('aterm-sidebar-width', String(sidebarWidth));
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    }

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  function startTimelineResize(e) {
    e.preventDefault();
    draggingTimeline = true;
    const startX = e.clientX;
    const startWidth = timelineWidth;

    function onMouseMove(e) {
      const delta = startX - e.clientX;
      timelineWidth = Math.min(TIMELINE_MAX, Math.max(TIMELINE_MIN, startWidth + delta));
    }

    function onMouseUp() {
      draggingTimeline = false;
      localStorage.setItem('aterm-timeline-width', String(timelineWidth));
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    }

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  function initTheme() {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    theme = prefersDark ? 'dark' : 'light';
    document.documentElement.setAttribute('data-theme', theme);
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', theme);
  }

  async function initTelepty() {
    try {
      tc = new TeleptClient();
      client.set(tc); // Set client immediately so Terminal can attach
      await tc.loadToken().catch(() => {}); // Token is optional
      try {
        const list = await tc.getSessions();
        sessions.set(Array.isArray(list) ? list : (list.sessions || []));
      } catch (e) {
        console.warn('[App] getSessions failed (CORS?), sessions will load from sidebar polling:', e.message);
      }
      teleptConnected.set(true);

      tc.connectBus(async (msg) => {
        addBusEvent(msg);
        if (msg.type === 'session_created' || msg.type === 'session_closed' ||
            msg.type === 'session_started' || msg.type === 'session_stopped' ||
            msg.event === 'created' || msg.event === 'closed') {
          try {
            const updated = await tc.getSessions();
            sessions.set(updated);
          } catch (e) {
            console.warn('[App] telepty session refresh failed:', e.message);
          }
        }
      });
    } catch (e) {
      console.warn('[App] telepty init failed (daemon may be offline):', e.message);
      teleptConnected.set(false);
    }
  }

  onMount(() => {
    initTheme();

    // Load persisted widths
    const savedSidebar = localStorage.getItem('aterm-sidebar-width');
    const savedTimeline = localStorage.getItem('aterm-timeline-width');
    if (savedSidebar) sidebarWidth = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Number(savedSidebar)));
    if (savedTimeline) timelineWidth = Math.min(TIMELINE_MAX, Math.max(TIMELINE_MIN, Number(savedTimeline)));


    ac = new AtermClient();
    atermClient.set(ac);

    ac.on('connected', async () => {
      atermConnected.set(true);
      await refreshWorkspaces(ac);
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

    ac.on('created', async () => { await refreshWorkspaces(ac); });
    ac.on('closed',  async () => { await refreshWorkspaces(ac); });

    refreshTimer = setInterval(() => refreshWorkspaces(ac), 5000);
    initTelepty();
  });

  onDestroy(() => {
    if (refreshTimer) clearInterval(refreshTimer);
    if (ac) ac.destroy();
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
  <!-- HEADER — 48px -->
  <header class="header">
    <div class="header-left">
      <span class="mascot mascot-sm">
        <span class="mascot-eye">·</span><span class="mascot-core">⣿</span><span class="mascot-eye">·</span>
      </span>
      <span class="header-title">aterm</span>
    </div>

    <div class="header-center"></div>

    <div class="header-right">
      <div class="connection-status">
        <span class="connection-dot" class:online={$atermConnected} class:offline={!$atermConnected}></span>
        <span class="connection-label">{$atermConnected ? 'Connected' : 'Disconnected'}</span>
      </div>
      <span class="header-sep"></span>
      <span class="shortcut-hint">&#8984;K</span>
      <span class="header-sep"></span>
      <button class="theme-toggle" on:click={toggleTheme} type="button" aria-label="Toggle theme">
        <!-- sun icon — shown in dark mode -->
        <svg class="icon-sun" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="7" cy="7" r="3"/>
          <line x1="7" y1="0.5" x2="7" y2="2"/>
          <line x1="7" y1="12" x2="7" y2="13.5"/>
          <line x1="0.5" y1="7" x2="2" y2="7"/>
          <line x1="12" y1="7" x2="13.5" y2="7"/>
          <line x1="2.4" y1="2.4" x2="3.5" y2="3.5"/>
          <line x1="10.5" y1="10.5" x2="11.6" y2="11.6"/>
          <line x1="11.6" y1="2.4" x2="10.5" y2="3.5"/>
          <line x1="3.5" y1="10.5" x2="2.4" y2="11.6"/>
        </svg>
        <!-- moon icon — shown in light mode -->
        <svg class="icon-moon" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12.5 7.8A5.5 5.5 0 1 1 6.2 1.5a4.5 4.5 0 0 0 6.3 6.3z"/>
        </svg>
      </button>
    </div>
  </header>

  <!-- MAIN 3-PANEL -->
  <div class="panels">
    <aside class="sidebar" style="width: {sidebarWidth}px">
      <SessionTree />
    </aside>

    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="resize-handle"
      class:dragging={draggingSidebar}
      role="separator"
      aria-label="Resize sidebar"
      on:mousedown={startSidebarResize}
    ></div>

    <main class="center">
      {#if $selectedSessionId}
        <Terminal workspaceId={$activeWorkspaceId} sessionId={$selectedSessionId} />
      {:else if $activeWorkspaceId}
        <Terminal workspaceId={$activeWorkspaceId} />
      {:else}
        <div class="empty">
          {#if $atermConnected}
            <div class="center-empty">
              <span class="mascot mascot-lg pulse">
                <span class="mascot-eye">·</span><span class="mascot-core">⣿</span><span class="mascot-eye">·</span>
              </span>
              <p class="empty-title">No workspace selected</p>
              <p class="empty-hint">Select a workspace or press <kbd>+</kbd> to create one</p>
            </div>
          {:else}
            <div class="center-empty">
              <div class="connecting-dots">
                <span class="dot"></span><span class="dot"></span><span class="dot"></span>
              </div>
              <p class="empty-title">Connecting to aterm...</p>
            </div>
          {/if}
        </div>
      {/if}
    </main>

    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="resize-handle"
      class:dragging={draggingTimeline}
      role="separator"
      aria-label="Resize timeline"
      on:mousedown={startTimelineResize}
    ></div>

    <aside class="inspector" style="width: {timelineWidth}px">
      <Timeline />
    </aside>
  </div>

  {#if showPalette}
    <CommandPalette on:close={() => showPalette = false} />
  {/if}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    animation: fadeIn 200ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  /* ── Header ── */
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    min-height: 48px;
    padding: 0 20px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
    user-select: none;
    -webkit-user-select: none;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .header-title {
    font-family: var(--font-sans);
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }

  .header-center { flex: 1; }

  .header-right {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .connection-status {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .connection-dot {
    width: 6px;
    height: 6px;
    border-radius: 1px;
    background: var(--text-disabled);
    transition: background 200ms ease;
  }

  .connection-dot.online  { background: var(--status-active); }
  .connection-dot.offline { background: var(--status-danger); }

  .connection-label {
    font-family: var(--font-sans);
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .header-sep {
    width: 1px;
    height: 16px;
    background: var(--border-subtle);
  }

  .shortcut-hint {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-disabled);
  }

  .theme-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--toggle-bg);
    border: none;
    border-radius: var(--radius-md);
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: background 150ms ease;
    color: var(--toggle-icon);
  }

  .theme-toggle:hover  { background: var(--border-default); }
  .theme-toggle:active { transform: scale(0.92); }

  .theme-toggle :global(svg) {
    width: 14px;
    height: 14px;
    stroke: currentColor;
    fill: none;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* ── Layout ── */
  .panels {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .sidebar {
    background: var(--bg-sidebar);
    overflow-y: auto;
    overflow-x: hidden;
    flex-shrink: 0;
  }

  .center {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-inset);
  }

  .inspector {
    background: var(--bg-timeline);
    overflow-y: auto;
    flex-shrink: 0;
  }

  .resize-handle {
    width: 1px;
    padding: 0 2px;
    margin: 0 -2px;
    cursor: col-resize;
    background: transparent;
    background-clip: content-box;
    transition: background 150ms ease;
    flex-shrink: 0;
    z-index: 10;
  }

  .resize-handle:hover {
    background: var(--text-disabled, #504840);
    background-clip: content-box;
  }

  .resize-handle.dragging {
    background: var(--accent, #d97706);
    background-clip: content-box;
  }

  /* ── Empty / connecting state ── */
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
  }

  .center-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    animation: fadeIn 400ms ease;
  }

  .empty-title {
    margin: 0;
    font-family: var(--font-sans);
    font-size: 18px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-top: 8px;
  }

  .empty-hint {
    margin: 0;
    font-family: var(--font-sans);
    font-size: 13px;
    color: var(--text-muted);
  }

  .empty-hint :global(kbd) {
    display: inline-block;
    background: var(--bg-kbd);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    padding: 1px 6px;
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--text-tertiary);
  }

  kbd {
    display: inline-block;
    background: var(--bg-kbd);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    padding: 1px 6px;
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .connecting-dots {
    display: flex;
    gap: 6px;
  }

  .connecting-dots .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-disabled);
    animation: dotBounce 1.4s ease-in-out infinite;
  }

  .connecting-dots .dot:nth-child(2) { animation-delay: 0.16s; }
  .connecting-dots .dot:nth-child(3) { animation-delay: 0.32s; }

  @keyframes dotBounce {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
    40%           { opacity: 1;   transform: scale(1.2); }
  }
</style>
