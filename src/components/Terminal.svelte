<script>
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { atermClient, activeWorkspace, selectedSessionId, selectedSession, client, viewMode } from '../lib/stores.js';
  import '@xterm/xterm/css/xterm.css';

  export let workspaceId;
  export let sessionId = null;

  let termEl;
  let term = null;
  let fitAddon = null;
  let unsubOutput = null;
  let resizeCleanup = null;
  let currentWorkspaceId = null;
  let currentSessionId = null;
  let ac = null;
  let tc = null; // telepty client
  let mounted = false;
  let mode = 'workspace'; // 'workspace' | 'session'

  const unsubClient = atermClient.subscribe(v => {
    ac = v;
    if (mounted && workspaceId && termEl && ac && mode === 'workspace') connect();
  });

  const unsubTeleptClient = client.subscribe(v => { tc = v; });

  const unsubViewMode = viewMode.subscribe(v => {
    mode = v;
  });

  function statusColor(ws) {
    if (!ws) return '#484f58';
    if (ws.status === 'dead') return '#f85149';
    return '#3fb950';
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
    if (currentSessionId && tc) { tc.detachSession(currentSessionId); }
    if (term) { term.dispose(); term = null; }
    fitAddon = null;
    currentWorkspaceId = null;
    currentSessionId = null;
  }

  function connect() {
    if (!ac || !workspaceId || !termEl) return;

    cleanup();
    currentWorkspaceId = workspaceId;
    const myWsId = workspaceId;

    term = new Terminal({
      theme: {
        background: '#000000',
        foreground: '#e6edf3',
        cursor: '#58a6ff',
        cursorAccent: '#000000',
        selectionBackground: '#1f6feb55',
        black: '#484f58',
        red: '#f85149',
        green: '#3fb950',
        yellow: '#d29922',
        blue: '#58a6ff',
        magenta: '#bc8cff',
        cyan: '#39c5cf',
        white: '#b1bac4',
        brightBlack: '#6e7681',
        brightRed: '#ff7b72',
        brightGreen: '#56d364',
        brightYellow: '#e3b341',
        brightBlue: '#79c0ff',
        brightMagenta: '#d2a8ff',
        brightCyan: '#56d4dd',
        brightWhite: '#f0f6fc',
      },
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      cursorBlink: true,
      scrollback: 5000,
      lineHeight: 1.4,
      letterSpacing: 0,
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

  function connectSession(sid) {
    if (!tc || !sid || !termEl) return;

    cleanup();
    currentSessionId = sid;

    term = new Terminal({
      theme: {
        background: '#000000',
        foreground: '#e6edf3',
        cursor: '#58a6ff',
        cursorAccent: '#000000',
        selectionBackground: '#1f6feb55',
      },
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      cursorBlink: true,
      scrollback: 5000,
      lineHeight: 1.4,
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(termEl);
    fitAddon.fit();
    term.focus();

    const myTerm = term;

    tc.attachSession(sid, {
      onOutput: (data) => {
        if (currentSessionId === sid && term === myTerm) {
          term.write(data);
        }
      },
      onClose: () => {
        if (currentSessionId === sid && term === myTerm) {
          term.write('\r\n[session disconnected]\r\n');
        }
      }
    });

    term.onData((data) => {
      if (currentSessionId === sid && tc) {
        tc.sendInput(sid, data);
      }
    });

    const resizeObserver = new ResizeObserver(() => {
      if (!fitAddon || !term) return;
      fitAddon.fit();
    });
    resizeObserver.observe(termEl);
    resizeCleanup = () => resizeObserver.disconnect();
  }

  onMount(() => {
    mounted = true;
    connect();
  });

  onDestroy(() => {
    mounted = false;
    unsubClient();
    unsubTeleptClient();
    unsubViewMode();
    cleanup();
  });

  // Reconnect when workspaceId changes
  $: if (mounted && workspaceId && termEl && ac && workspaceId !== currentWorkspaceId && mode === 'workspace') {
    connect();
  }

  // Connect to telepty session when sessionId changes
  $: if (mounted && sessionId && termEl && tc && sessionId !== currentSessionId && mode === 'session') {
    connectSession(sessionId);
  }
</script>

<div class="terminal-panel">
  <div class="session-header">
    <div class="session-identity">
      {#if mode === 'session' && sessionId}
        <span class="status-dot" style="background: #3fb950; box-shadow: 0 0 5px #3fb95088;"></span>
        <span class="session-badge">SESSION</span>
        <span class="session-name">{sessionId}</span>
      {:else}
        <span
          class="status-dot"
          style="background: {statusColor($activeWorkspace)}; box-shadow: 0 0 5px {statusColor($activeWorkspace)}88;"
        ></span>
        <span class="session-badge ws-badge">WORKSPACE</span>
        <span class="session-name">{shortId(workspaceId)}</span>
      {/if}
    </div>
    <div class="session-meta">
      {#if mode === 'workspace' && $activeWorkspace?.cwd}
        <span class="session-cwd">{$activeWorkspace.cwd}</span>
      {/if}
      {#if mode === 'session' && $selectedSession}
        <span class="session-cwd">{$selectedSession.host || 'Local'}</span>
      {/if}
      <span class="session-status-label" style="color: {mode === 'session' ? '#3fb950' : statusColor($activeWorkspace)};">
        {mode === 'session' ? 'attached' : statusLabel($activeWorkspace)}
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
    background: var(--color-bg-inset);
  }

  .session-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--session-header-height, 32px);
    padding: 0 14px;
    background: var(--color-bg-elevated);
    border-bottom: 1px solid var(--color-border-default);
    flex-shrink: 0;
    user-select: none;
  }

  .session-identity {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background var(--duration-gentle) ease, box-shadow var(--duration-gentle) ease;
  }

  .session-badge {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: #3fb950;
    background: rgba(63, 185, 80, 0.1);
    border: 1px solid rgba(63, 185, 80, 0.2);
    border-radius: 3px;
    padding: 1px 5px;
  }

  .session-badge.ws-badge {
    color: #58a6ff;
    background: rgba(88, 166, 255, 0.1);
    border-color: rgba(88, 166, 255, 0.2);
  }

  .session-name {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-secondary);
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
    color: var(--color-text-disabled);
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
    transition: color var(--duration-gentle) ease;
  }

  .terminal-wrap {
    flex: 1;
    padding: var(--space-3) var(--space-2) var(--space-2);
    background: var(--color-bg-inset);
    overflow: hidden;
  }

  .terminal-wrap :global(.xterm) { height: 100%; }
  .terminal-wrap :global(.xterm-viewport) { background: var(--color-bg-inset) !important; }
  .terminal-wrap :global(.xterm-screen) { background: var(--color-bg-inset); }
</style>
