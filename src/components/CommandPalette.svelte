<script>
  import { onMount } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { sessions, client, selectedSessionId } from '../lib/stores.js';

  const dispatch = createEventDispatcher();
  let query = '';
  let inputEl;
  let injectEl;
  let tc = null;
  client.subscribe(v => { tc = v; });

  // Inline inject/broadcast state
  let injectMode = null; // null | { type: 'inject'|'broadcast', id?: string, label: string }
  let injectText = '';

  onMount(() => {
    inputEl?.focus();
  });

  $: actions = buildActions(query, $sessions);

  function buildActions(q, sessionList) {
    const items = [];
    const lower = q.toLowerCase();

    for (const s of sessionList) {
      const name = s.id.replace(/^aigentry-/, '').replace(/-claude$/, '');
      if (!q || name.includes(lower) || s.id.includes(lower)) {
        items.push({ type: 'select', label: `Open ${name}`, id: s.id, icon: '▸' });
        items.push({ type: 'inject', label: `Inject to ${name}`, id: s.id, icon: '→' });
      }
    }

    if (!q || 'broadcast'.includes(lower)) {
      items.push({ type: 'broadcast', label: 'Broadcast to all', icon: '📢' });
    }
    if (!q || 'refresh'.includes(lower)) {
      items.push({ type: 'refresh', label: 'Refresh sessions', icon: '↻' });
    }

    return items.slice(0, 15);
  }

  let selectedIndex = 0;
  $: if (actions.length > 0 && selectedIndex >= actions.length) selectedIndex = 0;

  function startInject(action) {
    if (action.type === 'inject') {
      const name = action.id.replace(/^aigentry-/, '').replace(/-claude$/, '');
      injectMode = { type: 'inject', id: action.id, label: `Message to ${name}:` };
    } else if (action.type === 'broadcast') {
      injectMode = { type: 'broadcast', label: 'Broadcast to all:' };
    }
    injectText = '';
    // Focus the inject input on next tick
    setTimeout(() => injectEl?.focus(), 0);
  }

  async function commitInject() {
    if (!injectText.trim() || !injectMode) return;
    if (injectMode.type === 'inject' && tc) {
      await tc.inject(injectMode.id, injectText);
    } else if (injectMode.type === 'broadcast' && tc) {
      await tc.broadcast(injectText);
    }
    injectMode = null;
    injectText = '';
    dispatch('close');
  }

  function cancelInject() {
    injectMode = null;
    injectText = '';
    // Return focus to main input
    setTimeout(() => inputEl?.focus(), 0);
  }

  async function execute(action) {
    if (action.type === 'select') {
      selectedSessionId.set(action.id);
      dispatch('close');
    } else if (action.type === 'inject' || action.type === 'broadcast') {
      startInject(action);
    } else if (action.type === 'refresh') {
      if (tc) {
        const list = await tc.getSessions();
        sessions.set(list);
      }
      dispatch('close');
    }
  }

  function handleKey(e) {
    if (injectMode) return; // Let inject input handle its own keys
    if (e.key === 'ArrowDown') { e.preventDefault(); selectedIndex = Math.min(selectedIndex + 1, actions.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); selectedIndex = Math.max(selectedIndex - 1, 0); }
    else if (e.key === 'Enter' && actions[selectedIndex]) { e.preventDefault(); execute(actions[selectedIndex]); }
    else if (e.key === 'Escape') dispatch('close');
  }

  function handleInjectKey(e) {
    if (e.key === 'Enter') { e.preventDefault(); commitInject(); }
    else if (e.key === 'Escape') { e.preventDefault(); cancelInject(); }
  }
</script>

<svelte:window on:keydown={handleKey} />

<div class="overlay" on:click={() => dispatch('close')} role="dialog">
  <div class="palette" on:click|stopPropagation role="presentation">
    {#if injectMode}
      <div class="inject-header">
        <span class="inject-label">{injectMode.label}</span>
      </div>
      <input
        bind:this={injectEl}
        bind:value={injectText}
        on:keydown={handleInjectKey}
        placeholder="Type message, Enter to send, Esc to cancel"
        class="input inject-input"
      />
    {:else}
      <input
        bind:this={inputEl}
        bind:value={query}
        on:keydown={handleKey}
        placeholder="Search sessions, actions..."
        class="input"
      />
      <div class="results">
        {#each actions as action, i}
          <button
            class="result"
            class:active={i === selectedIndex}
            on:click={() => execute(action)}
          >
            <span class="action-icon">{action.icon}</span>
            <span>{action.label}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,0.5);
    display: flex; justify-content: center; padding-top: 20vh; z-index: 100;
  }
  .palette {
    width: 480px; max-height: 400px; background: #161b22;
    border: 1px solid #30363d; border-radius: 12px; overflow: hidden;
    box-shadow: 0 16px 64px rgba(0,0,0,0.5);
  }
  .input {
    width: 100%; padding: 12px 16px; border: none; border-bottom: 1px solid #21262d;
    background: transparent; color: #e6edf3; font-size: 15px; outline: none;
    box-sizing: border-box;
  }
  .input::placeholder { color: #484f58; }
  .inject-header {
    padding: 10px 16px 0;
    color: #8b949e;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .inject-input {
    border-bottom: none;
    font-size: 15px;
    padding-top: 8px;
  }
  .results { overflow-y: auto; max-height: 320px; }
  .result {
    display: flex; align-items: center; gap: 10px; width: 100%;
    padding: 10px 16px; border: none; background: none;
    color: #e6edf3; font-size: 14px; cursor: pointer; text-align: left;
  }
  .result:hover, .result.active { background: #1f6feb33; }
  .action-icon { width: 20px; text-align: center; flex-shrink: 0; }
</style>
