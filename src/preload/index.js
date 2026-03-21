import { contextBridge, ipcRenderer } from 'electron';
import { electronAPI } from '@electron-toolkit/preload';

// Expose aterm-specific APIs to the renderer via contextBridge
const atermAPI = {
  // PTY commands (invoke = request/response)
  send: (wsId, text) => ipcRenderer.invoke('pty:send', wsId, text),
  sendKey: (wsId, key) => ipcRenderer.invoke('pty:sendKey', wsId, key),
  readScreen: (wsId, lines) => ipcRenderer.invoke('pty:readScreen', wsId, lines),
  listWorkspaces: () => ipcRenderer.invoke('pty:listWorkspaces'),
  newWorkspace: (opts) => ipcRenderer.invoke('pty:newWorkspace', opts),
  closeWorkspace: (id) => ipcRenderer.invoke('pty:closeWorkspace', id),
  resize: (wsId, cols, rows) => ipcRenderer.invoke('pty:resize', wsId, cols, rows),
  status: () => ipcRenderer.invoke('pty:status'),

  // PTY output subscription (one-way push from main)
  onOutput: (callback) => {
    const handler = (_event, data) => callback(data);
    ipcRenderer.on('pty:output', handler);
    // Return cleanup function
    return () => ipcRenderer.removeListener('pty:output', handler);
  },

  // PTY lifecycle events (workspace created/closed)
  onEvent: (callback) => {
    const handler = (_event, data) => callback(data);
    ipcRenderer.on('pty:event', handler);
    return () => ipcRenderer.removeListener('pty:event', handler);
  },

  // Subscribe/unsubscribe to workspace output stream
  subscribe: (wsId) => ipcRenderer.send('pty:subscribe', wsId),
  unsubscribe: (wsId) => ipcRenderer.send('pty:unsubscribe', wsId),
};

// Use contextBridge APIs to expose Electron APIs to renderer
// only if context isolation is enabled, otherwise just add to global
if (process.contextIsolated) {
  try {
    contextBridge.exposeInMainWorld('electron', electronAPI);
    contextBridge.exposeInMainWorld('atermAPI', atermAPI);
  } catch (error) {
    console.error(error);
  }
} else {
  window.electron = electronAPI;
  window.atermAPI = atermAPI;
}
