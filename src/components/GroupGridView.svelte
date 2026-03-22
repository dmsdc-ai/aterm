<script>
  import { createEventDispatcher } from 'svelte';
  import Terminal from './Terminal.svelte';
  import { groups, workspaces } from '../lib/stores.js';
  import { workspaceDisplayName } from '../lib/workspace-labels.js';

  export let groupId;

  const dispatch = createEventDispatcher();
  let topicText = '';
  let lastTopicGroupId = null;

  $: group = $groups.find(g => g.id === groupId);
  $: sessionIds = group?.sessionIds || [];
  $: gridCols = Math.max(1, Math.ceil(Math.sqrt(sessionIds.length)));
  $: gridRows = Math.max(1, Math.ceil(sessionIds.length / gridCols));
  $: summary = group?.summary || null;
  $: responses = group?.responses || [];
  $: isBusy = group?.summaryStatus === 'broadcasting' || group?.summaryStatus === 'collecting';
  $: canBroadcast = !!topicText.trim() && sessionIds.length > 0 && !isBusy;
  $: canConverge = sessionIds.length > 0 && !isBusy && (group?.phase || 1) >= 2;
  $: phaseLabel = (group?.phase || 1) === 1 ? 'Phase 1 · Broadcast' : 'Phase 2 · Converge';

  $: if (group && (groupId !== lastTopicGroupId || (!topicText.trim() && group.topic))) {
    topicText = group.topic || '';
    lastTopicGroupId = groupId;
  }

  function sessionName(id) {
    const ws = $workspaces.find(w => w.id === id);
    return workspaceDisplayName(ws || { id });
  }

  function emitBroadcast() {
    const text = topicText.trim();
    if (!text) return;
    dispatch('broadcast', { groupId, text });
  }

  function emitConverge() {
    dispatch('converge', { groupId });
  }

  function formatTime(timestamp) {
    if (!timestamp) return '';
    try {
      return new Date(timestamp).toLocaleTimeString();
    } catch {
      return '';
    }
  }
</script>

<div class="grid-container">
  <div class="grid-header">
    <div class="grid-headline">
      <span class="grid-title">{group?.name || 'Group'}</span>
      <span class="grid-count">{sessionIds.length} sessions</span>
      <span class="phase-badge">{phaseLabel}</span>
    </div>
    <div class="grid-actions">
      <label class="topic-bar">
        <span class="topic-label">Topic</span>
        <input
          bind:value={topicText}
          class="topic-input"
          placeholder="Inject prompt to every session"
          on:keydown={(event) => event.key === 'Enter' && emitBroadcast()}
        />
      </label>
      <button class="action-btn secondary" on:click={emitBroadcast} disabled={!canBroadcast}>
        {group?.summaryStatus === 'broadcasting' ? 'Broadcasting...' : 'Broadcast'}
      </button>
      <button class="action-btn" on:click={emitConverge} disabled={!canConverge}>
        {group?.summaryStatus === 'collecting' ? 'Converging...' : 'Converge'}
      </button>
    </div>
  </div>
  <div class="grid-content" class:with-summary={responses.length > 0 || !!summary || group?.summaryStatus === 'collecting' || !!group?.error}>
    <div
      class="grid"
      style="grid-template-columns: repeat({gridCols}, 1fr); grid-template-rows: repeat({gridRows}, 1fr);"
    >
      {#each sessionIds as sid}
        <div class="grid-cell">
          <div class="cell-header">
            <span class="cell-name">{sessionName(sid)}</span>
          </div>
          <div class="cell-terminal">
            <Terminal workspaceId={sid} embedded />
          </div>
        </div>
      {/each}
    </div>

    {#if responses.length > 0 || summary || group?.summaryStatus === 'collecting' || group?.error}
      <aside class="summary-panel">
        <div class="summary-header">
          <div>
            <div class="summary-title">Summary</div>
            <div class="summary-meta">
              {#if group?.lastSummaryAt}
                Updated {formatTime(group.lastSummaryAt)}
              {:else if group?.lastBroadcastAt}
                Broadcast {formatTime(group.lastBroadcastAt)}
              {:else}
                Waiting for convergence
              {/if}
            </div>
          </div>
          <span class="summary-count">{responses.length}/{sessionIds.length}</span>
        </div>

        {#if group?.error}
          <div class="summary-error">{group.error}</div>
        {/if}

        {#if group?.summaryStatus === 'collecting'}
          <div class="summary-loading">Collecting `read_screen` snapshots from every session.</div>
        {/if}

        {#if summary?.consensus?.length}
          <div class="summary-section">
            <div class="summary-section-title">Consensus</div>
            {#each summary.consensus as line}
              <div class="summary-bullet">{line}</div>
            {/each}
          </div>
        {/if}

        {#if summary?.overview}
          <div class="summary-section">
            <div class="summary-section-title">Overview</div>
            <div class="summary-overview">{summary.overview}</div>
          </div>
        {/if}

        {#if responses.length > 0}
          <div class="summary-section">
            <div class="summary-section-title">Snapshots</div>
            <div class="response-list">
              {#each responses as response}
                <div class="response-card">
                  <div class="response-head">
                    <span class="response-name">{sessionName(response.sessionId)}</span>
                    <span class="response-time">{formatTime(response.capturedAt)}</span>
                  </div>
                  {#if response.error}
                    <div class="response-error">{response.error}</div>
                  {:else}
                    <pre class="response-excerpt">{response.excerpt || response.text || '(empty)'}</pre>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </aside>
    {/if}
  </div>
</div>

<style>
  .grid-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .grid-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 56px;
    padding: 10px 14px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
    user-select: none;
  }

  .grid-headline {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .grid-title {
    font-family: var(--font-sans);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .grid-count {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }

  .phase-badge {
    display: inline-flex;
    align-items: center;
    padding: 4px 9px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .grid-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
    justify-content: flex-end;
  }

  .topic-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 240px;
    max-width: 640px;
    flex: 1;
    padding: 0 12px;
    height: 36px;
    border: 1px solid var(--border-default);
    border-radius: 10px;
    background: var(--bg-inset);
  }

  .topic-label {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    flex-shrink: 0;
  }

  .topic-input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 13px;
    outline: none;
  }

  .topic-input::placeholder {
    color: var(--text-disabled);
  }

  .action-btn {
    height: 36px;
    padding: 0 14px;
    border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 12px;
    cursor: pointer;
    transition: background 120ms ease, opacity 120ms ease;
  }

  .action-btn.secondary {
    border-color: var(--border-default);
    background: var(--bg-panel);
  }

  .action-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .action-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .grid-content {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .grid-content.with-summary .grid {
    flex: 1 1 auto;
  }

  .grid {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: 1px;
    background: var(--border-default);
    overflow: hidden;
  }

  .grid-cell {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-inset);
  }

  .cell-header {
    display: flex;
    align-items: center;
    height: 24px;
    padding: 0 8px;
    background: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .cell-name {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 500;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cell-terminal {
    flex: 1;
    overflow: hidden;
  }

  .summary-panel {
    width: min(360px, 32vw);
    min-width: 280px;
    border-left: 1px solid var(--border-default);
    background: var(--bg-panel);
    display: flex;
    flex-direction: column;
    padding: 14px;
    gap: 14px;
    overflow-y: auto;
  }

  .summary-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .summary-title {
    font-family: var(--font-sans);
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .summary-meta {
    margin-top: 3px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }

  .summary-count {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    padding: 3px 7px;
    border-radius: 999px;
    background: var(--bg-sidebar);
  }

  .summary-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .summary-section-title {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .summary-bullet,
  .summary-overview,
  .summary-loading,
  .summary-error {
    padding: 10px 12px;
    border-radius: 10px;
    background: var(--bg-inset);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    white-space: pre-wrap;
  }

  .summary-error,
  .response-error {
    color: var(--status-danger);
  }

  .response-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .response-card {
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--border-default);
    background: var(--bg-inset);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .response-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .response-name,
  .response-time {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }

  .response-excerpt {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  @media (max-width: 1280px) {
    .grid-header,
    .grid-actions {
      flex-wrap: wrap;
    }

    .summary-panel {
      width: 100%;
      min-width: 0;
      max-height: 42%;
      border-left: none;
      border-top: 1px solid var(--border-default);
    }

    .grid-content {
      flex-direction: column;
    }
  }
</style>
