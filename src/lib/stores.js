import { writable, derived } from 'svelte/store';

// ── aterm workspace stores ────────────────────────────────────────────────────

// AtermClient instance
export const atermClient = writable(null);

// Workspace list: Array<{ id, cwd, command, status, createdAt, bufferLines }>
export const workspaces = writable([]);

// Currently selected workspace id
export const activeWorkspaceId = writable(null);

// Selected workspace object (derived)
export const activeWorkspace = derived(
  [workspaces, activeWorkspaceId],
  ([$workspaces, $id]) => $workspaces.find(w => w.id === $id) || null
);

// aterm connection status
export const atermConnected = writable(false);

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
