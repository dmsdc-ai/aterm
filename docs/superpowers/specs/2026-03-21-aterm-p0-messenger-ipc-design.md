# aterm P0: Messenger IPC + CLI Design

## Summary

aterm P0 adds Unix socket IPC and a messenger-style CLI to aterm while preserving its messenger-type CLI terminal identity. Instead of replicating cmux's terminal-centric approach, we selectively adopt cmux's architectural advantages (socket IPC, workspace hierarchy) through a messenger lens.

**Key principle:** aterm is a messenger view; telepty is the PTY owner.

## Background

### Deliberation Consensus (2026-03-21)
Two deliberations informed this design:
1. **Cross-machine module** — SSH tunnel + WS relay hybrid for cross-machine communication
2. **aterm identity vs cmux** — 4 sessions unanimously agreed: keep messenger identity, adopt only compatible cmux patterns

### What We Take From cmux
| cmux Pattern | Adopted? | How |
|---|---|---|
| Unix socket IPC | Yes | telepty daemon exposes socket endpoint |
| Workspace hierarchy | Yes | Maps to messenger channels/DMs |
| CLI control | Partially | Wrapped as `@session message` messenger syntax |
| PTY direct ownership | No | telepty owns PTY, aterm attaches |

### What We Reject
- Tauri/Rust tech stack switch (Svelte+xterm.js sufficient, 0MB install)
- cmux-style verbose CLI flags (`--workspace --surface`)
- PTY ownership in aterm (conflicts with telepty)

## Architecture

```
+-------------------------------------+
|          aterm (Svelte webapp)       |
|  SessionTree | Timeline | Terminal  |
|         ^ WebSocket                 |
+---------+---------------------------+
|        telepty daemon               |
|  +---------+----------------------+ |
|  | WS API  |  Unix Socket API     | |
|  | :3848   |  ~/.telepty/aterm.sock| |
|  +---------+----------------------+ |
|        ^ PTY ownership/management   |
+---------+---------------------------+
|     aterm CLI (Node.js thin wrapper)|
|  aterm send @session "message"      |
|         ^ Unix Socket               |
+-----------------------------------------+
```

### Change Scope
- **telepty daemon**: Add Unix socket endpoint (~100 lines)
- **aterm CLI**: Node.js thin wrapper (`bin/aterm`) (~150 lines)
- **aterm Svelte**: @session input mode in CommandPalette (~100 lines)

## Component Details

### 1. telepty Unix Socket Server

**Location:** `~/.telepty/aterm.sock`

Exposes the same handlers as the existing WS/HTTP API over a Unix domain socket.

**Protocol:** Newline-delimited JSON (`\n` separator). Each request is a single JSON line; each response is a single JSON line.

**Commands:**

| Command | Request | Response | Maps To |
|---|---|---|---|
| `send` | `{"cmd":"send","session":"id","text":"msg"}` | `{"ok":true}` | telepty inject |
| `send-key` | `{"cmd":"send-key","session":"id","key":"return"}` | `{"ok":true}` | PTY write |
| `read-screen` | `{"cmd":"read-screen","session":"id","lines":50}` | `{"lines":[...]}` | PTY buffer read |
| `list` | `{"cmd":"list"}` | `{"sessions":[...]}` | session list |
| `status` | `{"cmd":"status"}` | `{"daemon":...,"sessions":[...]}` | health check |

**Error format:** `{"ok":false,"error":"message"}`

**Security:** Unix socket file permissions (0600) — only the owning user can connect.

### 2. aterm CLI

**Location:** `aigentry-aterm/bin/aterm` (Node.js, single file)

**Install:** `npm link` or add to PATH

**Messenger-style syntax:**

```bash
# Send message to session (primary UX)
aterm send @aigentry-telepty-claude "build result please"
aterm send @all "announcement"           # broadcast

# Read screen
aterm read-screen @aigentry-telepty-claude
aterm read-screen @aigentry-telepty-claude --lines 20

# Status
aterm status                              # all sessions summary
aterm list                                # session list

# Key sending
aterm send-key @aigentry-telepty-claude return
aterm send-key @aigentry-telepty-claude ctrl+c
```

**Implementation:**
- `net.connect()` to `~/.telepty/aterm.sock`
- Send JSON request, read JSON response
- `@all` → iterate all sessions
- `@` prefix is required for session targeting (distinguishes from raw text)

### 3. Svelte Frontend Changes

**@session input mode in CommandPalette:**
- Typing `@` in input shows session autocomplete dropdown
- `@session message` + Enter → calls telepty inject via WS
- `Ctrl+Space` toggles between messenger mode and terminal mode
- Messenger mode: input acts like chat input (send to session)
- Terminal mode: input acts like normal terminal (send to PTY)

**Visual indicators:**
- Input bar shows mode badge: `[Messenger]` or `[Terminal]`
- `@` autocomplete shows session status (active/idle)

## Role Boundaries

| Domain | telepty (Owner) | aterm (View) |
|---|---|---|
| PTY lifecycle | create/destroy/resize/signal | attach/detach/display |
| Session management | register/discover/route | tree display/selection |
| Message delivery | inject/reply protocol | @session wrapping UI |
| Unix socket | socket server | CLI client |
| Cross-machine | SSH tunnel + WS relay | display remote indicator |

## Testing Strategy

1. **Socket IPC**: Unit test — connect, send command, verify response
2. **CLI**: Integration test — `aterm send @test "hello"` → verify inject received
3. **Svelte @session**: Manual test — type `@session msg`, verify inject
4. **Reconnection**: Kill telepty, restart, verify aterm CLI auto-reconnects

## Risks and Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Socket reconnection on telepty restart | Medium | Heartbeat + auto-reconnect in CLI |
| PTY resize IPC latency (~1-2ms) | Low | Acceptable for xterm.js 16ms frame budget |
| @session syntax collision with terminal commands | Low | Mode toggle (Ctrl+Space) or prefix detection |

## Out of Scope (Future)

- Tauri native app wrapping (Phase 2, if PMF confirmed)
- Deliberation Theater panel
- Brain memory widget
- Cross-machine PTY attach (depends on cross-machine module completion)
