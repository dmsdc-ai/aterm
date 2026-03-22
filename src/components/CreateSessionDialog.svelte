<script>
  import { createEventDispatcher } from 'svelte';
  import { CLI_PRESETS, folderNameFromCwd, splitArgs } from '../lib/cli-presets.js';

  export let cwd = '';

  const dispatch = createEventDispatcher();

  let selectedPreset = 'claude';
  let customCommand = '';
  let customArgs = '';

  $: currentPreset = CLI_PRESETS.find(p => p.id === selectedPreset);
  $: previewCommand = currentPreset?.args ? `${currentPreset.command} ${currentPreset.args}` : currentPreset?.command || '';

  function folderName() {
    return folderNameFromCwd(cwd);
  }

  function handleCreate() {
    let command, args;
    if (selectedPreset === 'custom') {
      command = customCommand.trim();
      args = splitArgs(customArgs);
    } else {
      command = currentPreset.command;
      args = splitArgs(currentPreset.args);
    }
    if (!command) return;
    dispatch('create', { cwd, command, args });
  }

  function handleCancel() {
    dispatch('cancel');
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') handleCancel();
    if (e.key === 'Enter' && selectedPreset !== 'custom') handleCreate();
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" on:mousedown|self={handleCancel}>
  <div class="dialog">
    <div class="dialog-header">
      <span class="dialog-title">New Session</span>
      <span class="dialog-folder">{folderName()}</span>
    </div>

    <div class="dialog-body">
      <div class="label">CLI</div>
      <div class="preset-grid">
        {#each CLI_PRESETS as preset}
          <button
            class="preset-btn"
            class:active={selectedPreset === preset.id}
            on:click={() => selectedPreset = preset.id}
            type="button"
            style={`--preset-accent: ${preset.accent};`}
          >
            <span class="preset-icon">{preset.icon}</span>
            <span class="preset-label">{preset.label}</span>
          </button>
        {/each}
      </div>

      {#if selectedPreset === 'custom'}
        <div class="field">
          <div class="label">Command</div>
          <input class="input" bind:value={customCommand} placeholder="e.g. claude, codex, gemini" />
        </div>
        <div class="field">
          <div class="label">Arguments</div>
          <input class="input" bind:value={customArgs} placeholder="e.g. --full-auto" />
        </div>
      {:else}
        <div class="preview">
          <span class="preview-label">Command:</span>
          <code>{previewCommand}</code>
        </div>
      {/if}
    </div>

    <div class="dialog-footer">
      <button class="btn btn-cancel" on:click={handleCancel} type="button">Cancel</button>
      <button class="btn btn-create" on:click={handleCreate} type="button">Create</button>
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
    z-index: 200;
    animation: fadeIn 120ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .dialog {
    background: var(--bg-sidebar, #f0ebe3);
    border: 1px solid var(--border-default, #d4cfc7);
    border-radius: 12px;
    width: 380px;
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
    gap: 12px;
  }

  .label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted, #888);
  }

  .preset-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }

  .preset-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 10px 4px;
    background: var(--bg-button, #fff);
    border: 1px solid var(--border-subtle, #e5e0d8);
    border-radius: 8px;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .preset-btn:hover {
    background: var(--bg-button-hover, #f0ebe3);
    border-color: var(--border-default, #d4cfc7);
  }

  .preset-btn .preset-icon {
    color: var(--preset-accent, currentColor);
  }

  .preset-btn.active {
    background: var(--preset-accent, var(--accent, #d97706));
    border-color: var(--preset-accent, var(--accent, #d97706));
    color: #fff;
  }

  .preset-btn.active .preset-icon { color: currentColor; }

  .preset-icon {
    font-size: 22px;
    line-height: 1;
  }

  .preset-label {
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .input {
    font-family: var(--font-mono);
    font-size: 13px;
    padding: 8px 10px;
    background: var(--bg-inset, #f5f1eb);
    border: 1px solid var(--border-default, #d4cfc7);
    border-radius: 6px;
    color: var(--text-primary, #1a1a1a);
    outline: none;
  }

  .input:focus {
    border-color: var(--accent, #d97706);
  }

  .preview {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-tertiary, #666);
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .preview-label {
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--text-muted, #888);
  }

  .preview code {
    background: var(--bg-inset, #f5f1eb);
    padding: 2px 6px;
    border-radius: 4px;
    white-space: normal;
    word-break: break-word;
  }

  .dialog-footer {
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border-subtle, #e5e0d8);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn {
    font-family: var(--font-sans);
    font-size: 13px;
    font-weight: 500;
    padding: 7px 16px;
    border-radius: 6px;
    border: none;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .btn-cancel {
    background: var(--bg-button, #fff);
    border: 1px solid var(--border-default, #d4cfc7);
    color: var(--text-secondary, #3d3d3d);
  }

  .btn-cancel:hover { background: var(--bg-button-hover, #f0ebe3); }

  .btn-create {
    background: var(--accent, #d97706);
    color: #fff;
  }

  .btn-create:hover { background: var(--accent-hover, #b45309); }
</style>
