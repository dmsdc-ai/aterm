<script>
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { atermClient, activeWorkspace } from '../lib/stores.js';
  import '@xterm/xterm/css/xterm.css';

  export let workspaceId;

  let termEl;
  let term = null;
  let fitAddon = null;
  let unsubOutput = null;
  let resizeCleanup = null;
  let currentWorkspaceId = null;
  let ac = null;
  let mounted = false;

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

  function shortId(id) {
    if (!id) return '';
    if (id === 'default') return 'default';
    return id.replace(/^ws-/, '').replace(/-[^-]+$/, '');
  }

  function cleanup() {
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

  function connect() {
    if (!ac || !workspaceId || !termEl) {
      console.warn('[Terminal] connect skipped:', { ac: !!ac, workspaceId, termEl: !!termEl });
      return;
    }
    if (!ac.connected) {
      console.warn('[Terminal] waiting for WS connection before attaching...');
      // Retry when connected
      const unsub = ac.on('connected', () => { unsub(); connect(); });
      return;
    }

    console.log('[Terminal] connecting to workspace:', workspaceId, 'ac.connected:', ac.connected);

    cleanup();
    currentWorkspaceId = workspaceId;
    const myWsId = workspaceId;

    term = new Terminal({
      theme: getTermTheme(),
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
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
    fitAddon.fit();
    term.focus();

    const myTerm = term;

    // Send initial resize to server
    if (ac) {
      const { cols, rows } = term;
      ac.resize(workspaceId, cols, rows).catch(() => {});
    }

    // Subscribe to real-time PTY output
    console.log('[Terminal] subscribing to PTY output for:', workspaceId);
    unsubOutput = ac.onOutput(workspaceId, (data) => {
      if (currentWorkspaceId === myWsId && term === myTerm) {
        term.write(data);
      }
    });

    // Send terminal input to workspace
    term.onData((data) => {
      if (currentWorkspaceId === myWsId && ac) {
        ac.send(workspaceId, data).catch(() => {});
      }
    });

    // Resize: fit terminal → send cols/rows to server
    const resizeObserver = new ResizeObserver(() => {
      if (!fitAddon || !term) return;
      fitAddon.fit();
      const { cols, rows } = term;
      if (ac && currentWorkspaceId === myWsId) {
        ac.resize(workspaceId, cols, rows).catch(() => {});
      }
    });
    resizeObserver.observe(termEl);
    resizeCleanup = () => resizeObserver.disconnect();
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
  <div class="session-header">
    <div class="session-identity">
      <span
        class="status-dot"
        style="background: {statusColor($activeWorkspace)}; box-shadow: 0 0 5px {statusColor($activeWorkspace)}88;"
      ></span>
      <span class="session-name">{shortId(workspaceId)}</span>
    </div>
    <div class="session-meta">
      {#if $activeWorkspace?.cwd}
        <span class="session-cwd">{$activeWorkspace.cwd}</span>
      {/if}
      <span class="session-status-label" style="color: {statusColor($activeWorkspace)};">
        {statusLabel($activeWorkspace)}
      </span>
    </div>
  </div>
  <div class="terminal-wrap" bind:this={termEl} on:click={() => term && term.focus()}></div>
</div>

<style>
  .terminal-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
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
    padding: 6px 4px 4px;
    background: var(--bg-inset, #131010);
    overflow: hidden;
  }

  .terminal-wrap :global(.xterm) { height: 100%; }
  .terminal-wrap :global(.xterm-viewport) { background: var(--bg-inset, #131010) !important; }
  .terminal-wrap :global(.xterm-screen) { background: var(--bg-inset, #131010); }
</style>
