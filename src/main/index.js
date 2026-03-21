import { app, BrowserWindow, ipcMain, shell } from 'electron';
import { join } from 'path';
import { electronApp, optimizer, is } from '@electron-toolkit/utils';
import ptyManager from '../server/pty-manager.js';
import { start as startSocket, stop as stopSocket, SOCKET_PATH } from '../server/socket-server.js';

const DEFAULT_WORKSPACE_ID = 'default';

// Track per-window subscriptions: windowId -> Map<workspaceId, unsubFn>
const windowSubscriptions = new Map();

function createWindow() {
  const mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 800,
    minHeight: 500,
    show: false,
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 12, y: 14 },
    backgroundColor: '#0d1117',
    webPreferences: {
      preload: join(__dirname, '../preload/index.mjs'),
      sandbox: false,
    },
  });

  mainWindow.on('ready-to-show', () => {
    mainWindow.show();
  });

  mainWindow.webContents.setWindowOpenHandler((details) => {
    shell.openExternal(details.url);
    return { action: 'deny' };
  });

  // Track subscriptions for this window
  const winId = mainWindow.id;
  windowSubscriptions.set(winId, new Map());

  mainWindow.on('closed', () => {
    // Clean up PTY subscriptions for this window
    const subs = windowSubscriptions.get(winId);
    if (subs) {
      for (const unsub of subs.values()) {
        unsub();
      }
      windowSubscriptions.delete(winId);
    }
  });

  // Load the renderer
  if (is.dev && process.env['ELECTRON_RENDERER_URL']) {
    mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL']);
  } else {
    mainWindow.loadFile(join(__dirname, '../renderer/index.html'));
  }

  return mainWindow;
}

// ── IPC Handlers ──────────────────────────────────────────────────────────────

function generateWorkspaceId() {
  return `ws-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

function setupIPC() {
  ipcMain.handle('pty:send', (_event, wsId, text) => {
    ptyManager.sendToWorkspace(wsId, text);
    return { ok: true };
  });

  ipcMain.handle('pty:sendKey', (_event, wsId, key) => {
    ptyManager.sendKeyToWorkspace(wsId, key);
    return { ok: true };
  });

  ipcMain.handle('pty:readScreen', (_event, wsId, lines) => {
    const result = ptyManager.readScreen(wsId, lines ?? 50);
    return { ok: true, lines: result };
  });

  ipcMain.handle('pty:listWorkspaces', () => {
    const list = ptyManager.listWorkspaces();
    return { ok: true, workspaces: list };
  });

  ipcMain.handle('pty:newWorkspace', (_event, opts = {}) => {
    const id = opts.id || generateWorkspaceId();
    ptyManager.createWorkspace(id, {
      cwd: opts.cwd,
      command: opts.command,
      cols: opts.cols,
      rows: opts.rows,
    });
    // Notify all renderer windows about new workspace
    for (const win of BrowserWindow.getAllWindows()) {
      win.webContents.send('pty:event', { event: 'created', workspace: id });
    }
    return { ok: true, id };
  });

  ipcMain.handle('pty:closeWorkspace', (_event, id) => {
    ptyManager.destroyWorkspace(id);
    // Notify all renderer windows
    for (const win of BrowserWindow.getAllWindows()) {
      win.webContents.send('pty:event', { event: 'closed', workspace: id });
    }
    return { ok: true };
  });

  ipcMain.handle('pty:resize', (_event, wsId, cols, rows) => {
    ptyManager.resizeWorkspace(wsId, cols, rows);
    return { ok: true };
  });

  ipcMain.handle('pty:status', () => {
    const list = ptyManager.listWorkspaces();
    return {
      ok: true,
      workspaces: list.length,
      socketPath: SOCKET_PATH,
    };
  });

  // Subscribe to workspace output — sends data to the requesting renderer window
  ipcMain.on('pty:subscribe', (event, wsId) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win) return;

    const winId = win.id;
    const subs = windowSubscriptions.get(winId);
    if (!subs) return;

    // Already subscribed?
    if (subs.has(wsId)) return;

    try {
      const unsub = ptyManager.onData(wsId, (data) => {
        if (!win.isDestroyed()) {
          win.webContents.send('pty:output', { workspace: wsId, data });
        }
      });
      subs.set(wsId, unsub);
    } catch (err) {
      console.error(`[main] subscribe error for ${wsId}:`, err.message);
    }
  });

  ipcMain.on('pty:unsubscribe', (event, wsId) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win) return;

    const winId = win.id;
    const subs = windowSubscriptions.get(winId);
    if (!subs) return;

    const unsub = subs.get(wsId);
    if (unsub) {
      unsub();
      subs.delete(wsId);
    }
  });
}

// ── App Lifecycle ─────────────────────────────────────────────────────────────

app.whenReady().then(async () => {
  // Set app user model id for windows
  electronApp.setAppUserModelId('com.aigentry.aterm');

  // Default open or close DevTools by F12 in dev, ignore in production
  app.on('browser-window-created', (_, window) => {
    optimizer.watchWindowShortcuts(window);
  });

  // Start Unix socket server for CLI access
  try {
    await startSocket();
    console.log(`[aterm] Unix socket: ${SOCKET_PATH}`);
  } catch (err) {
    console.error('[aterm] socket server failed:', err.message);
  }

  // Create default workspace
  try {
    ptyManager.createWorkspace(DEFAULT_WORKSPACE_ID);
    console.log(`[aterm] default workspace created (id: ${DEFAULT_WORKSPACE_ID})`);
  } catch (err) {
    console.error('[aterm] failed to create default workspace:', err.message);
  }

  // Set up IPC handlers
  setupIPC();

  // Create the main window
  createWindow();

  app.on('activate', () => {
    // On macOS re-create window when dock icon clicked and no windows exist
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  // On macOS, keep app running even when all windows are closed
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('before-quit', async () => {
  // Clean up all PTY processes
  for (const ws of ptyManager.listWorkspaces()) {
    try {
      ptyManager.destroyWorkspace(ws.id);
    } catch (_) {}
  }
  // Stop socket server
  await stopSocket();
  console.log('[aterm] cleanup complete');
});
