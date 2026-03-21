import { WebSocketServer } from 'ws';
import ptyManager from './pty-manager.js';
import { start, stop, SOCKET_PATH } from './socket-server.js';

const DEFAULT_WORKSPACE_ID = 'default';
const WS_PORT = 3849;

let wss = null;

// ── WebSocket helpers ────────────────────────────────────────────────────────

function send(ws, obj) {
  if (ws.readyState === ws.OPEN) {
    ws.send(JSON.stringify(obj));
  }
}

function broadcast(obj) {
  if (!wss) return;
  const msg = JSON.stringify(obj);
  for (const client of wss.clients) {
    if (client.readyState === client.OPEN) {
      client.send(msg);
    }
  }
}

// ── Command handler (same semantics as socket-server) ────────────────────────

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
      // Broadcast creation event to all WS clients
      broadcast({ event: 'created', workspace: id });
      return { ok: true, id };
    }

    case 'close-workspace': {
      if (!cmd.workspace) return { ok: false, error: 'Missing workspace id' };
      ptyManager.destroyWorkspace(cmd.workspace);
      broadcast({ event: 'closed', workspace: cmd.workspace });
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
        workspaces: workspaces.length,
        socketPath: SOCKET_PATH,
      };
    }

    default:
      return { ok: false, error: `Unknown command: '${cmd.cmd}'` };
  }
}

// ── Subscribe a WS client to a workspace's PTY output ───────────────────────

// Map: workspaceId -> Set of WS clients subscribed
const wsSubscriptions = new Map();

function subscribeToWorkspace(ws, workspaceId) {
  if (!wsSubscriptions.has(workspaceId)) {
    wsSubscriptions.set(workspaceId, new Set());
    // Attach pty output listener the first time anyone subscribes
    try {
      const unsub = ptyManager.onData(workspaceId, (data) => {
        const msg = JSON.stringify({ event: 'output', workspace: workspaceId, data });
        const subs = wsSubscriptions.get(workspaceId);
        if (subs) {
          for (const client of subs) {
            if (client.readyState === client.OPEN) client.send(msg);
          }
        }
      });
      // Store unsub so we can tear it down when last subscriber leaves
      wsSubscriptions.get(workspaceId)._unsub = unsub;
    } catch (err) {
      console.error(`[ws-server] subscribe error for ${workspaceId}:`, err.message);
    }
  }
  wsSubscriptions.get(workspaceId).add(ws);
}

function unsubscribeFromWorkspace(ws, workspaceId) {
  const subs = wsSubscriptions.get(workspaceId);
  if (!subs) return;
  subs.delete(ws);
  if (subs.size === 0) {
    if (subs._unsub) subs._unsub();
    wsSubscriptions.delete(workspaceId);
  }
}

function unsubscribeAll(ws) {
  for (const [workspaceId, subs] of wsSubscriptions.entries()) {
    if (subs.has(ws)) {
      subs.delete(ws);
      if (subs.size === 0) {
        if (subs._unsub) subs._unsub();
        wsSubscriptions.delete(workspaceId);
      }
    }
  }
}

// ── WebSocket server ─────────────────────────────────────────────────────────

function startWsServer() {
  return new Promise((resolve) => {
    wss = new WebSocketServer({ port: WS_PORT });

    wss.on('connection', (ws) => {
      ws.on('message', async (raw) => {
        let cmd;
        try {
          cmd = JSON.parse(raw.toString());
        } catch {
          send(ws, { ok: false, error: 'Invalid JSON' });
          return;
        }

        // Special: subscribe to workspace output stream
        if (cmd.cmd === 'subscribe') {
          if (!cmd.workspace) { send(ws, { ok: false, error: 'Missing workspace id' }); return; }
          subscribeToWorkspace(ws, cmd.workspace);
          send(ws, { ok: true, subscribed: cmd.workspace });
          return;
        }

        // Special: unsubscribe
        if (cmd.cmd === 'unsubscribe') {
          if (!cmd.workspace) { send(ws, { ok: false, error: 'Missing workspace id' }); return; }
          unsubscribeFromWorkspace(ws, cmd.workspace);
          send(ws, { ok: true, unsubscribed: cmd.workspace });
          return;
        }

        const response = await handleCommand(cmd);
        // Tag response with request id if provided
        if (cmd.id !== undefined) response.id = cmd.id;
        send(ws, response);
      });

      ws.on('close', () => {
        unsubscribeAll(ws);
      });

      ws.on('error', (err) => {
        if (err.code !== 'ECONNRESET') {
          console.error('[ws-server] client error:', err.message);
        }
      });
    });

    wss.on('listening', () => {
      console.log(`[ws-server] listening on ws://localhost:${WS_PORT}`);
      resolve();
    });

    wss.on('error', (err) => {
      console.error('[ws-server] error:', err.message);
      resolve(); // Don't hard-fail the whole server
    });
  });
}

function stopWsServer() {
  return new Promise((resolve) => {
    if (!wss) return resolve();
    wss.close(() => resolve());
  });
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function startServer() {
  await start();
  await startWsServer();

  // Create a default workspace on startup
  try {
    ptyManager.createWorkspace(DEFAULT_WORKSPACE_ID);
    console.log(`[aterm] default workspace created (id: ${DEFAULT_WORKSPACE_ID})`);
  } catch (err) {
    console.error('[aterm] failed to create default workspace:', err.message);
  }

  console.log(`[aterm] server ready`);
  console.log(`  Unix socket: ${SOCKET_PATH}`);
  console.log(`  WebSocket:   ws://localhost:${WS_PORT}`);
}

async function stopServer() {
  for (const ws of ptyManager.listWorkspaces()) {
    try { ptyManager.destroyWorkspace(ws.id); } catch (_) {}
  }
  await stopWsServer();
  await stop();
  console.log('[aterm] server stopped');
}

// Graceful shutdown
process.on('SIGINT', async () => {
  console.log('\n[aterm] received SIGINT, shutting down...');
  await stopServer();
  process.exit(0);
});

process.on('SIGTERM', async () => {
  console.log('[aterm] received SIGTERM, shutting down...');
  await stopServer();
  process.exit(0);
});

startServer().catch((err) => {
  console.error('[aterm] startup failed:', err);
  process.exit(1);
});

export { startServer, stopServer };
