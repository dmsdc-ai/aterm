/**
 * aterm client for Svelte frontend.
 *
 * WebSocket mode only — connects to ws://localhost:3849 (Node.js sidecar server).
 * Electron IPC mode removed after Tauri v2 migration.
 */

const WS_URL = 'ws://localhost:3849';

let _idCounter = 0;
function nextId() { return ++_idCounter; }

// -- WebSocket Client --------------------------------------------------------

class AtermWSClient {
  constructor(options = {}) {
    this.url = options.url || WS_URL;

    this.ws = null;
    this.connected = false;

    // Pending requests: id -> { resolve, reject, timer }
    this._pending = new Map();
    // Output subscribers: workspaceId -> Set<callback>
    this._outputSubs = new Map();
    // Global event listeners: eventName -> Set<callback>
    this._listeners = new Map();

    this._reconnectDelay = 500;
    this._reconnectTimer = null;
    this._destroyed = false;

    this.connect();
  }

  // -- Connection ------------------------------------------------------------

  connect() {
    if (this._destroyed) return;

    try {
      this.ws = new WebSocket(this.url);
    } catch (err) {
      console.warn('[aterm-client] WebSocket construction failed:', err.message);
      this._scheduleReconnect();
      return;
    }

    this.ws.onopen = () => {
      this.connected = true;
      this._reconnectDelay = 500;
      this._resubscribeAll();
      this._emit('connected');
    };

    this.ws.onclose = () => {
      this.connected = false;
      this._rejectAllPending('Connection closed');
      this._emit('disconnected');
      this._scheduleReconnect();
    };

    this.ws.onerror = () => {};

    this.ws.onmessage = (e) => {
      let msg;
      try { msg = JSON.parse(e.data); } catch { return; }
      this._handleMessage(msg);
    };
  }

  _scheduleReconnect() {
    if (this._destroyed) return;
    clearTimeout(this._reconnectTimer);
    this._reconnectTimer = setTimeout(() => {
      this._reconnectDelay = Math.min(this._reconnectDelay * 1.5, 3000);
      this.connect();
    }, this._reconnectDelay);
  }

  destroy() {
    this._destroyed = true;
    clearTimeout(this._reconnectTimer);
    if (this.ws) { this.ws.onclose = null; this.ws.close(); }
    this._rejectAllPending('Client destroyed');
  }

  // -- Message dispatch ------------------------------------------------------

  _handleMessage(msg) {
    if (msg.event === 'output') {
      const subs = this._outputSubs.get(msg.workspace);
      if (subs) for (const cb of subs) cb(msg.data);
      return;
    }

    if (msg.event === 'created' || msg.event === 'closed') {
      this._emit(msg.event, msg.workspace);
      return;
    }

    if (msg.id !== undefined) {
      const pending = this._pending.get(msg.id);
      if (pending) {
        clearTimeout(pending.timer);
        this._pending.delete(msg.id);
        if (msg.ok === false) {
          pending.reject(new Error(msg.error || 'Command failed'));
        } else {
          pending.resolve(msg);
        }
      }
    }
  }

  _send(cmd, timeout = 8000) {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        return reject(new Error('Not connected'));
      }

      const id = nextId();
      const timer = setTimeout(() => {
        this._pending.delete(id);
        reject(new Error(`Command timeout: ${cmd.cmd}`));
      }, timeout);

      this._pending.set(id, { resolve, reject, timer });
      this.ws.send(JSON.stringify({ ...cmd, id }));
    });
  }

  _rejectAllPending(reason) {
    for (const { reject, timer } of this._pending.values()) {
      clearTimeout(timer);
      reject(new Error(reason));
    }
    this._pending.clear();
  }

  // -- Commands --------------------------------------------------------------

  listWorkspaces() {
    return this._send({ cmd: 'list-workspaces' }).then(r => r.workspaces ?? []);
  }

  newWorkspace(opts = {}) {
    return this._send({ cmd: 'new-workspace', ...opts }).then(r => r.id);
  }

  closeWorkspace(id) {
    return this._send({ cmd: 'close-workspace', workspace: id });
  }

  send(workspaceId, text) {
    return this._send({ cmd: 'send', workspace: workspaceId, text });
  }

  sendKey(workspaceId, key) {
    return this._send({ cmd: 'send-key', workspace: workspaceId, key });
  }

  readScreen(workspaceId, lines = 50) {
    return this._send({ cmd: 'read-screen', workspace: workspaceId, lines }).then(r => r.lines ?? []);
  }

  resize(workspaceId, cols, rows) {
    return this._send({ cmd: 'resize', workspace: workspaceId, cols, rows });
  }

  status() {
    return this._send({ cmd: 'status' });
  }

  // -- Output subscription ---------------------------------------------------

  onOutput(workspaceId, callback) {
    if (!this._outputSubs.has(workspaceId)) {
      this._outputSubs.set(workspaceId, new Set());
    }
    this._outputSubs.get(workspaceId).add(callback);

    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ cmd: 'subscribe', workspace: workspaceId }));
    } else {
      const onConnect = () => {
        if (this._outputSubs.get(workspaceId)?.has(callback)) {
          this.ws.send(JSON.stringify({ cmd: 'subscribe', workspace: workspaceId }));
        }
      };
      this.once('connected', onConnect);
    }

    return () => this._removeOutputSub(workspaceId, callback);
  }

  _removeOutputSub(workspaceId, callback) {
    const subs = this._outputSubs.get(workspaceId);
    if (!subs) return;
    subs.delete(callback);
    if (subs.size === 0) {
      this._outputSubs.delete(workspaceId);
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(JSON.stringify({ cmd: 'unsubscribe', workspace: workspaceId }));
      }
    }
  }

  _resubscribeAll() {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    for (const workspaceId of this._outputSubs.keys()) {
      this.ws.send(JSON.stringify({ cmd: 'subscribe', workspace: workspaceId }));
    }
  }

  // -- Event emitter ---------------------------------------------------------

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
}

// -- Factory -----------------------------------------------------------------

/**
 * Create an aterm WebSocket client.
 */
export function createAtermClient(options = {}) {
  return new AtermWSClient(options);
}

// Backward-compatible export
export class AtermClient {
  constructor(options = {}) {
    return createAtermClient(options);
  }
}
