<script>
  import { onMount, onDestroy } from 'svelte';
  import { AtermClient } from './lib/aterm-client.js';
  import {
    atermClient, atermConnected, workspaces, activeWorkspaceId,
    refreshWorkspaces,
  } from './lib/stores.js';
  import SessionTree from './components/SessionTree.svelte';
  import Timeline from './components/Timeline.svelte';
  import Terminal from './components/Terminal.svelte';
  import CommandPalette from './components/CommandPalette.svelte';

  let ac = null;
  let showPalette = false;
  let showSettings = false;
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
    const saved = localStorage.getItem('aterm-theme');
    if (saved === 'light' || saved === 'dark') {
      theme = saved;
      document.documentElement.setAttribute('data-theme', theme);
    } else {
      theme = 'light';
      document.documentElement.setAttribute('data-theme', 'light');
    }
  }

  function setTheme(value) {
    theme = value;
    localStorage.setItem('aterm-theme', value);
    if (value === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      document.documentElement.setAttribute('data-theme', prefersDark ? 'dark' : 'light');
    } else {
      document.documentElement.setAttribute('data-theme', value);
    }
  }

  function handleSettingsClickOutside(e) {
    const panel = document.querySelector('.settings-panel');
    const btn = document.querySelector('.settings-btn');
    if (panel && !panel.contains(e.target) && btn && !btn.contains(e.target)) {
      showSettings = false;
    }
  }

  async function createWorkspaceFromFolder(event) {
    const { cwd } = event.detail;
    try {
      const response = await fetch('/api/create-session', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ cwd, command: 'claude', args: ['--dangerously-skip-permissions'] }),
      });
      const data = await response.json();
      if (!data.ok) {
        console.error('[App] createWorkspace failed:', data.error);
      }
    } catch (e) {
      console.error('[App] createWorkspace failed:', e.message);
    }
    if (ac) {
      await refreshWorkspaces(ac);
      // Auto-select newest workspace
      let list;
      const unsub = workspaces.subscribe(v => { list = v; });
      unsub();
      if (list && list.length > 0) {
        activeWorkspaceId.set(list[list.length - 1].id);
      }
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
    if (e.key === 'Escape') {
      showPalette = false;
      showSettings = false;
    }
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
    </div>
  </header>

  <!-- MAIN 3-PANEL -->
  <div class="panels">
    <aside class="sidebar" style="width: {sidebarWidth}px">
      <SessionTree on:settings={() => showSettings = !showSettings} on:create-workspace={createWorkspaceFromFolder} />
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
      {#if $activeWorkspaceId}
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

  {#if showSettings}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="settings-overlay" on:mousedown={handleSettingsClickOutside}>
      <div class="settings-panel">
        <div class="settings-section">
          <div class="settings-label">Theme</div>
          <div class="theme-options">
            <button class:active={theme === 'light'} on:click={() => setTheme('light')} type="button">
              ☀ Light
            </button>
            <button class:active={theme === 'dark'} on:click={() => setTheme('dark')} type="button">
              ☾ Dark
            </button>
            <button class:active={theme === 'system'} on:click={() => setTheme('system')} type="button">
              ⚙ System
            </button>
          </div>
        </div>
      </div>
    </div>
  {/if}

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

  .settings-btn {
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

  .settings-btn:hover  { background: var(--border-default); }
  .settings-btn:active { transform: scale(0.92); }

  .settings-overlay {
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  .settings-panel {
    position: fixed;
    bottom: 56px;
    left: 16px;
    width: 240px;
    background: var(--bg-sidebar);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 12px);
    padding: 16px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    z-index: 100;
    animation: fadeIn 120ms ease;
  }

  .settings-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .settings-label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }

  .theme-options {
    display: flex;
    gap: 6px;
  }

  .theme-options button {
    flex: 1;
    padding: 6px 4px;
    font-family: var(--font-sans);
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-inset);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
    white-space: nowrap;
  }

  .theme-options button:hover {
    background: var(--border-default);
    color: var(--text-primary);
  }

  .theme-options button.active {
    background: var(--accent, #d97706);
    border-color: var(--accent, #d97706);
    color: #fff;
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
