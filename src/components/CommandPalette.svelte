<script>
  import { createEventDispatcher } from 'svelte';
  import { sessions, client, selectedSessionId } from '../lib/stores.js';

  const dispatch = createEventDispatcher();
  let query = '';
  let inputEl;
  let tc = null;
  client.subscribe(v => { tc = v; });

  $: actions = buildActions(query, $sessions);

  function buildActions(q, sessionList) {
    const items = [];
    const lower = q.toLowerCase();

    // Session actions
    for (const s of sessionList) {
      const name = s.id.replace(/^aigentry-/, '').replace(/-claude$/, '');
      if (!q || name.includes(lower) || s.id.includes(lower)) {
        items.push({ type: 'select', label: `Open ${name}`, id: s.id, icon: '▸' });
        items.push({ type: 'inject', label: `Inject to ${name}`, id: s.id, icon: '→' });
      }
    }

    // Global actions
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

  async function execute(action) {
    if (action.type === 'select') {
      selectedSessionId.set(action.id);
      dispatch('close');
    } else if (action.type === 'inject') {
      const text = prompt(`Inject to ${action.id}:`);
      if (text && tc) await tc.inject(action.id, text);
      dispatch('close');
    } else if (action.type === 'broadcast') {
      const text = prompt('Broadcast to all:');
      if (text && tc) await tc.broadcast(text);
      dispatch('close');
    } else if (action.type === 'refresh') {
      if (tc) {
        const list = await tc.getSessions();
        sessions.set(list);
      }
      dispatch('close');
    }
  }

  function handleKey(e) {
    if (e.key === 'ArrowDown') { e.preventDefault(); selectedIndex = Math.min(selectedIndex + 1, actions.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); selectedIndex = Math.max(selectedIndex - 1, 0); }
    else if (e.key === 'Enter' && actions[selectedIndex]) { e.preventDefault(); execute(actions[selectedIndex]); }
    else if (e.key === 'Escape') dispatch('close');
  }
</script>

<div class="overlay" on:click={() => dispatch('close')} on:keydown={handleKey} role="dialog">
  <div class="palette" on:click|stopPropagation role="presentation">
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
  </div>
</div>

<svelte:window on:keydown={handleKey} />

{#if inputEl}
  <svelte:element this="script">
    {inputEl?.focus()}
  </svelte:element>
{/if}

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
  .results { overflow-y: auto; max-height: 320px; }
  .result {
    display: flex; align-items: center; gap: 10px; width: 100%;
    padding: 10px 16px; border: none; background: none;
    color: #e6edf3; font-size: 14px; cursor: pointer; text-align: left;
  }
  .result:hover, .result.active { background: #1f6feb33; }
  .action-icon { width: 20px; text-align: center; flex-shrink: 0; }
</style>
