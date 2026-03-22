/**
 * aterm client — Tauri IPC mode.
 * All communication via Tauri invoke (commands) and listen (events).
 * No WebSocket, no reconnection logic. IPC is always available.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

class AtermIPCClient {
  constructor() {
    this.connected = true; // IPC is always connected
    this._outputSubs = new Map(); // workspaceId -> Set<callback>
    this._listeners = new Map(); // eventName -> Set<callback>
    this._setupListeners();
  }

  async _setupListeners() {
    await listen('pty-output', (event) => {
      const { workspace, data } = event.payload;
      const subs = this._outputSubs.get(workspace);
      if (subs) for (const cb of subs) cb(data);
    });

    await listen('workspace-created', (event) => {
      this._emit('created', event.payload);
    });

    await listen('workspace-closed', (event) => {
      this._emit('closed', event.payload);
    });

    await listen('workspace-updated', (event) => {
      this._emit('updated', event.payload);
    });

    await listen('inject-queued', (event) => {
      this._emit('inject-queued', event.payload);
    });

    await listen('inject-delivered', (event) => {
      this._emit('inject-delivered', event.payload);
    });
  }

  // -- Commands (all async, using Tauri invoke) --

  async listWorkspaces() {
    return await invoke('list_workspaces');
  }

  async listTeleptySessions() {
    return await invoke('telepty_list_sessions');
  }

  async newWorkspace(opts = {}) {
    const id = opts.id || `ws-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
    return await invoke('create_workspace', {
      id,
      cwd: opts.cwd || '',
      command: opts.command || null,
      args: opts.args || null,
      cols: opts.cols || null,
      rows: opts.rows || null,
      ephemeral: opts.ephemeral || false,
    });
  }

  async closeWorkspace(id) {
    return await invoke('close_workspace', { id });
  }

  async send(workspaceId, text) {
    return await invoke('send_to_workspace', { id: workspaceId, text });
  }

  async sendKey(workspaceId, key) {
    return await invoke('send_key', { id: workspaceId, key });
  }

  async readScreen(workspaceId, maxBytes = 256 * 1024) {
    return await invoke('read_screen', { id: workspaceId, maxBytes });
  }

  async resize(workspaceId, cols, rows) {
    return await invoke('resize_workspace', { id: workspaceId, cols: Math.round(cols), rows: Math.round(rows) });
  }

  async queueInject(workspaceId, from, text) {
    return await invoke('queue_inject', { id: workspaceId, from, text });
  }

  async peekQueue(workspaceId) {
    return await invoke('peek_queue', { id: workspaceId });
  }

  // -- Output subscription (via Tauri events, not WS subscribe/unsubscribe) --

  onOutput(workspaceId, callback) {
    if (!this._outputSubs.has(workspaceId)) {
      this._outputSubs.set(workspaceId, new Set());
    }
    this._outputSubs.get(workspaceId).add(callback);

    return () => {
      const subs = this._outputSubs.get(workspaceId);
      if (subs) {
        subs.delete(callback);
        if (subs.size === 0) this._outputSubs.delete(workspaceId);
      }
    };
  }

  // -- Event emitter (same API as before) --

  on(event, fn) {
    if (!this._listeners.has(event)) this._listeners.set(event, new Set());
    this._listeners.get(event).add(fn);
    return () => this._listeners.get(event)?.delete(fn);
  }

  once(event, fn) {
    const wrapper = (...args) => { fn(...args); this._listeners.get(event)?.delete(wrapper); };
    return this.on(event, wrapper);
  }

  _emit(event, ...args) {
    const fns = this._listeners.get(event);
    if (!fns) return;
    for (const fn of fns) fn(...args);
  }

  destroy() {
    // No-op for IPC — no connection to close
    this._outputSubs.clear();
    this._listeners.clear();
  }
}

export function createAtermClient(options = {}) {
  return new AtermIPCClient();
}

export class AtermClient {
  constructor(options = {}) {
    return createAtermClient(options);
  }
}
