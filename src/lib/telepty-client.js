/**
 * telepty WS Client Library for aterm
 * Connects to telepty daemon (localhost:3848) for session management.
 */

const DEFAULT_PORT = 3848;

export class TeleptClient {
  constructor(options = {}) {
    this.host = options.host || 'localhost';
    this.port = options.port || DEFAULT_PORT;
    this.token = options.token || '';
    // Always use proxy through aterm server (/telepty/* → telepty daemon /api/*)
    // This avoids CORS issues in browser
    if (typeof location !== 'undefined') {
      // Browser: use same-origin proxy
      this.baseUrl = '/telepty';
      const proto = location.protocol === 'https:' ? 'wss' : 'ws';
      this.wsUrl = `${proto}://${location.host}/telepty`;
    } else {
      // Node.js (CLI): connect directly
      this.baseUrl = `http://${this.host}:${this.port}/api`;
      this.wsUrl = `ws://${this.host}:${this.port}/api`;
    }
    this.busWs = null;
    this.sessionWsMap = new Map(); // sessionId → WebSocket
    this.listeners = new Map();
  }

  // ── Auth ──────────────────────────────────────────────────

  async loadToken() {
    // In browser context, fetch token from daemon meta endpoint
    try {
      const res = await fetch(`${this.baseUrl}/meta`);
      if (res.ok) {
        const meta = await res.json();
        this.token = meta.auth_token || meta.token || this.token;
      }
    } catch (e) { console.warn('[telepty] token load failed:', e); }
    return this.token;
  }

  headers() {
    return {
      'Content-Type': 'application/json',
      'x-telepty-token': this.token
    };
  }

  // ── Session Discovery ─────────────────────────────────────

  async getSessions() {
    const res = await fetch(`${this.baseUrl}/sessions`, { headers: this.headers() });
    return res.json();
  }

  async getSession(id) {
    const res = await fetch(`${this.baseUrl}/sessions/${encodeURIComponent(id)}`, { headers: this.headers() });
    return res.json();
  }

  // ── Inject / Broadcast ────────────────────────────────────

  async inject(sessionId, prompt, options = {}) {
    const body = { prompt, ...options };
    const res = await fetch(`${this.baseUrl}/sessions/${encodeURIComponent(sessionId)}/inject`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(body)
    });
    return res.json();
  }

  async broadcast(prompt) {
    const res = await fetch(`${this.baseUrl}/sessions/broadcast/inject`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify({ prompt })
    });
    return res.json();
  }

  async multicast(sessionIds, prompt) {
    const res = await fetch(`${this.baseUrl}/sessions/multicast/inject`, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify({ session_ids: sessionIds, prompt })
    });
    return res.json();
  }

  // ── Session Lifecycle ─────────────────────────────────────

  async deleteSession(id) {
    const res = await fetch(`${this.baseUrl}/sessions/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.headers()
    });
    return res.json();
  }

  // ── Event Bus (WS) ───────────────────────────────────────

  connectBus(onEvent) {
    const url = `${this.wsUrl}/bus?token=${encodeURIComponent(this.token)}`;
    this.busWs = new WebSocket(url);

    this.busWs.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        onEvent(msg);
        this.emit('bus', msg);
      } catch {}
    };

    this.busWs.onclose = () => {
      setTimeout(() => this.connectBus(onEvent), 3000);
    };

    this.busWs.onerror = () => {};
    return this.busWs;
  }

  disconnectBus() {
    if (this.busWs) { this.busWs.close(); this.busWs = null; }
  }

  // ── Session WS (attach) ──────────────────────────────────

  attachSession(sessionId, { onOutput, onClose } = {}) {
    const url = `${this.wsUrl}/sessions/${encodeURIComponent(sessionId)}?token=${encodeURIComponent(this.token)}`;
    const ws = new WebSocket(url);

    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type === 'output' && onOutput) onOutput(msg.data);
        if (msg.type === 'inject' && onOutput) onOutput(msg.data);
      } catch {}
    };

    ws.onclose = () => {
      this.sessionWsMap.delete(sessionId);
      if (onClose) onClose();
    };

    ws.onerror = () => {};
    this.sessionWsMap.set(sessionId, ws);
    return ws;
  }

  sendInput(sessionId, data) {
    const ws = this.sessionWsMap.get(sessionId);
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'input', data }));
    }
  }

  detachSession(sessionId) {
    const ws = this.sessionWsMap.get(sessionId);
    if (ws) { ws.close(); this.sessionWsMap.delete(sessionId); }
  }

  // ── Event emitter ────────────────────────────────────────

  on(event, fn) {
    if (!this.listeners.has(event)) this.listeners.set(event, []);
    this.listeners.get(event).push(fn);
  }

  off(event, fn) {
    const fns = this.listeners.get(event);
    if (fns) this.listeners.set(event, fns.filter(f => f !== fn));
  }

  emit(event, data) {
    const fns = this.listeners.get(event) || [];
    fns.forEach(fn => fn(data));
  }

  // ── Cleanup ──────────────────────────────────────────────

  destroy() {
    this.disconnectBus();
    for (const [id] of this.sessionWsMap) this.detachSession(id);
  }
}
