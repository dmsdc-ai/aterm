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
    if (term) { term.dispose(); term = null; }
    fitAddon = null;
    currentWorkspaceId = null;
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

    const myTerm = term;

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

  onMount(() => {
    mounted = true;
    connect();
  });

  onDestroy(() => {
    mounted = false;
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
  <div class="terminal-wrap" bind:this={termEl}></div>
</div>

<style>
  .terminal-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
    background: #000000;
  }

  .session-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 14px;
    background: #161b22;
    border-bottom: 1px solid #30363d;
    flex-shrink: 0;
    user-select: none;
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

  .session-name {
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 12px;
    font-weight: 500;
    color: #c9d1d9;
    letter-spacing: 0.01em;
  }

  .session-meta {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .session-cwd {
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 10px;
    color: #484f58;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .session-status-label {
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0.03em;
    transition: color 400ms ease;
  }

  .terminal-wrap {
    flex: 1;
    padding: 6px 4px 4px;
    background: #000000;
    overflow: hidden;
  }

  .terminal-wrap :global(.xterm) { height: 100%; }
  .terminal-wrap :global(.xterm-viewport) { background: #000000 !important; }
  .terminal-wrap :global(.xterm-screen) { background: #000000; }
</style>
