import { writable, derived } from 'svelte/store';

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
    // Extract project from id: "aigentry-brain-claude" → "aigentry-brain"
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
