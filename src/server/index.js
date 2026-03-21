import http from 'http';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { WebSocketServer } from 'ws';
import ptyManager from './pty-manager.js';
import { start, stop, SOCKET_PATH } from './socket-server.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DIST_DIR = path.resolve(__dirname, '../../dist');
// No default workspace — user creates sessions via + button
const PORT = 3849;

let wss = null;
let httpServer = null;

// ── MIME types for static file serving ──────────────────────────────────────

const MIME_TYPES = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
};

function handleRequest(req, res) {
  // POST /api/create-session — create a workspace with claude via node-pty
  if (req.url === '/api/create-session' && req.method === 'POST') {
    let body = '';
    req.on('data', chunk => { body += chunk; });
    req.on('end', () => {
      let parsed;
      try { parsed = JSON.parse(body); } catch {
        res.writeHead(400, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: 'Invalid JSON' }));
        return;
      }
      const { cwd, command, args } = parsed;
      if (!cwd) {
        res.writeHead(400, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: 'Missing cwd' }));
        return;
      }
      // Generate id from folder name
      const folderName = cwd.replace(/\/+$/, '').split('/').pop() || 'session';
      const id = parsed.id || `${folderName}-${Date.now().toString(36)}`;
      const safeId = id.replace(/[^a-zA-Z0-9_\-\.]/g, '');
      try {
        ptyManager.createWorkspace(safeId, {
          cwd,
          command: command || undefined,
          args: args || undefined,
        });
        broadcast({ event: 'created', workspace: safeId });
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: true, id: safeId }));
      } catch (err) {
        res.writeHead(500, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: false, error: err.message }));
      }
    });
    return;
  }

  // GET /api/suggest-paths?q=... — return matching subdirs under ~/projects
  if (req.url.startsWith('/api/suggest-paths') && req.method === 'GET') {
    const urlObj = new URL(req.url, `http://localhost`);
    const q = (urlObj.searchParams.get('q') || '').trim();
    const homeDir = process.env.HOME || '/root';

    // If query looks like an absolute path, list children of the deepest existing dir
    if (q.startsWith('/') || q.startsWith('~')) {
      const expanded = q.startsWith('~') ? q.replace('~', homeDir) : q;
      // List children of parent dir whose name starts with last segment
      const parts = expanded.replace(/\/+$/, '').split('/');
      const base = parts.slice(0, -1).join('/') || '/';
      const prefix = parts[parts.length - 1] || '';
      fs.readdir(base, { withFileTypes: true }, (err, entries) => {
        if (err) {
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ paths: [] }));
          return;
        }
        const matches = entries
          .filter(e => e.isDirectory() && e.name.startsWith(prefix) && !e.name.startsWith('.'))
          .slice(0, 10)
          .map(e => path.join(base, e.name).replace(homeDir, '~'));
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ paths: matches }));
      });
    } else {
      // Default: list ~/projects/* matching query
      const projectsDir = path.join(homeDir, 'projects');
      fs.readdir(projectsDir, { withFileTypes: true }, (err, entries) => {
        if (err) {
          res.writeHead(200, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ paths: [] }));
          return;
        }
        const lower = q.toLowerCase();
        const matches = entries
          .filter(e => e.isDirectory() && e.name.toLowerCase().includes(lower) && !e.name.startsWith('.'))
          .slice(0, 10)
          .map(e => `~/projects/${e.name}`);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ paths: matches }));
      });
    }
    return;
  }

  return serveStatic(req, res);
}

function serveStatic(req, res) {
  let urlPath = req.url.split('?')[0];
  if (urlPath === '/') urlPath = '/index.html';

  const filePath = path.join(DIST_DIR, urlPath);
  // Prevent directory traversal
  if (!filePath.startsWith(DIST_DIR)) {
    res.writeHead(403);
    res.end('Forbidden');
    return;
  }

  fs.readFile(filePath, (err, data) => {
    if (err) {
      // SPA fallback: serve index.html for any non-file route
      fs.readFile(path.join(DIST_DIR, 'index.html'), (err2, indexData) => {
        if (err2) {
          res.writeHead(404);
          res.end('Not found');
          return;
        }
        res.writeHead(200, { 'Content-Type': 'text/html' });
        res.end(indexData);
      });
      return;
    }
    const ext = path.extname(filePath);
    const mime = MIME_TYPES[ext] || 'application/octet-stream';
    res.writeHead(200, { 'Content-Type': mime });
    res.end(data);
  });
}

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
    httpServer = http.createServer(handleRequest);
    wss = new WebSocketServer({ noServer: true });

    httpServer.on('upgrade', (req, socket, head) => {
      wss.handleUpgrade(req, socket, head, (ws) => {
        wss.emit('connection', ws, req);
      });
    });

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

    httpServer.listen(PORT, () => {
      const hasDist = fs.existsSync(path.join(DIST_DIR, 'index.html'));
      console.log(`[aterm] listening on http://localhost:${PORT}`);
      if (hasDist) {
        console.log(`[aterm] serving UI from ${DIST_DIR}`);
      } else {
        console.log(`[aterm] no dist/ found — run 'npm run build' for UI. WS-only mode.`);
      }
      resolve();
    });

    httpServer.on('error', (err) => {
      console.error('[aterm] http server error:', err.message);
      resolve();
    });
  });
}

function stopWsServer() {
  return new Promise((resolve) => {
    if (!wss) return resolve();
    wss.close(() => {
      if (httpServer) {
        httpServer.close(() => resolve());
      } else {
        resolve();
      }
    });
  });
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function startServer() {
  await start();
  await startWsServer();

  console.log(`[aterm] server ready (no default workspace — use + to create sessions)`);
  console.log(`  Unix socket: ${SOCKET_PATH}`);
  console.log(`  HTTP + WS:   http://localhost:${PORT}`);
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
