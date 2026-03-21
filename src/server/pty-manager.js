import * as pty from 'node-pty';
import os from 'os';

const BUFFER_MAX_LINES = 1000;

const KEY_MAP = {
  'return': '\r',
  'ctrl+c': '\x03',
  'ctrl+d': '\x04',
  'ctrl+z': '\x1a',
  'tab': '\t',
  'escape': '\x1b',
};

class PtyManager {
  constructor() {
    this.workspaces = new Map();
  }

  createWorkspace(id, options = {}) {
    if (this.workspaces.has(id)) {
      throw new Error(`Workspace '${id}' already exists`);
    }

    const shell = options.command || process.env.SHELL || '/bin/bash';
    const args = options.args || [];
    const cwd = options.cwd || os.homedir();
    const cols = options.cols || 80;
    const rows = options.rows || 24;

    const ptyProcess = pty.spawn(shell, args, {
      name: 'xterm-256color',
      cols,
      rows,
      cwd,
      env: process.env,
    });

    const workspace = {
      id,
      pty: ptyProcess,
      cwd,
      command: shell,
      args,
      buffer: [],
      createdAt: new Date().toISOString(),
      status: 'active',
      callbacks: new Map(),
    };

    // Accumulate output into rolling line buffer
    let partial = '';
    ptyProcess.onData((data) => {
      // Append to partial line accumulator
      partial += data;
      const parts = partial.split('\n');
      // All but the last are complete lines
      for (let i = 0; i < parts.length - 1; i++) {
        workspace.buffer.push(parts[i]);
        if (workspace.buffer.length > BUFFER_MAX_LINES) {
          workspace.buffer.shift();
        }
      }
      // Last part is incomplete — keep for next chunk
      partial = parts[parts.length - 1];

      // Notify subscribers
      for (const cb of workspace.callbacks.values()) {
        cb(data);
      }
    });

    ptyProcess.onExit(() => {
      workspace.status = 'dead';
    });

    this.workspaces.set(id, workspace);
    return workspace;
  }

  destroyWorkspace(id) {
    const ws = this._get(id);
    try {
      ws.pty.kill();
    } catch (_) {
      // already dead
    }
    ws.status = 'dead';
    this.workspaces.delete(id);
  }

  sendToWorkspace(id, text) {
    const ws = this._get(id);
    ws.pty.write(text);
  }

  sendKeyToWorkspace(id, key) {
    const mapped = KEY_MAP[key.toLowerCase()];
    if (!mapped) {
      throw new Error(`Unknown key: '${key}'. Supported: ${Object.keys(KEY_MAP).join(', ')}`);
    }
    const ws = this._get(id);
    ws.pty.write(mapped);
  }

  readScreen(id, lines = 50) {
    const ws = this._get(id);
    const count = Math.min(lines, ws.buffer.length);
    return ws.buffer.slice(ws.buffer.length - count);
  }

  listWorkspaces() {
    return Array.from(this.workspaces.values()).map((ws) => ({
      id: ws.id,
      cwd: ws.cwd,
      command: ws.command,
      args: ws.args,
      status: ws.status,
      createdAt: ws.createdAt,
      bufferLines: ws.buffer.length,
    }));
  }

  resizeWorkspace(id, cols, rows) {
    const ws = this._get(id);
    ws.pty.resize(cols, rows);
  }

  onData(id, callback) {
    const ws = this._get(id);
    const cbId = Symbol();
    ws.callbacks.set(cbId, callback);
    // Return unsubscribe function
    return () => ws.callbacks.delete(cbId);
  }

  // Internal helper — throws if workspace not found
  _get(id) {
    const ws = this.workspaces.get(id);
    if (!ws) throw new Error(`Workspace '${id}' not found`);
    return ws;
  }
}

export default new PtyManager();
