<script>
  import { onMount, onDestroy } from 'svelte';
  import { AtermClient } from './lib/aterm-client.js';
  import {
    atermClient, atermConnected, workspaces, activeWorkspaceId,
    refreshWorkspaces, refreshTeleptySessions, visibleTeleptySessions, groups, activeGroupId, activeSession,
  } from './lib/stores.js';
  import SessionTree from './components/SessionTree.svelte';
  import Terminal from './components/Terminal.svelte';
  import TeleptyTerminal from './components/TeleptyTerminal.svelte';
  import GroupGridView from './components/GroupGridView.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import CreateSessionDialog from './components/CreateSessionDialog.svelte';
  import DeliberateDialog from './components/DeliberateDialog.svelte';
  import { folderNameFromCwd, presetLaunch } from './lib/cli-presets.js';
  import { isTeleptyAttachWorkspace } from './lib/workspace-labels.js';

  let ac = null;
  let showPalette = false;
  let pendingSessionCwd = null;
  let pendingDeliberation = null;
  let showSettings = false;
  let refreshTimer = null;
  let theme = 'light';

  // Resize state
  const SIDEBAR_DEFAULT = 250;
  const SIDEBAR_MIN = 150;
  const SIDEBAR_MAX = 400;

  let sidebarWidth = SIDEBAR_DEFAULT;
  let draggingSidebar = false;

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

  async function createWorkspaceFromFolder() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: 'Select project folder' });
      if (!selected) return;
      pendingSessionCwd = Array.isArray(selected) ? selected[0] : selected;
    } catch (e) {
      console.warn('[App] folder dialog failed:', e);
    }
  }

  function storeValue(store) {
    let value;
    const unsubscribe = store.subscribe((current) => { value = current; });
    unsubscribe();
    return value;
  }

  function randomSuffix() {
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
  }

  function patchGroup(groupId, patchOrUpdater) {
    groups.update((list) => list.map((group) => {
      if (group.id !== groupId) return group;
      const nextPatch = typeof patchOrUpdater === 'function' ? patchOrUpdater(group) : patchOrUpdater;
      return nextPatch ? { ...group, ...nextPatch } : group;
    }));
  }

  function selectGroup(groupId) {
    if (!groupId) return;
    activeGroupId.set(groupId);
    activeWorkspaceId.set(null);
  }

  function stripAnsi(text = '') {
    return text
      .replace(/\u001b\][^\u0007]*(\u0007|\u001b\\)/g, '')
      .replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, '')
      .replace(/\u001b[@-_]/g, '');
  }

  function cleanScreenText(text = '') {
    return stripAnsi(text)
      .replace(/\r/g, '\n')
      .split('\n')
      .map((line) => line.replace(/\s+$/g, ''))
      .join('\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
  }

  function meaningfulLines(text = '') {
    return cleanScreenText(text)
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
  }

  function extractExcerpt(text = '') {
    const lines = meaningfulLines(text);
    if (lines.length === 0) return '';
    return lines.slice(-6).join('\n');
  }

  function summarizeResponses(responses) {
    const successful = responses.filter((response) => !response.error && response.text);
    if (successful.length === 0) {
      return {
        overview: 'No readable session output was captured from `read_screen`.',
        consensus: [],
      };
    }

    const lineCounts = new Map();
    for (const response of successful) {
      const uniqueLines = new Set(
        meaningfulLines(response.text)
          .filter((line) => line.length >= 24)
          .slice(-12)
      );
      for (const line of uniqueLines) {
        lineCounts.set(line, (lineCounts.get(line) || 0) + 1);
      }
    }

    const consensus = [...lineCounts.entries()]
      .filter(([, count]) => count > 1)
      .sort((a, b) => b[1] - a[1] || b[0].length - a[0].length)
      .slice(0, 3)
      .map(([line, count]) => `${line} (${count}/${successful.length})`);

    const takeaways = successful
      .map((response) => {
        const lines = meaningfulLines(response.text);
        return {
          sessionId: response.sessionId,
          line: lines.slice(-1)[0] || '',
        };
      })
      .filter((item) => item.line);

    const overview = consensus.length > 0
      ? `${successful.length} session(s) responded. Shared lines surfaced across multiple screens.`
      : takeaways.length > 0
        ? `No exact overlap across ${successful.length} session(s). Review the latest takeaway from each screen below.`
        : `${successful.length} session(s) responded, but the captured output was sparse.`;

    return {
      overview,
      consensus,
      takeaways,
    };
  }

  async function broadcastGroup(groupId, rawText) {
    const text = rawText.trim();
    const group = storeValue(groups).find((item) => item.id === groupId);
    if (!ac || !group || !text) return;

    patchGroup(groupId, {
      topic: text,
      phase: 2,
      responses: [],
      summary: null,
      summaryStatus: 'broadcasting',
      error: null,
    });

    try {
      await Promise.all(group.sessionIds.map((workspaceId) => ac.queueInject(workspaceId, 'broadcast', text)));
      patchGroup(groupId, {
        lastBroadcastAt: Date.now(),
        summaryStatus: 'idle',
      });
      selectGroup(groupId);
    } catch (e) {
      patchGroup(groupId, {
        phase: 1,
        summaryStatus: 'error',
        error: String(e?.message || e || 'Broadcast failed'),
      });
      console.error('[App] group broadcast failed:', e);
    }
  }

  async function convergeGroup(groupId) {
    const group = storeValue(groups).find((item) => item.id === groupId);
    if (!ac || !group) return;

    patchGroup(groupId, {
      phase: 2,
      summaryStatus: 'collecting',
      error: null,
    });

    try {
      const settled = await Promise.allSettled(
        group.sessionIds.map(async (sessionId) => ({
          sessionId,
          text: cleanScreenText(await ac.readScreen(sessionId)),
          capturedAt: Date.now(),
        }))
      );

      const responses = group.sessionIds.map((sessionId, index) => {
        const item = settled[index];
        if (item?.status === 'fulfilled') {
          return {
            ...item.value,
            excerpt: extractExcerpt(item.value.text),
          };
        }
        return {
          sessionId,
          text: '',
          excerpt: '',
          capturedAt: Date.now(),
          error: String(item?.reason?.message || item?.reason || 'read_screen failed'),
        };
      });

      patchGroup(groupId, {
        responses,
        summary: summarizeResponses(responses),
        summaryStatus: 'ready',
        lastSummaryAt: Date.now(),
      });
      selectGroup(groupId);
    } catch (e) {
      patchGroup(groupId, {
        summaryStatus: 'error',
        error: String(e?.message || e || 'Converge failed'),
      });
      console.error('[App] group converge failed:', e);
    }
  }

  function buildSessionId(cwd, cliName, suffix = '') {
    const folderName = folderNameFromCwd(cwd);
    return `${folderName}-${cliName}${suffix ? `-${suffix}` : ''}`;
  }

  function nextSessionId(cwd, cliName) {
    const existingIds = new Set(storeValue(workspaces).map((workspace) => workspace.id));
    const base = buildSessionId(cwd, cliName);
    if (!existingIds.has(base)) return base;

    let index = 2;
    while (existingIds.has(`${base}-${index}`)) {
      index += 1;
    }
    return `${base}-${index}`;
  }

  async function createWorkspace({ cwd, command, args, id, ephemeral = false }) {
    return ac.newWorkspace({
      id,
      cwd,
      command,
      args,
      ephemeral,
    });
  }

  async function refreshAndSelectLatest() {
    if (!ac) return;
    await Promise.all([refreshWorkspaces(ac), refreshTeleptySessions(ac)]);
    const list = storeValue(workspaces).filter((workspace) => !isTeleptyAttachWorkspace(workspace));
    if (list && list.length > 0) {
      activeWorkspaceId.set(list[list.length - 1].id);
    }
  }

  async function handleSessionCreate(event) {
    const { cwd, command, args } = event.detail;
    pendingSessionCwd = null;
    try {
      const cliName = command === 'telepty' ? 'claude' : command;
      const sessionId = nextSessionId(cwd, cliName);
      await createWorkspace({ cwd, command, args, id: sessionId });
    } catch (e) {
      console.error('[App] createWorkspace failed:', e);
    }
    await refreshAndSelectLatest();
  }

  function handleSessionCancel() {
    pendingSessionCwd = null;
  }

  function openDeliberateDialog(prefilledTopic = '') {
    const currentSession = storeValue(activeSession);
    const list = storeValue(workspaces).filter((workspace) => !isTeleptyAttachWorkspace(workspace));
    const source = currentSession?.kind === 'pty'
      ? currentSession
      : list.find((ws) => ws.status !== 'dead');
    if (!source) return;
    pendingDeliberation = {
      workspaceId: source.id,
      cwd: source.cwd,
      topic: prefilledTopic,
    };
  }

  function handleDeliberateCancel() {
    pendingDeliberation = null;
  }

  async function handleDeliberateCreate(event) {
    const topic = event.detail.topic.trim();
    const source = pendingDeliberation;
    pendingDeliberation = null;

    if (!topic || !source || !ac) return;

    try {
      const suffix = randomSuffix();
      const codexPreset = presetLaunch('codex');
      const geminiPreset = presetLaunch('gemini');
      if (!codexPreset || !geminiPreset) return;

      const codexId = buildSessionId(source.cwd, 'codex', suffix);
      const geminiId = buildSessionId(source.cwd, 'gemini', suffix);

      await createWorkspace({
        cwd: source.cwd,
        command: codexPreset.command,
        args: codexPreset.args,
        id: codexId,
        ephemeral: true,
      });
      await createWorkspace({
        cwd: source.cwd,
        command: geminiPreset.command,
        args: geminiPreset.args,
        id: geminiId,
        ephemeral: true,
      });

      await refreshWorkspaces(ac);

      const groupId = `deliberate-${suffix}`;
      const groupName = topic.length > 42 ? `Deliberate: ${topic.slice(0, 39)}...` : `Deliberate: ${topic}`;
      const sessionIds = [source.id, codexId, geminiId];

      groups.update((list) => [
        ...list.filter((group) => group.id !== groupId),
        {
          id: groupId,
          name: groupName,
          sessionIds,
          topic,
          phase: 1,
          responses: [],
          summary: null,
          summaryStatus: 'idle',
          error: null,
          lastBroadcastAt: null,
          lastSummaryAt: null,
        },
      ]);
      activeGroupId.set(groupId);
      activeWorkspaceId.set(null);
    } catch (e) {
      console.error('[App] deliberate failed:', e);
    }
  }

  onMount(async () => {
    try { initTheme(); } catch (e) { console.warn('[App] initTheme failed:', e); }

    // Load persisted widths
    try {
      const savedSidebar = localStorage.getItem('aterm-sidebar-width');
      if (savedSidebar) sidebarWidth = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Number(savedSidebar)));
    } catch (e) { console.warn('[App] localStorage read failed:', e); }

    try {
      ac = new AtermClient();
      atermClient.set(ac);

      // IPC is always connected — load workspaces immediately
      try {
        await Promise.all([refreshWorkspaces(ac), refreshTeleptySessions(ac)]);
        activeWorkspaceId.update(cur => {
          if (cur) return cur;
          const remoteSessions = storeValue(visibleTeleptySessions);
          const localSessions = storeValue(workspaces).filter((workspace) => !isTeleptyAttachWorkspace(workspace));
          return remoteSessions?.[0]?.id ?? localSessions?.[0]?.id ?? null;
        });
      } catch (e) { console.warn('[App] workspace refresh failed:', e); }

      ac.on('created', async () => { try { await Promise.all([refreshWorkspaces(ac), refreshTeleptySessions(ac)]); } catch {} });
      ac.on('closed',  async () => { try { await Promise.all([refreshWorkspaces(ac), refreshTeleptySessions(ac)]); } catch {} });
      ac.on('updated', async () => { try { await Promise.all([refreshWorkspaces(ac), refreshTeleptySessions(ac)]); } catch {} });

      refreshTimer = setInterval(() => {
        try {
          refreshWorkspaces(ac);
          refreshTeleptySessions(ac);
        } catch {}
      }, 5000);
    } catch (e) {
      console.warn('[App] AtermClient init failed:', e);
    }
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

  function handlePaletteNew() {
    showPalette = false;
    createWorkspaceFromFolder();
  }

  function handlePaletteDeliberate(event) {
    showPalette = false;
    openDeliberateDialog(event.detail?.topic || '');
  }

  function handlePaletteGroup(event) {
    showPalette = false;
    const requestedGroupId = event.detail?.groupId;
    const latestGroup = storeValue(groups).slice(-1)[0] || null;
    const targetGroupId = requestedGroupId || latestGroup?.id || '';
    if (targetGroupId) {
      selectGroup(targetGroupId);
      return;
    }
    openDeliberateDialog(event.detail?.topic || '');
  }

  async function handlePaletteBroadcast(event) {
    showPalette = false;
    const requestedGroupId = event.detail?.groupId;
    const latestGroup = storeValue(groups).slice(-1)[0] || null;
    const targetGroupId = requestedGroupId || storeValue(activeGroupId) || latestGroup?.id || '';
    const text = (event.detail?.text || '').trim();

    if (!targetGroupId) {
      openDeliberateDialog(text);
      return;
    }

    selectGroup(targetGroupId);

    if (!text) return;
    await broadcastGroup(targetGroupId, text);
  }

  async function handleGroupBroadcast(event) {
    const groupId = event.detail?.groupId;
    const text = event.detail?.text || '';
    if (!groupId) return;
    await broadcastGroup(groupId, text);
  }

  async function handleGroupConverge(event) {
    const groupId = event.detail?.groupId;
    if (!groupId) return;
    await convergeGroup(groupId);
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
      <span class="header-title">aterm v2 (Tauri)</span>
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
      {#if $activeGroupId}
        <GroupGridView
          groupId={$activeGroupId}
          on:broadcast={handleGroupBroadcast}
          on:converge={handleGroupConverge}
        />
      {:else if $activeSession}
        {#if $activeSession.kind === 'telepty'}
          <TeleptyTerminal sessionId={$activeSession.id} />
        {:else}
          <Terminal workspaceId={$activeSession.id} />
        {/if}
      {:else}
        <div class="empty">
          <div class="center-empty">
            <span class="mascot mascot-lg pulse">
              <span class="mascot-eye">·</span><span class="mascot-core">⣿</span><span class="mascot-eye">·</span>
            </span>
            <p class="empty-title">No workspace selected</p>
            <p class="empty-hint">Select a session or press <kbd>+</kbd> to create one</p>
          </div>
        </div>
      {/if}
    </main>
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
    <CommandPalette
      on:close={() => showPalette = false}
      on:new={handlePaletteNew}
      on:deliberate={handlePaletteDeliberate}
      on:group={handlePaletteGroup}
      on:broadcast={handlePaletteBroadcast}
    />
  {/if}

  {#if pendingSessionCwd}
    <CreateSessionDialog cwd={pendingSessionCwd} on:create={handleSessionCreate} on:cancel={handleSessionCancel} />
  {/if}

  {#if pendingDeliberation}
    <DeliberateDialog
      cwd={pendingDeliberation.cwd}
      topic={pendingDeliberation.topic}
      on:create={handleDeliberateCreate}
      on:cancel={handleDeliberateCancel}
    />
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
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .sidebar {
    background: var(--bg-sidebar);
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    flex-shrink: 0;
  }

  .center {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-inset);
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
