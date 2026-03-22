import { folderNameFromCwd } from './cli-presets.js';

export const TELEPTY_ATTACH_PREFIX = 'telepty-attach-';

export function teleptyAttachWorkspaceId(remoteId) {
  const safeId = String(remoteId || 'session').replace(/[^a-zA-Z0-9._-]/g, '-');
  return `${TELEPTY_ATTACH_PREFIX}${safeId}`;
}

export function isTeleptyAttachWorkspace(workspace) {
  return workspace?.id?.startsWith(TELEPTY_ATTACH_PREFIX) ?? false;
}

export function teleptyRemoteSessionId(workspace) {
  if (!workspace || workspace.command !== 'telepty') return null;

  const args = workspace.args || [];
  const allowIndex = args.indexOf('allow');
  if (allowIndex !== -1) {
    const idFlagIndex = args.indexOf('--id', allowIndex + 1);
    if (idFlagIndex !== -1) {
      return args[idFlagIndex + 1] || null;
    }
  }

  const attachIndex = args.indexOf('attach');
  if (attachIndex !== -1) {
    const target = args[attachIndex + 1] || '';
    return target.split('@')[0] || null;
  }

  return null;
}

function detectCliName(workspace) {
  if (!workspace) return 'session';
  if (workspace.command && workspace.command !== 'telepty') {
    return workspace.command;
  }

  const args = workspace.args || [];
  const cli = args.find((arg) => ['claude', 'codex', 'gemini'].includes(arg));
  return cli || workspace.command || 'session';
}

function titleCase(value) {
  if (!value) return '';
  return value.charAt(0).toUpperCase() + value.slice(1);
}

export function workspaceDisplayName(workspace) {
  if (!workspace) return 'session';
  const folder = workspace.cwd ? folderNameFromCwd(workspace.cwd) : workspace.id;
  const cli = titleCase(detectCliName(workspace));
  return cli ? `${folder} · ${cli}` : folder;
}
