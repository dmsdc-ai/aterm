<script>
  import { onMount } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { workspaces, activeWorkspaceId, atermClient } from '../lib/stores.js';

  const dispatch = createEventDispatcher();
  let query = '';
  let inputEl;

  onMount(() => {
    inputEl?.focus();
  });

  $: actions = buildActions(query, $workspaces);

  function buildActions(q, wsList) {
    const items = [];
    const lower = q.toLowerCase();

    for (const ws of wsList) {
      const name = ws.cwd ? ws.cwd.split('/').pop() : ws.id;
      if (!q || name.toLowerCase().includes(lower) || ws.id.toLowerCase().includes(lower)) {
        items.push({ type: 'select', label: `Open ${name}`, id: ws.id, category: 'session', glyph: '▸' });
      }
    }

    if (!q || 'new'.includes(lower) || 'create'.includes(lower)) {
      items.push({ type: 'new', label: 'New session...', category: 'action', glyph: '+' });
    }

    return items.slice(0, 15);
  }

  let selectedIndex = 0;
  $: if (actions.length > 0 && selectedIndex >= actions.length) selectedIndex = 0;

  function execute(action) {
    if (action.type === 'select') {
      activeWorkspaceId.set(action.id);
      dispatch('close');
    } else if (action.type === 'new') {
      dispatch('close');
      // Trigger new session dialog — handled by parent
    }
  }

  function handleKey(e) {
    if (e.key === 'ArrowDown') { e.preventDefault(); selectedIndex = Math.min(selectedIndex + 1, actions.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); selectedIndex = Math.max(selectedIndex - 1, 0); }
    else if (e.key === 'Enter' && actions[selectedIndex]) { e.preventDefault(); execute(actions[selectedIndex]); }
    else if (e.key === 'Escape') dispatch('close');
  }

  function categoryColor(cat) {
    if (cat === 'session') return 'var(--accent-subtle, #d9770622)';
    if (cat === 'action') return 'var(--accent-subtle, #d9770622)';
    return 'transparent';
  }
  function categoryTextColor(cat) {
    if (cat === 'session') return 'var(--accent, #d97706)';
    if (cat === 'action') return 'var(--text-secondary)';
    return 'var(--text-tertiary)';
  }
</script>

<svelte:window on:keydown={handleKey} />

<div
  class="overlay"
  on:click={() => dispatch('close')}
  on:keydown={(e) => e.key === 'Escape' && dispatch('close')}
  role="dialog"
  aria-modal="true"
  aria-label="Command palette"
  tabindex="-1"
>
  <div class="palette" on:click|stopPropagation role="presentation">

      <!-- Search mode -->
      <div class="search-row">
        <span class="search-icon">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" xmlns="http://www.w3.org/2000/svg">
            <circle cx="6" cy="6" r="4.5" stroke="#484f58" stroke-width="1.25"/>
            <path d="M9.5 9.5L12.5 12.5" stroke="#484f58" stroke-width="1.25" stroke-linecap="round"/>
          </svg>
        </span>
        <input
          bind:this={inputEl}
          bind:value={query}
          on:keydown={handleKey}
          placeholder="Search sessions, actions..."
          class="search-input"
          autocomplete="off"
          spellcheck="false"
        />
        {#if query}
          <button class="clear-btn" aria-label="Clear search" on:click={() => { query = ''; inputEl?.focus(); }}>
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M2 2L10 10M10 2L2 10" stroke="#484f58" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </button>
        {/if}
      </div>

      {#if actions.length > 0}
        <div class="divider"></div>
        <div class="results">
          {#each actions as action, i}
            <button
              class="result"
              class:active={i === selectedIndex}
              on:click={() => execute(action)}
              on:mouseenter={() => selectedIndex = i}
            >
              <span class="result-glyph" style="color: {categoryTextColor(action.category)};">{action.glyph}</span>
              <span class="result-label">{action.label}</span>
              <span class="result-badge" style="background: {categoryColor(action.category)}; color: {categoryTextColor(action.category)};">
                {action.category}
              </span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="empty-state">
          <span>No results for "{query}"</span>
        </div>
      {/if}

      <div class="palette-footer">
        <span class="hint"><kbd>↑↓</kbd> navigate</span>
        <span class="hint"><kbd>Enter</kbd> select</span>
        <span class="hint"><kbd>Esc</kbd> close</span>
      </div>

  </div>
</div>

<style>
  /* ── Keyframes ───────────────────────────────────────── */
  @keyframes fadeIn {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @keyframes fadeInScale {
    from { opacity: 0; transform: scale(0.96) translateY(-8px); }
    to   { opacity: 1; transform: scale(1) translateY(0); }
  }

  .overlay {
    position: fixed;
    top: 0; right: 0; bottom: 0; left: 0;
    background: var(--color-overlay-backdrop);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 18vh;
    z-index: 100;
    animation-name: fadeIn;
    animation-duration: 100ms;
    animation-timing-function: ease;
  }

  .palette {
    width: 520px;
    background: var(--color-bg-elevated);
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-2xl);
    overflow: hidden;
    box-shadow: var(--shadow-palette);
    animation-name: fadeInScale;
    animation-duration: 200ms;
    animation-timing-function: cubic-bezier(0.34, 1.56, 0.64, 1);
    animation-fill-mode: both;
  }

  /* ── Search row ── */
  .search-row {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 14px;
    height: 52px;
    background: var(--color-bg-elevated);
  }

  .search-icon {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    margin-right: 10px;
  }

  .search-input {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--color-text-primary);
    font-family: var(--font-mono);
    font-size: var(--text-lg);
    font-weight: 400;
    outline: none;
    letter-spacing: 0.01em;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--color-text-disabled);
    font-weight: 400;
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius-md);
    flex-shrink: 0;
    opacity: 0.6;
    transition: opacity var(--duration-fast) var(--ease-default);
  }

  .clear-btn:hover { opacity: 1; }

  /* ── Divider ── */
  .divider {
    height: 1px;
    background: var(--color-border-subtle);
    margin: 0;
  }

  /* ── Results list ── */
  .results {
    overflow-y: auto;
    max-height: 292px;
    padding: 4px 0;
  }

  .results::-webkit-scrollbar { width: 4px; }
  .results::-webkit-scrollbar-track { background: transparent; }
  .results::-webkit-scrollbar-thumb { background: var(--color-border-default); border-radius: var(--radius-xs); }

  .result {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 14px;
    border: none;
    background: none;
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background var(--duration-instant) var(--ease-default),
                color var(--duration-instant) var(--ease-default);
  }

  .result:hover,
  .result.active {
    background: var(--color-accent-blue-subtle);
    color: var(--color-text-primary);
  }

  .result.active {
    background: var(--color-accent-blue-subtle);
    border-left: 2px solid var(--color-accent-blue);
    padding-left: 12px;
  }

  .result-glyph {
    width: 16px;
    text-align: center;
    flex-shrink: 0;
    font-size: 12px;
    line-height: 1;
  }

  .result-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
  }

  .result-badge {
    font-size: 10px;
    font-weight: 500;
    padding: 2px 7px;
    border-radius: var(--radius-pill);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    flex-shrink: 0;
    font-family: var(--font-sans);
  }

  /* ── Empty state ── */
  .empty-state {
    padding: 28px 16px;
    text-align: center;
    color: var(--color-text-disabled);
    font-family: var(--font-mono);
    font-size: 13px;
  }

  /* ── Footer hints ── */
  .palette-footer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 14px;
    border-top: 1px solid var(--color-border-subtle);
    background: var(--color-bg-base);
  }

  .hint {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--color-text-disabled);
    font-size: 11px;
    font-family: var(--font-sans);
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg-raised);
    border: 1px solid var(--color-border-default);
    border-bottom-color: var(--color-text-disabled);
    color: var(--color-text-muted);
    border-radius: var(--radius-md);
    padding: 1px 5px;
    font-size: 10px;
    font-family: var(--font-sans);
    line-height: 1.6;
  }

  /* ── Inject panel ── */
  .inject-panel {
    display: flex;
    flex-direction: column;
  }

  .inject-top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px 8px;
    border-bottom: 1px solid var(--color-border-subtle);
  }

  .inject-glyph {
    font-size: 13px;
    color: var(--color-success);
    flex-shrink: 0;
  }

  .inject-label {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .inject-cancel-btn {
    background: none;
    border: 1px solid var(--color-border-default);
    border-radius: var(--radius-md);
    color: var(--color-text-disabled);
    font-size: 10px;
    font-family: var(--font-sans);
    padding: 2px 7px;
    cursor: pointer;
    letter-spacing: 0.04em;
    transition: color var(--duration-fast) var(--ease-default),
                border-color var(--duration-fast) var(--ease-default);
  }

  .inject-cancel-btn:hover {
    color: var(--color-text-tertiary);
    border-color: var(--color-text-disabled);
  }

  .inject-input {
    border: none;
    background: transparent;
    color: var(--color-text-primary);
    font-family: var(--font-mono);
    font-size: var(--text-lg);
    font-weight: 400;
    outline: none;
    padding: 14px 14px;
    width: 100%;
    box-sizing: border-box;
    letter-spacing: 0.01em;
  }

  .inject-input::placeholder {
    color: var(--color-text-disabled);
  }

  .inject-footer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 14px;
    border-top: 1px solid var(--color-border-subtle);
    background: var(--color-bg-base);
  }
</style>
