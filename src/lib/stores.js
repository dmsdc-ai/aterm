import { writable, derived } from 'svelte/store';

// ── Legacy telepty stores (preserved for backward compatibility) ──────────────

// telepty client instance
export const client = writable(null);

// Session list from daemon
export const sessions = writable([]);

// Currently selected session ID
export const selectedSessionId = writable(null);

// Bus event log
export const busEvents = writable([]);

// Connection status
export const connected = writable(false);

// Grouped sessions by project
export const sessionTree = derived(sessions, ($sessions) => {
  const tree = {};
  for (const s of $sessions) {
    const parts = s.id.replace(/-claude$/, '').split('-');
    const project = parts.length > 1 ? parts.slice(0, -1).join('-') : parts[0];
    if (!tree[project]) tree[project] = [];
    tree[project].push(s);
  }
  return tree;
});

// Selected session object
export const selectedSession = derived(
  [sessions, selectedSessionId],
  ([$sessions, $id]) => $sessions.find(s => s.id === $id) || null
);

// Add bus event (max 200)
export function addBusEvent(event) {
  busEvents.update(events => {
    const updated = [...events, { ...event, _ts: Date.now() }];
    return updated.length > 200 ? updated.slice(-200) : updated;
  });
}

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

// Refresh workspace list from server
export async function refreshWorkspaces(ac) {
  if (!ac) return;
  try {
    const list = await ac.listWorkspaces();
    workspaces.set(list);
  } catch (e) {
    console.warn('[stores] refreshWorkspaces failed:', e.message);
  }
}
