export const CLI_PRESETS = [
  {
    id: 'claude',
    label: 'Claude',
    command: 'claude',
    args: '--dangerously-skip-permissions --continue',
    icon: '✺',
    accent: '#d97706',
  },
  {
    id: 'codex',
    label: 'Codex',
    command: 'codex',
    args: 'resume --last --dangerously-bypass-approvals-and-sandbox',
    icon: '⌘',
    accent: '#16a34a',
  },
  {
    id: 'gemini',
    label: 'Gemini',
    command: 'gemini',
    args: 'resume -y',
    icon: '✦',
    accent: '#2563eb',
  },
  {
    id: 'custom',
    label: 'Custom',
    command: '',
    args: '',
    icon: '⚙',
    accent: '#6b7280',
  },
];

export function getCliPreset(id) {
  return CLI_PRESETS.find((preset) => preset.id === id) ?? null;
}

export function splitArgs(argsText) {
  return argsText.trim() ? argsText.trim().split(/\s+/) : [];
}

export function presetLaunch(id) {
  const preset = getCliPreset(id);
  if (!preset) return null;
  return {
    command: preset.command,
    args: splitArgs(preset.args),
  };
}

export function folderNameFromCwd(cwd) {
  return cwd.replace(/\/+$/, '').split('/').pop() || 'workspace';
}
