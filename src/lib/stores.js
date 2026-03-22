import { writable, derived } from 'svelte/store';
import { isTeleptyAttachWorkspace, teleptyRemoteSessionId } from './workspace-labels.js';

// ── aterm workspace stores ────────────────────────────────────────────────────

// AtermClient instance
export const atermClient = writable(null);

// Workspace list: Array<{ id, cwd, command, status, createdAt, bufferLines }>
export const workspaces = writable([]);

// Discovered telepty sessions: Array<{ id, remoteId, cwd, command, status, kind }>
export const teleptySessions = writable([]);

// Currently selected workspace id
export const activeWorkspaceId = writable(null);

// Selected workspace object (derived)
export const activeWorkspace = derived(
  [workspaces, activeWorkspaceId],
  ([$workspaces, $id]) => $workspaces.find(w => w.id === $id) || null
);

export const visibleTeleptySessions = derived(
  [workspaces, teleptySessions],
  ([$workspaces, $teleptySessions]) => {
    const localRemoteIds = new Set(
      $workspaces
        .filter((workspace) => !isTeleptyAttachWorkspace(workspace))
        .map((workspace) => teleptyRemoteSessionId(workspace))
        .filter(Boolean)
    );

    return $teleptySessions.filter((session) => !localRemoteIds.has(session.remoteId));
  }
);

export const sessions = derived(
  [workspaces, visibleTeleptySessions],
  ([$workspaces, $visibleTeleptySessions]) => [
    ...$visibleTeleptySessions,
    ...$workspaces
      .filter((workspace) => !isTeleptyAttachWorkspace(workspace))
      .map((workspace) => ({ ...workspace, kind: 'pty' })),
  ]
);

export const activeSession = derived(
  [sessions, activeWorkspaceId],
  ([$sessions, $id]) => $sessions.find((session) => session.id === $id) || null
);

// aterm connection status
export const atermConnected = writable(true);

// Refresh workspace list from server (only updates store if changed)
let _lastWorkspacesJson = '';
export async function refreshWorkspaces(ac) {
  if (!ac) return;
  try {
    const list = await ac.listWorkspaces();
    const json = JSON.stringify(list);
    if (json !== _lastWorkspacesJson) {
      _lastWorkspacesJson = json;
      workspaces.set(list);
    }
  } catch (e) {
    // Don't warn on every poll failure — just skip
  }
}

let _lastTeleptySessionsJson = '';
export async function refreshTeleptySessions(ac) {
  if (!ac || !ac.listTeleptySessions) return;
  try {
    const list = await ac.listTeleptySessions();
    const normalized = (list || []).map((session) => ({
      ...session,
      id: `telepty:${session.id}`,
      remoteId: session.id,
      kind: 'telepty',
    }));
    const json = JSON.stringify(normalized);
    if (json !== _lastTeleptySessionsJson) {
      _lastTeleptySessionsJson = json;
      teleptySessions.set(normalized);
    }
  } catch (e) {
    // Ignore daemon discovery errors — local PTY sessions remain available.
  }
}

// ── Group stores ─────────────────────────────────────────────────────────────

// Groups: Array<{ id, name, sessionIds: string[] }>
export const groups = writable([]);

// Currently active group id (null = no group, show 1:1 terminal)
export const activeGroupId = writable(null);

// ── Inject queue stores ─────────────────────────────────────────────────────

// Pending inject counts per workspace: { [workspaceId]: number }
export const pendingInjects = writable({});
