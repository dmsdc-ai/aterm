<script>
  import { onMount } from 'svelte';
  import { createEventDispatcher } from 'svelte';
  import { folderNameFromCwd } from '../lib/cli-presets.js';

  export let cwd = '';
  export let topic = '';

  const dispatch = createEventDispatcher();
  let topicText = topic;
  let inputEl;

  $: topicText = topic;

  onMount(() => {
    inputEl?.focus();
  });

  function handleCancel() {
    dispatch('cancel');
  }

  function handleSubmit() {
    const trimmed = topicText.trim();
    if (!trimmed) return;
    dispatch('create', { topic: trimmed });
  }

  function handleKeydown(event) {
    if (event.key === 'Escape') {
      handleCancel();
    } else if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      handleSubmit();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" on:mousedown|self={handleCancel}>
  <div class="dialog">
    <div class="dialog-header">
      <span class="dialog-title">Deliberate</span>
      <span class="dialog-folder">{folderNameFromCwd(cwd)}</span>
    </div>

    <div class="dialog-body">
      <div class="label">Topic</div>
      <textarea
        bind:this={inputEl}
        bind:value={topicText}
        class="topic-input"
        rows="6"
        placeholder="Enter the discussion topic for the current session, Codex, and Gemini."
      ></textarea>
      <div class="hint">Cmd+Enter to start</div>
    </div>

    <div class="dialog-footer">
      <button class="btn btn-cancel" on:click={handleCancel} type="button">Cancel</button>
      <button class="btn btn-create" on:click={handleSubmit} type="button">Start</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 220;
    animation: fadeIn 120ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    width: 460px;
    background: var(--bg-sidebar, #f0ebe3);
    border: 1px solid var(--border-default, #d4cfc7);
    border-radius: 12px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.2);
    animation: slideUp 150ms ease;
  }

  @keyframes slideUp {
    from { transform: translateY(8px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }

  .dialog-header {
    padding: 16px 20px 12px;
    border-bottom: 1px solid var(--border-subtle, #e5e0d8);
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .dialog-title {
    font-family: var(--font-sans);
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary, #1a1a1a);
  }

  .dialog-folder {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted, #888);
  }

  .dialog-body {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted, #888);
  }

  .topic-input {
    min-height: 140px;
    resize: vertical;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid var(--border-subtle, #e5e0d8);
    background: var(--bg-inset, #fff);
    color: var(--text-primary, #1a1a1a);
    font: inherit;
    outline: none;
  }

  .topic-input:focus {
    border-color: var(--accent, #d97706);
    box-shadow: 0 0 0 1px var(--accent, #d97706);
  }

  .hint {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted, #888);
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px 18px;
    border-top: 1px solid var(--border-subtle, #e5e0d8);
  }

  .btn {
    min-width: 86px;
    padding: 8px 14px;
    border-radius: 8px;
    border: 1px solid var(--border-subtle, #e5e0d8);
    font: inherit;
    cursor: pointer;
  }

  .btn-cancel {
    background: var(--bg-button, #fff);
    color: var(--text-secondary, #444);
  }

  .btn-create {
    background: var(--accent, #d97706);
    border-color: var(--accent, #d97706);
    color: #fff;
  }
</style>
