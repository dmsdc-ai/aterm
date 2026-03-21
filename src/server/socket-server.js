import net from 'net';
import fs from 'fs';
import path from 'path';
import os from 'os';
import ptyManager from './pty-manager.js';

const SOCKET_DIR = path.join(os.homedir(), '.aterm');
const SOCKET_PATH = path.join(SOCKET_DIR, 'aterm.sock');

let server = null;
let startTime = null;

function ensureSocketDir() {
  fs.mkdirSync(SOCKET_DIR, { recursive: true });
}

function removeStaleSocket() {
  try {
    fs.unlinkSync(SOCKET_PATH);
  } catch (_) {
    // No stale socket — fine
  }
}

function generateWorkspaceId() {
  return `ws-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

async function handleCommand(cmd) {
  switch (cmd.cmd) {
    case 'send': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      if (cmd.text === undefined) return { ok: false, error: 'Missing text' };
      ptyManager.sendToWorkspace(cmd.workspace, cmd.text);
      return { ok: true };
    }

    case 'send-key': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      if (!cmd.key) return { ok: false, error: 'Missing key' };
      ptyManager.sendKeyToWorkspace(cmd.workspace, cmd.key);
      return { ok: true };
    }

    case 'read-screen': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      const lines = ptyManager.readScreen(cmd.workspace, cmd.lines ?? 50);
      return { ok: true, lines };
    }

    case 'list-workspaces': {
      const workspaces = ptyManager.listWorkspaces();
      return { ok: true, workspaces };
    }

    case 'new-workspace': {
      const id = cmd.id || generateWorkspaceId();
      ptyManager.createWorkspace(id, {
        cwd: cmd.cwd,
        command: cmd.command,
        cols: cmd.cols,
        rows: cmd.rows,
      });
      return { ok: true, id };
    }

    case 'close-workspace': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      ptyManager.destroyWorkspace(cmd.workspace);
      return { ok: true };
    }

    case 'resize': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      if (!cmd.cols || !cmd.rows) return { ok: false, error: 'Missing cols/rows' };
      ptyManager.resizeWorkspace(cmd.workspace, cmd.cols, cmd.rows);
      return { ok: true };
    }

    case 'status': {
      const workspaces = ptyManager.listWorkspaces();
      return {
        ok: true,
        uptime: startTime ? Math.floor((Date.now() - startTime) / 1000) : 0,
        workspaces: workspaces.length,
        socketPath: SOCKET_PATH,
      };
    }

    default:
      return { ok: false, error: `Unknown command: '${cmd.cmd}'` };
  }
}

function handleClient(socket) {
  let buf = '';

  socket.on('data', async (chunk) => {
    buf += chunk.toString();
    const lines = buf.split('\n');
    // Keep the last (potentially incomplete) fragment
    buf = lines.pop();

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;

      let response;
      try {
        const cmd = JSON.parse(trimmed);
        response = await handleCommand(cmd);
      } catch (err) {
        response = { ok: false, error: err.message };
      }

      if (!socket.destroyed) {
        socket.write(JSON.stringify(response) + '\n');
      }
    }
  });

  socket.on('error', (err) => {
    if (err.code !== 'ECONNRESET') {
      console.error('[socket-server] client error:', err.message);
    }
  });

  socket.on('close', () => {
    // nothing to clean up per-client
  });
}

export function start() {
  return new Promise((resolve, reject) => {
    ensureSocketDir();
    removeStaleSocket();

    server = net.createServer(handleClient);
    startTime = Date.now();

    server.on('error', (err) => {
      console.error('[socket-server] server error:', err.message);
      reject(err);
    });

    server.listen(SOCKET_PATH, () => {
      // Restrict socket to owner only
      fs.chmodSync(SOCKET_PATH, 0o600);
      console.log(`[socket-server] listening on ${SOCKET_PATH}`);
      resolve(SOCKET_PATH);
    });
  });
}

export function stop() {
  return new Promise((resolve) => {
    if (!server) return resolve();
    server.close(() => {
      removeStaleSocket();
      resolve();
    });
  });
}

export { SOCKET_PATH };
