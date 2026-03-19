<script>
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { client } from '../lib/stores.js';
  import '@xterm/xterm/css/xterm.css';

  export let sessionId;

  let termEl;
  let term = null;
  let fitAddon = null;
  let ws = null;
  let tc = null;
  let resizeCleanup = null;
  let currentSessionId = null;

  const unsub = client.subscribe(v => { tc = v; });

  function cleanup() {
    if (resizeCleanup) { resizeCleanup(); resizeCleanup = null; }
    if (currentSessionId && tc) { tc.detachSession(currentSessionId); }
    ws = null;
    if (term) { term.dispose(); term = null; }
    fitAddon = null;
  }

  function connect() {
    if (!tc || !sessionId) return;

    cleanup();
    currentSessionId = sessionId;

    term = new Terminal({
      theme: {
        background: '#0d1117',
        foreground: '#e6edf3',
        cursor: '#58a6ff',
        selectionBackground: '#1f6feb55'
      },
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      cursorBlink: true,
      scrollback: 5000
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(termEl);
    fitAddon.fit();

    ws = tc.attachSession(sessionId, {
      onOutput: (data) => term.write(data),
      onClose: () => term.write('\r\n\x1b[33m[session disconnected]\x1b[0m\r\n')
    });

    term.onData((data) => tc.sendInput(sessionId, data));

    const resizeObserver = new ResizeObserver(() => {
      if (fitAddon) fitAddon.fit();
    });
    resizeObserver.observe(termEl);
    resizeCleanup = () => resizeObserver.disconnect();
  }

  onMount(() => {
    connect();
  });

  onDestroy(() => {
    unsub();
    cleanup();
  });

  // Reconnect when sessionId changes
  $: if (sessionId && termEl && tc) {
    connect();
  }
</script>

<div class="terminal-wrap" bind:this={termEl}></div>

<style>
  .terminal-wrap {
    flex: 1;
    padding: 4px;
    background: #0d1117;
    overflow: hidden;
  }
  .terminal-wrap :global(.xterm) { height: 100%; }
</style>
