<script>
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { atermClient, workspaces, pendingInjects } from '../lib/stores.js';
  import '@xterm/xterm/css/xterm.css';

  export let workspaceId;
  export let embedded = false;

  let termEl;
  let term = null;
  let fitAddon = null;
  let unsubOutput = null;
  let resizeCleanup = null;
  let compositionCleanup = null;
  let currentWorkspaceId = null;
  let ac = null;
  let mounted = false;
  let unsubInjectQueued = null;
  let unsubInjectDelivered = null;
  const TERM_FONT_SIZE = 13;
  const TERM_FONT_FAMILY = "'JetBrains Mono', 'Fira Code', monospace";

  $: workspace = $workspaces.find((item) => item.id === workspaceId) || null;

  const unsubClient = atermClient.subscribe(v => {
    ac = v;
    if (mounted && workspaceId && termEl && ac) connect();
  });

  function statusColor(ws) {
    if (!ws) return 'var(--text-disabled)';
    if (ws.status === 'dead') return 'var(--status-danger)';
    return 'var(--status-active)';
  }

  function statusLabel(ws) {
    if (!ws) return 'no workspace';
    return ws.status === 'dead' ? 'dead' : 'active';
  }

  function sessionName(ws) {
    if (ws?.cwd) {
      const parts = ws.cwd.replace(/\/+$/, '').split('/');
      return parts[parts.length - 1] || ws?.id || '';
    }
    if (!ws?.id) return '';
    return ws.id.replace(/^ws-/, '').replace(/-[^-]+$/, '') || ws.id;
  }

  function cleanup() {
    if (unsubInjectQueued) { unsubInjectQueued(); unsubInjectQueued = null; }
    if (unsubInjectDelivered) { unsubInjectDelivered(); unsubInjectDelivered = null; }
    if (compositionCleanup) { compositionCleanup(); compositionCleanup = null; }
    if (resizeCleanup) { resizeCleanup(); resizeCleanup = null; }
    if (unsubOutput) { unsubOutput(); unsubOutput = null; }
    if (term) { term.dispose(); term = null; }
    fitAddon = null;
    currentWorkspaceId = null;
  }

  function getTermTheme() {
    const isDark = document.documentElement.getAttribute('data-theme') !== 'light';
    if (isDark) {
      return {
        background: '#131010',
        foreground: '#e8e4e0',
        cursor: '#d97706',
        cursorAccent: '#131010',
        selectionBackground: 'rgba(217, 119, 6, 0.25)',
        black: '#504840', red: '#d96c6c', green: '#5cb97a', yellow: '#d4a853',
        blue: '#d97706', magenta: '#b8a0d8', cyan: '#5cb0b8', white: '#b0a898',
        brightBlack: '#6a6058', brightRed: '#e88080', brightGreen: '#70cc8a',
        brightYellow: '#e8bb6a', brightBlue: '#f59e0b', brightMagenta: '#c8b0e8',
        brightCyan: '#6ac8d0', brightWhite: '#e8e4e0',
      };
    } else {
      return {
        background: '#faf6f0',
        foreground: '#1a1a1a',
        cursor: '#d97706',
        cursorAccent: '#faf6f0',
        selectionBackground: 'rgba(217, 119, 6, 0.15)',
        black: '#1a1a1a', red: '#d45555', green: '#3da85e', yellow: '#c4952e',
        blue: '#b45309', magenta: '#9b7fd0', cyan: '#0891b2', white: '#666666',
        brightBlack: '#3d3d3d', brightRed: '#e06060', brightGreen: '#4db870',
        brightYellow: '#d4a040', brightBlue: '#d97706', brightMagenta: '#b090e0',
        brightCyan: '#0ea5c9', brightWhite: '#1a1a1a',
      };
    }
  }

  // Watch for theme changes and update terminal
  let themeObserver = null;
  function watchTheme() {
    themeObserver = new MutationObserver(() => {
      if (term) term.options.theme = getTermTheme();
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }

  async function waitForTerminalFonts() {
    if (!document.fonts) return;
    const loads = [
      document.fonts.load(`${TERM_FONT_SIZE}px "JetBrains Mono"`),
      document.fonts.load(`${TERM_FONT_SIZE}px "Fira Code"`),
      document.fonts.ready,
    ];
    await Promise.allSettled(loads);
  }

  function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function nextAnimationFrame() {
    return new Promise((resolve) => requestAnimationFrame(() => resolve()));
  }

  async function waitForNonZeroTerminalSize(el, workspaceKey) {
    for (let attempt = 0; attempt < 40; attempt += 1) {
      if (!mounted || currentWorkspaceId !== workspaceKey || termEl !== el) return false;
      const { width, height } = el.getBoundingClientRect();
      if (width > 0 && height > 0) return true;
      await sleep(50);
    }
    return false;
  }

  function connect() {
    if (!ac || !workspaceId || !termEl) {
      console.warn('[Terminal] connect skipped:', { ac: !!ac, workspaceId, termEl: !!termEl });
      return;
    }
    // IPC is always available — no connection wait needed

    cleanup();
    currentWorkspaceId = workspaceId;
    const myWsId = workspaceId;
    const myTermEl = termEl;

    term = new Terminal({
      theme: getTermTheme(),
      fontSize: TERM_FONT_SIZE,
      fontFamily: TERM_FONT_FAMILY,
      cursorBlink: true,
      scrollback: 5000,
      lineHeight: 1.4,
      letterSpacing: 0,
      allowProposedApi: true,
      windowsMode: false,
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(termEl);

    const myTerm = term;
    const sendText = (text) => {
      if (text && currentWorkspaceId === myWsId && ac) {
        ac.send(workspaceId, text).catch(() => {});
      }
    };
    compositionCleanup = null;
    let initialLayoutReady = false;
    let outputSubscribed = false;
    let lastCols = 0;
    let lastRows = 0;
    let initialFitQueued = false;
    let layoutSyncRunning = false;
    let pendingLayoutSync = null;

    const performLayoutSync = async ({ refresh = false, focus = false } = {}) => {
      if (currentWorkspaceId !== myWsId || !fitAddon || !term || !termEl) return false;

      const { width, height } = termEl.getBoundingClientRect();
      if (width <= 0 || height <= 0) return false;

      fitAddon.fit();
      const { cols, rows } = term;
      if (cols <= 0 || rows <= 0) return false;

      if (refresh) term.refresh(0, rows - 1);
      if (focus) term.focus();

      const sizeChanged = cols !== lastCols || rows !== lastRows;
      lastCols = cols;
      lastRows = rows;

      if (ac && currentWorkspaceId === myWsId && sizeChanged) {
        try {
          await ac.resize(myWsId, cols, rows);
        } catch {}
      }

      return true;
    };

    const syncTerminalLayout = async ({ refresh = false, focus = false } = {}) => {
      pendingLayoutSync = pendingLayoutSync
        ? {
            refresh: pendingLayoutSync.refresh || refresh,
            focus: pendingLayoutSync.focus || focus,
          }
        : { refresh, focus };

      if (layoutSyncRunning) return false;
      layoutSyncRunning = true;

      let applied = false;
      try {
        while (pendingLayoutSync) {
          const nextSync = pendingLayoutSync;
          pendingLayoutSync = null;
          applied = (await performLayoutSync(nextSync)) || applied;
        }
      } finally {
        layoutSyncRunning = false;
      }

      return applied;
    };

    const subscribeOutput = () => {
      if (outputSubscribed || !ac) return;
      outputSubscribed = true;

      unsubOutput = ac.onOutput(workspaceId, (data) => {
        if (currentWorkspaceId === myWsId && term === myTerm) {
          term.write(data);
        }
      });
    };

    const writeSnapshot = (snapshot) => new Promise((resolve) => {
      if (!snapshot) {
        resolve();
        return;
      }

      myTerm.write(snapshot, () => {
        if (currentWorkspaceId === myWsId && term === myTerm) {
          myTerm.refresh(0, myTerm.rows - 1);
        }
        resolve();
      });
    });

    const restoreScreen = async () => {
      if (!ac || currentWorkspaceId !== myWsId || term !== myTerm) return;

      try {
        const snapshot = await ac.readScreen(workspaceId);
        if (currentWorkspaceId !== myWsId || term !== myTerm || !snapshot) return;

        await writeSnapshot(snapshot);
      } catch {
        // Ignore snapshot restore failures — live PTY output remains authoritative.
      }
    };

    const queueInitialFit = () => {
      if (initialFitQueued || initialLayoutReady) return;
      initialFitQueued = true;

      (async () => {
        const hasSize = await waitForNonZeroTerminalSize(myTermEl, myWsId);
        if (!hasSize) return;
        await waitForTerminalFonts();
        await nextAnimationFrame();
        await nextAnimationFrame();
        if (currentWorkspaceId !== myWsId) return;
        if (await syncTerminalLayout({ refresh: true, focus: true })) {
          initialLayoutReady = true;
          await nextAnimationFrame();
          await syncTerminalLayout({ refresh: true });
          restoreScreen().finally(() => {
            subscribeOutput();
          });
        }
      })().finally(() => {
        initialFitQueued = false;
      });
    };

    // Send terminal input to workspace
    term.onData((data) => {
      sendText(data);
    });

    // Resize: fit terminal → send cols/rows to server
    const resizeObserver = new ResizeObserver(([entry]) => {
      if (!entry || currentWorkspaceId !== myWsId) return;
      const { width, height } = entry.contentRect;
      if (width <= 0 || height <= 0) return;

      if (!initialLayoutReady) {
        queueInitialFit();
        return;
      }

      void syncTerminalLayout();
    });
    resizeObserver.observe(termEl);
    resizeCleanup = () => resizeObserver.disconnect();

    // Subscribe to inject events for this workspace
    if (ac) {
      unsubInjectQueued = ac.on('inject-queued', (payload) => {
        if (payload.workspace === myWsId) {
          pendingInjects.update(p => ({ ...p, [payload.workspace]: payload.pending }));
        }
      });
      unsubInjectDelivered = ac.on('inject-delivered', (payload) => {
        if (payload.workspace === myWsId) {
          pendingInjects.update(p => ({ ...p, [payload.workspace]: payload.pending }));
        }
      });
    }

    queueInitialFit();
  }

  onMount(() => {
    mounted = true;
    watchTheme();
    connect();
  });

  onDestroy(() => {
    mounted = false;
    if (themeObserver) { themeObserver.disconnect(); themeObserver = null; }
    unsubClient();
    cleanup();
  });

  // Reconnect when workspaceId changes
  $: if (mounted && workspaceId && termEl && ac && workspaceId !== currentWorkspaceId) {
    connect();
  }
</script>

<div class="terminal-panel">
  {#if !embedded}
    <div class="session-header">
      <div class="session-identity">
        <span
          class="status-dot"
          style="background: {statusColor(workspace)}; box-shadow: 0 0 5px {statusColor(workspace)}88;"
        ></span>
        <span class="session-name">{sessionName(workspace)}</span>
        {#if $pendingInjects[workspaceId] > 0}
          <span class="inject-badge" title="{$pendingInjects[workspaceId]} pending inject(s)">
            {$pendingInjects[workspaceId]}
          </span>
        {/if}
      </div>
      <div class="session-meta">
        {#if workspace?.cwd}
          <span class="session-cwd">{workspace.cwd}</span>
        {/if}
        <span class="session-status-label" style="color: {statusColor(workspace)};">
          {statusLabel(workspace)}
        </span>
      </div>
    </div>
  {/if}
  <div class="terminal-wrap" bind:this={termEl} on:click={() => term && term.focus()}></div>
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
    transition: background 400ms ease, box-shadow 400ms ease;
  }

  .session-badge {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--status-active);
    background: rgba(92, 185, 122, 0.1);
    border: 1px solid rgba(92, 185, 122, 0.2);
    border-radius: 3px;
    padding: 1px 5px;
  }

  .session-badge.ws-badge {
    color: var(--accent);
    background: var(--accent-subtle);
    border-color: var(--accent-muted);
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
    font-weight: 400;
    letter-spacing: 0.03em;
    transition: color 400ms ease;
  }

  .terminal-wrap {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    padding: 6px 4px 4px;
    background: var(--bg-inset, #131010);
    overflow: hidden;
  }

  .terminal-wrap :global(.xterm) {
    width: 100%;
    height: 100%;
  }

  .terminal-wrap :global(.xterm-viewport) {
    width: 100% !important;
    height: 100% !important;
    background: var(--bg-inset, #131010) !important;
  }

  .terminal-wrap :global(.xterm-screen) {
    width: 100%;
    height: 100%;
    background: var(--bg-inset, #131010);
  }
</style>
