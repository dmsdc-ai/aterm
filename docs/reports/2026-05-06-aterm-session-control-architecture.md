# Aterm Session Control Architecture Deep-Analysis (2026-05-06)

> Architect role · read-only · NO code change · NO ADR commit · grill-feed input.
> Source SAWP: `/tmp/aigentry-dispatch/aterm-session-control-architecture.md`.
> Cross-checked: aterm-core/src, aterm-ipc, aterm-session, bin/aterm.js, cmux/CLI, aigentry-telepty/{cli.js,daemon.js,terminal-backend.js}.

---

## TL;DR

1. **aterm v3 is NOT a viewer.** It is a fully self-hosting terminal: `aterm-core` owns its PTY (Model B), exposes a typed `SessionAction` IPC surface (`aterm-session/src/action.rs:5–80`), and ships a thin Node CLI (`bin/aterm.js`, 242 LOC) that already covers every orchestrator-daily verb (list/new/close/inject/send/send-key/read-screen).
2. **Model A (telepty-owned PTY) is scaffolding only.** Bridge POSTs registration metadata; `TeleptyBusClient` (Swift) subscribes to status events; **no PTY-byte stream from telepty bus to aterm renderer exists.** `AttachExternal` is wired down to a Swift host callback (`lib.rs:2499/2563`) but does not pump bytes into the alacritty `Term`.
3. **5/7 cmux daily verbs are covered by telepty CLI; aterm itself covers ALL 7** (incl. proper `send-key` with arbitrary keysym, which telepty lacks).
4. **Architectural debt is concentrated, not diffuse.** Workspace root `aterm-v3` bin points at non-existent `src-v3/main.rs`; `aterm-core/src/renderer*.rs` (~3621 LOC) is gated by an undefined `wgpu` feature; `archived/src-v3-future` and `archived/src-tauri-v1` linger as repo-history references.
5. **Recommendation: opt-3-prime** (Model B as primary, Model A demoted to read-only discovery via `telepty list` + bus). Effort ≈ 1.5 person-weeks, gated on dead-code cleanup. Aligns with Art.1 (경량 — after cleanup), Art.9 (독립), Art.13 (객관적), and Y1 + memory feedback `aterm > cmux 우선`. Avoids Art.3 violation that opt-2 would trigger.

---

## §1 Internal Architecture Map (Task 1)

### 1.1 Crate / process topology

```
        ┌────────────────────────────────────────────────────────────┐
        │ aterm.app (.app bundle, codesigned)                        │
        │  ┌──────────────────────────────────────────────────────┐  │
        │  │ Swift / AppKit shell                                 │  │
        │  │  main.swift → AppDelegate → TerminalView (Metal)    │  │
        │  │              ├ SessionSidebarView                    │  │
        │  │              ├ OrchestratorInputBar                  │  │
        │  │              └ TeleptyBusClient (WS :3848 listener) │  │
        │  └────────────────┬──────────────────────────────────────┘  │
        │                   │ FFI (aterm_core.h, cbindgen-generated) │
        │  ┌────────────────▼──────────────────────────────────────┐  │
        │  │ libaterm_core.dylib  (cdylib)                        │  │
        │  │  lib.rs → AtermCore + FFI surface                    │  │
        │  │       ├ app.rs (EventBus, dispatcher)                │  │
        │  │       ├ pty.rs (PtyManager · portable-pty)           │  │
        │  │       ├ terminal.rs (alacritty Term + OSC133)        │  │
        │  │       ├ inject.rs (idle-FSM queue, mailbox)          │  │
        │  │       ├ session.rs (workspace persistence)           │  │
        │  │       ├ telepty_bridge.rs (HTTP curl, fire-forget)   │  │
        │  │       ├ telepty.rs (subprocess CLI wrapper)          │  │
        │  │       ├ tailscale.rs (tsnet)                         │  │
        │  │       ├ mailbox/* (file-backed ACK)                  │  │
        │  │       └ renderer*.rs ← #[cfg(feature="wgpu")] DEAD   │  │
        │  └──────────────────────────────────────────────────────┘  │
        └───────────┬─────────────────────────────────────┬──────────┘
                    │ UnixSocket /tmp/aterm-{pid}.sock    │ stdout/stdin
                    │ aterm-ipc::server (252 LOC, peer-uid auth)
                    │
        ┌───────────▼──────────┐                ┌────────▼────────┐
        │ bin/aterm.js (242)   │                │ child PTY procs │
        │ aterm CLI            │                │ (claude, codex, │
        │  send/send-key/      │                │  bash, …)       │
        │  read-screen/list/   │                └─────────────────┘
        │  new/close/status    │
        └──────────────────────┘
                    ▲
                    │ executed by user / orchestrator inject
```

`aterm-v3` workspace bin (root `Cargo.toml:10-12`) points at `src-v3/main.rs` which does **not exist** (`ls /Users/.../src-v3` → `No such file or directory`). The root crate compiles only because `cargo build` defaults to the cdylib path of `aterm-core`. Workspace deps wgpu/glyphon/winit at the root are unused.

### 1.2 Two PTY models — full traces

#### Model A — telepty-owned (scaffolding · 0 % runtime use)

```
USER cmd line:
  open-session.sh → telepty allow --id <name> <cli>
                    │
                    ▼
  telepty daemon (:3848, Node)
    ├─ spawns wrapped child via node-pty
    ├─ owns master FD, runs prompt-FSM
    └─ broadcasts BusEvent JSON over WS /api/bus

aterm side (read-only):
  AppDelegate.applicationDidFinishLaunching
    → TeleptyBusClient.connect()              TeleptyBusClient.swift:111
    → WS subscribe → handleMessage()          TeleptyBusClient.swift:263–272
    → BusEvent (transport+semantic)
    → SwiftUI @Published [TeleptySession]
    → SessionSidebarView pill (status only)    SessionSidebarView.swift

  SessionAction::AttachExternal { session_id }   action.rs:59–62
    → app.dispatch                              app.rs:851–860
    → AtermHostCallbacks.attach_external_session lib.rs:2499/2563
    → Swift host callback              ⛔ NO PTY-byte stream wired
                                       ⛔ alacritty Term receives nothing
```

**Conclusion:** Model A is a **status mirror**. No path on `BusEvent` carries PTY bytes; no path turns those into `Term::advance_bytes()` calls. `AttachExternal` resolves to a Swift hook that, today, does nothing observable in the renderer. The bus client is a sidebar-only pill source.

#### Model B — aterm-internal (100 % runtime use)

```
INPUT  (NSEvent.keyDown, NSTextInputClient)
  TerminalView.swift:693  keyDown / interpretKeyEvents
  → aterm_core_write_pty(core, ptr, len)         lib.rs:994–1008
  → AtermCore::write_pty                         lib.rs:495–504
  → PtyManager::send_to_workspace                pty.rs:566–580
  → workspace.writer.write_all                   pty.rs (writer Mutex)
  → portable-pty slave → child shell

REGISTER
  AppDelegate.createWorkspace                    AppDelegate.swift:1515
  → TerminalView.preSpawnInBackground            TerminalView.swift:1151
  → aterm_core_spawn_shell                       lib.rs:965–991
  → AtermCore::spawn_shell                       lib.rs:417–493
  → PtyManager::create                           pty.rs:260–463
        ├ portable-pty spawn
        ├ reader thread (loop)
        ├ InjectQueue per workspace
        └ register with global app                app.rs:240–281
            └ telepty_bridge.register() (best-effort POST)

OUTPUT  (PTY → render)
  reader thread reads master                     pty.rs:247–289
  → Term::advance_bytes (FairMutex)              terminal.rs (parking_lot)
  → alacritty VTE parser updates grid
  → PtyOutputSignal::mark_dirty                  pty.rs:69–83
  → wake_callback (set via aterm_core_set_dirty_callback)
  → DispatchQueue.main async
  → CVDisplayLink frame
  → MetalRenderer.swift:47 cell_text pipeline
  → aterm_core_get_render_data (lock + flatten)  lib.rs:1360–1490
  → CellDataFFI[] → MTLBuffer
  → CAMetalLayer present

INJECT  (orchestrator → session text)
  $ aterm inject <ws> "..."
  → bin/aterm.js → JSON over UnixSocket           aterm-ipc/server.rs
  → app.dispatch SessionAction::Inject            app.rs:319–360
  → InjectQueue.enqueue                           inject.rs:90–150
  → idle FSM (OSC133 / pattern heuristics)        inject.rs:13–37
  → run_injector_loop writes to PTY               inject.rs:197–400
  → EventBus::publish InjectDelivered             app.rs:53–68
  → Swift drain → UI update
```

### 1.3 Component reference table

Legend: ✅ active in this model · ⚠️ scaffolding only · ❌ unused · 💀 dead code (never compiled or unreachable)

| # | Component | Path | LOC | Owner | Model A | Model B | Status |
|---|---|---|---:|---|:---:|:---:|---|
| 1 | `PtyManager` (spawn + reader + writer + signal) | `aterm-core/src/pty.rs` | 2290 | Rust | ❌ | ✅ | active |
| 2 | `Terminal` wrapper + `PromptMarkStore` | `aterm-core/src/terminal.rs` | 754 | Rust | ❌ | ✅ | active |
| 3 | `InjectQueue` + idle FSM | `aterm-core/src/inject.rs` | 1155 | Rust | ❌ | ✅ | active |
| 4 | `app::Aterm` dispatcher + EventBus | `aterm-core/src/app.rs` | 1018 | Rust | ⚠️ (AttachExternal hook) | ✅ | active |
| 5 | `SessionStore` (workspaces.json persistence) | `aterm-core/src/session.rs` | 504 | Rust | ❌ | ✅ | active |
| 6 | `TeleptyBridge` (HTTP curl) | `aterm-core/src/telepty_bridge.rs` | 358 | Rust | ⚠️ register-only | ⚠️ register-only | partial |
| 7 | `TeleptyClient` (subprocess `telepty list --json`) | `aterm-core/src/telepty.rs` | 188 | Rust | ⚠️ | ⚠️ | partial · used by sidebar discovery |
| 8 | `tailscale` (`tsnet`) | `aterm-core/src/tailscale.rs` | 516 | Rust | ❌ | ✅ (cross-machine) | active |
| 9 | `mailbox/*` (file-backed ACK) | `aterm-core/src/mailbox/` | 905 | Rust | ❌ | ✅ | active |
| 10 | `sync` (FairMutex port) | `aterm-core/src/sync.rs` | 49 | Rust | ❌ | ✅ | active |
| 11 | `cli_presets` | `aterm-core/src/cli_presets.rs` | 46 | Rust | ❌ | ✅ | active |
| 12 | `lib.rs` FFI surface | `aterm-core/src/lib.rs` | 2918 | Rust | ⚠️ (AttachExternal stub) | ✅ | active (some `#[cfg(feature="wgpu")]` blocks 💀) |
| 13 | `renderer.rs` (wgpu instanced cell) | `aterm-core/src/renderer.rs` | 2613 | Rust | n/a | n/a | 💀 (no `wgpu` feature in Cargo.toml) |
| 14 | `renderer_atlas.rs` | `aterm-core/src/renderer_atlas.rs` | 385 | Rust | n/a | n/a | 💀 same gate |
| 15 | `renderer_glyph.rs` (glyphon FontSystem) | `aterm-core/src/renderer_glyph.rs` | 623 | Rust | n/a | n/a | 💀 same gate |
| 16 | `aterm-ipc::server` (UnixSocket, peer-uid auth) | `aterm-ipc/src/server.rs` | 252 | Rust | ❌ | ✅ | active · primary CLI surface |
| 17 | `aterm-ipc::auth` | `aterm-ipc/src/auth.rs` | 45 | Rust | ❌ | ✅ | active |
| 18 | `SessionAction` enum | `aterm-session/src/action.rs` | 80 | Rust | ⚠️ (AttachExternal) | ✅ | active |
| 19 | Swift `TerminalView` (NSTextInputClient + Metal driver) | `macos/Sources/TerminalView.swift` | 1404 | Swift | ❌ | ✅ | active |
| 20 | Swift `MetalRenderer` (cell pipeline) | `macos/Sources/MetalRenderer.swift` | 768 | Swift | ❌ | ✅ | active |
| 21 | Swift `GlyphAtlas` (CoreText raster + LRU) | `macos/Sources/GlyphAtlas.swift` | 668 | Swift | ❌ | ✅ | active |
| 22 | Swift `SessionSidebarView` | `macos/Sources/SessionSidebarView.swift` | 1573 | Swift | ⚠️ pill | ✅ | active |
| 23 | Swift `TeleptyBusClient` (WS :3848) | `macos/Sources/TeleptyBusClient.swift` | 342 | Swift | ⚠️ status | ⚠️ status | partial · sidebar pill source |
| 24 | Swift `OrchestratorInputBar` + Commands + History | `macos/Sources/Orchestrator*.swift` | 81+317+653 | Swift | ❌ | ✅ | active |
| 25 | Swift `AppDelegate` | `macos/Sources/AppDelegate.swift` | 2522 | Swift | ⚠️ AttachExternal handler | ✅ | active |
| 26 | `bin/aterm.js` (Node CLI client) | `bin/aterm.js` | 242 | Node | ❌ | ✅ | active |
| 27 | Workspace root bin `aterm-v3` | `Cargo.toml:10-12` → `src-v3/main.rs` | n/a | Rust | n/a | n/a | 💀 src-v3/ does not exist |
| 28 | Workspace root deps wgpu/glyphon/winit/pollster | `Cargo.toml:13-25` | n/a | Rust | n/a | n/a | 💀 root bin dead |
| 29 | `archived/src-v3-future/main.rs` | `archived/` | n/a | Rust | n/a | n/a | 💀 archived |
| 30 | `archived/src-tauri-v1/src/main.rs` | `archived/` | n/a | Rust | n/a | n/a | 💀 archived |

### 1.4 Top 5 architectural smells (verified)

1. **Ghost workspace bin** — `[[bin]] aterm-v3` in root `Cargo.toml:10-12` references `src-v3/main.rs` (absent dir, archived under `archived/src-v3-future/main.rs`). Compiles silently because `cargo build -p aterm-core` is the build path. Wastes `cargo metadata` cycles, confuses readers.
2. **Dead wgpu trio** — `aterm-core/src/{renderer,renderer_atlas,renderer_glyph}.rs` (3 621 LOC, 30+ `#[cfg(feature = "wgpu")]` gates in `lib.rs:56–907`) compile only if a `wgpu` feature is set; aterm-core's `Cargo.toml` has no `[features]` section → never set. Active GPU path is Swift `MetalRenderer.swift`.
3. **Dual telepty integration paths, both partial** — `TeleptyBridge` (Rust, HTTP curl) registers workspaces and never reads back; `TeleptyBusClient` (Swift, WS) reads status only; neither pumps PTY bytes. The `AttachExternal` SessionAction is a Swift host-callback no-op (`lib.rs:2499/2563`).
4. **`bin/aterm.js` is a thin client; protocol surface lives in Rust** — clean separation but means cmux-style commands (`new`, `close`, `ls`) are aliased only in Node; protocol-level docs for the IPC are sparse outside `action.rs`.
5. **FairMutex on `Term` shared between reader thread and main-thread render** — works today (synchronized output suppresses partial frames since fix #157/#201) but contention risk on high-volume output streams (`npm install`-style progress bursts). Not blocking for this analysis.

### 1.5 Is Model B activatable from outside aterm? → Yes, already.

`bin/aterm.js` ships with: `send`, `send-key`, `read-screen`, `list-workspaces`/`ls`, `new-workspace`/`new`, `close-workspace`/`close`, `status`. This is the orchestrator-daily set delivered via `aterm-ipc` UnixSocket → `app.dispatch(SessionAction::*)`. **No telepty involvement** in the CLI path. Telepty is only consulted for **discovery of sessions hosted in OTHER terminals** (ghostty, kitty), via `telepty list`/bus, surfaced in the sidebar.

---

## §2 Session Control Surface Gaps (Task 2)

### 2.1 Three-way command parity

Sources: cmux `CLI/cmux.swift:1539–7000` help block (sub-agent inventory) · telepty `cli.js:833–3050` · aterm `aterm-session/src/action.rs:5–80` + `bin/aterm.js`.

| Orchestrator-daily verb | cmux | telepty CLI | aterm CLI / SessionAction | aterm gap? |
|---|:---:|:---:|:---:|:---:|
| list-workspaces / list | `list-workspaces` | `list` (`cli.js:1530+`) | `aterm ls` → `ListWorkspaces` | ✅ |
| new-workspace / spawn | `new-workspace` | `spawn` / `allow` | `aterm new` → `CreateWorkspace` | ✅ |
| close-workspace / kill | `close-workspace` | `delete` | `aterm close` → `CloseWorkspace` | ✅ |
| inject text+enter | (`send` + `send-key enter`) | `inject` | `aterm inject` → `Inject{text,from,force}` | ✅ |
| send text only (no Enter) | `send` | `inject` w/o final newline | `aterm send` (Inject text only) | ✅ |
| send-key (arbitrary keysym) | `send-key` (rich) | `send-key enter` only ⚠️ | `aterm send-key` → `SendKey{key:String}` (any) | ✅ richer than telepty |
| read-screen | `read-screen` / `capture-pane` | `read-screen` (`cli.js`) | `aterm read-screen` → `ReadScreenText` | ✅ |
| identify session | `identify` | `session info` | (workspace metadata via `WorkspaceStatus`/`ListWorkspaces` data) | ⚠️ no dedicated cmd |
| focus / select | `focus-window`, `select-workspace` | n/a | `FocusWorkspace` (action only, no bin alias yet) | ⚠️ action OK, CLI alias missing |
| rename | `rename-workspace`, `rename-tab` | `rename` | `RenameWorkspace` (action only) | ⚠️ no `aterm rename` alias |
| restart / respawn | `respawn-pane` | n/a | `RestartWorkspace`, `RestartAllWorkspaces` | ✅ |
| change CLI | n/a | n/a | `ChangeWorkspaceCLI` | ✅ aterm-only |
| wait-until state | `wait-for` | `status` (poll) | `WaitUntil{workspace,state,timeout}` | ✅ aterm typed, telepty external poll |
| event subscribe | `set-hook` (limited) | `listen` / `monitor` (WS) | `Subscribe{events}` over IPC | ✅ |
| snapshot / refresh | `refresh-surfaces` | n/a | `RequestSnapshot` | ✅ |
| status pill / progress | `set-status`, `set-progress` | n/a | n/a | ❌ all three lack |
| notify (OSC9/99/777) | `notify` | n/a | n/a | ❌ all three lack |
| pane / split mgmt | full (panes, surfaces, splits) | n/a | n/a (single surface per workspace) | ❌ aterm + telepty lack |
| window mgmt | full (windows, focus, close) | n/a | (single window today) | ❌ aterm + telepty lack |
| browser pane | rich (40+ subcmds) | n/a | n/a | ❌ |

### 2.2 Smallest-patch-to-close summary (telepty side)

For options that keep telepty as the cross-terminal discovery layer (opt-1 / opt-3-prime), the only gap **on the orchestrator-daily critical path** is `send-key` (arbitrary keysym).

**Patch (3 files, ~30 LOC):**

| File | Change |
|---|---|
| `aigentry-telepty/cli.js:1831–1860` | drop the `if (key !== 'enter') exit` guard; pass `keysym` in submit body |
| `aigentry-telepty/daemon.js:1475–1548` (`/api/sessions/:id/submit`) | accept `body.keysym`; route to backend |
| `aigentry-telepty/terminal-backend.js` (after `cmuxSendEnter` at L102) | add `cmuxSendKey(sessionId, keysym)` → `execSync('cmux send-key --surface … <keysym>')`; add equivalent for kitty backend |

This is a **gap in telepty, not in aterm** — `aterm send-key` already accepts arbitrary keysyms because `SessionAction::SendKey { key: String }` is unconstrained at the Rust dispatcher.

### 2.3 Non-critical gaps (deferred)

- `notify`, `set-status`, `set-progress`, pane/window/browser primitives — none in cmux↔telepty parity column today; **none required for aterm-as-orchestrator-host** unless aterm absorbs cmux's full UX role (opt-2). Out of scope unless opt-2 is chosen.
- `identify` — covered indirectly via `WorkspaceStatus` and `ListWorkspaces.data`; could add a thin `aterm identify` alias in `bin/aterm.js` (~10 LOC) to mirror cmux semantics.

---

## §3 Option Evaluation (Task 3)

### Common pre-requisite (applies to ANY option): dead-code cleanup

The architecture audit surfaced 4 distinct ghost zones. Cleaning these up is a force-multiplier for every downstream option and reduces grill-grade ambiguity:

| Cleanup item | Effect | Effort |
|---|---|---|
| Remove `[[bin]] aterm-v3` + `src-v3/main.rs` reference + unused root deps (wgpu/glyphon/winit/pollster) from root `Cargo.toml` | workspace compiles cleanly, `cargo metadata` honest | 0.5 d |
| Delete `aterm-core/src/{renderer,renderer_atlas,renderer_glyph}.rs` (3 621 LOC) and the `#[cfg(feature="wgpu")]` blocks in `lib.rs` | no behavior change (already never compiled), readability +++ | 0.5 d |
| Delete `archived/src-v3-future`, `archived/src-tauri-v1` (already archived; remove if no historical interest) | repo size + clarity | 0.1 d |
| Doc the active topology in `aterm-structure-map.md` (already started) | onboarding | 0.4 d |

**Total cleanup ≈ 1.5 person-days.** Required regardless of opt choice.

### opt-1 — aterm = cmux-equivalent **viewer** for telepty-owned sessions

**Goal:** Sidebar UX boosts; CLI delegated 100 % to `telepty`. aterm renders telepty-owned PTYs.

**Concrete work:**

1. **PTY-byte ingress from telepty bus** (does not exist today): add a new bus event `terminal_bytes` (telepty side) + handler in `TeleptyBusClient.swift:263+` → forward bytes through a new FFI `aterm_core_feed_external_pty(session_id, bytes, len)` → splice into a new `Term` instance keyed by `session_id`. Estimate: **~600–900 LOC across telepty (daemon+spec), Swift, Rust**.
2. **Bidirectional input**: keypresses from a focused external session must go back to telepty over WS or HTTP. New write API in telepty + new Swift→FFI→bridge path.
3. **State sync**: cursor, scrollback, resize, OSC marks, sync-output flag — all must flow through the bus. Wire-format design and versioning.
4. **Disable Model B** (or run dual): a feature flag `ATERM_PRIMARY=telepty` to skip `PtyManager::create()` and route all spawn through `telepty allow`.

**Touch list:** `aterm-core/src/{lib,app,terminal,telepty_bridge}.rs`; `macos/Sources/{TerminalView,TeleptyBusClient,SessionSidebarView}.swift`; `aigentry-telepty/{daemon,session-state,session-routing}.js` + bus protocol spec.

**Effort:** **~3.5 person-weeks** (excl. cleanup). Most of the cost is the new wire protocol + back-pressure / sync semantics, which alacritty/portable-pty solves trivially in-process today.

**Constitutional alignment:**
- Art.1 (경량) — ❌ adds new wire protocol, dual code paths, and a viewer-mode flag.
- Art.3 (역할 침범) — ✅ aterm stays a renderer; telepty owns PTY.
- Art.9 (독립) — ❌ aterm becomes telepty-dependent; cannot run without daemon.
- Art.17 (무의존) — ➖ telepty is internal; not an "external" violation.

**UX implication:** User benefit ≈ "see all sessions across terminals in aterm sidebar." But aterm Model B already gives the within-aterm sidebar today, and `telepty list` already gives the cross-terminal list. The marginal gain is **cross-terminal visual rendering inside aterm**, which the user has not requested and which conflicts with the dogfood feedback `feedback_aterm_v3_only.md` (src-v3 only) + `feedback_aterm_session_priority.md` (aterm > cmux 우선).

**Risk:** highest of the three. Building a redundant rendering pipeline for telepty-owned PTYs duplicates alacritty parsing, scrollback, IME, theme — all already perfected in Model B. Long-term parallel-codepath debt.

### opt-2 — aterm replaces cmux as **orchestrator UI host** (full UX parity)

**Goal:** absorb cmux's window/workspace/pane/surface hierarchy + status pills + browser panes + tmux-compat into aterm.

**Concrete work:**

1. Window manager (multi-window, focus-window, close-window).
2. Pane/surface hierarchy (split horiz/vert, focus-pane, swap, break, join).
3. Status pill + progress + notification subsystem (`set-status`, `clear-status`, `set-progress`, `notify` mirroring cmux's daemon).
4. Browser pane bridge (40+ subcommands).
5. Layout engine for split tree + persistence.
6. tmux-compat command set.
7. Whatever socket protocol cmux clients use (V1 string or V2 JSON RPC).

**Touch list:** virtually all of `macos/Sources/*.swift` plus a new `aterm-core` layout module + IPC protocol additions covering the cmux command surface.

**Effort:** **8–14 person-weeks**, depending on whether browser panes and tmux-compat are in scope.

**Constitutional alignment:**
- Art.1 (경량) — ❌ explicit oversteps; many subsystems duplicate cmux.
- Art.3 (역할 침범) — ❌❌ aterm absorbing cmux is the textbook role-침범 violation.
- Art.9 (독립) — ✅ aterm stands alone for everything.
- Art.5 (최선) — ⚠️ good outcome but at a 5–10× cost vs. opt-3-prime.

**UX implication:** User gets a single app, but at the cost of months of work that the user has explicitly declined ("Claude Code 기능 재발명 X"). Memory feedback: `aterm > cmux 우선` says aterm should *take priority*, **not** that aterm should *absorb cmux's role*.

**Risk:** very high. Likely cannibalizes cmux maintenance and creates feature drift.

**Recommendation:** **REJECT** unless user explicitly endorses cmux retirement.

### opt-3 — Activate aterm-internal PTY model (standalone orchestrator)

**Goal:** Make aterm self-sufficient: it owns PTYs, exposes its own CLI surface, and operates with telepty *optional* (used only for cross-terminal discovery and bus messaging — not as PTY owner).

**Reality check:** ~95 % already done. Verified in this audit:

- `PtyManager::create()` (`pty.rs:260–463`) owns master FD + reader thread.
- `aterm-ipc` UnixSocket server (`aterm-ipc/src/server.rs`, 252 LOC, peer-uid auth) accepts `SessionAction::*`.
- `bin/aterm.js` (242 LOC) delivers cmux-compatible verbs.
- `SessionAction` enum (`action.rs:5–80`) covers: Inject, ListWorkspaces, CreateWorkspace, CloseWorkspace, ReadScreenText, **SendKey (arbitrary keysym — richer than telepty!)**, RestartWorkspace, RenameWorkspace, ChangeWorkspaceCLI, FocusWorkspace, AttachExternal, DetachWorkspace, ReloadSettings, Subscribe, WaitUntil, RequestSnapshot.
- Cross-machine: `tailscale.rs` (`tsnet`) is in tree.

**Concrete remaining work (opt-3-prime variant):**

| Item | File touch | Estimate |
|---|---|---|
| Common cleanup (§3 head) | root `Cargo.toml`, `aterm-core/src/{lib,renderer*}.rs`, `archived/*` | 1.5 d |
| `bin/aterm.js` add `rename`, `focus`, `restart`, `wait-until`, `subscribe`, `identify` aliases | `bin/aterm.js` | 0.5 d |
| Decide `AttachExternal` semantics: either (a) wire it to switch the active TerminalView to a telepty-discovered session **read-only** (sidebar pill click-through, OK as scaffolding) or (b) delete the SessionAction + Swift host callback (`lib.rs:2499/2563`, `app.rs:851–860`) until a real bus-byte path exists | `aterm-session/src/action.rs`, `aterm-core/src/{app,lib}.rs`, `AppDelegate.swift` | 1 d |
| Demote `TeleptyBridge` to discovery-only and document non-PTY role | `telepty_bridge.rs` doc comments + `telepty_bus.md` doc | 0.5 d |
| Ensure Model A read-only sidebar pill mode is documented + clean (`TeleptyBusClient` keeps WS subscription, but sidebar state machine clearly separates "aterm-internal workspace" vs "external telepty session pill") | `SessionSidebarView.swift`, structure-map | 1 d |
| Add `aterm identify` for cmux-style session identification | `bin/aterm.js`, `action.rs` | 0.5 d |
| Patch telepty `send-key` to accept arbitrary keysym (so cmux↔telepty is full-parity for cross-terminal bridging) — optional but symmetry-improving | telepty 3 files | 1 d |
| Tests — IPC roundtrip, send-key keysym, AttachExternal disposition | `tests/` | 1 d |

**Effort:** **~1.5 person-weeks** (incl. cleanup). Of these, ~5 d is documentation + tests rather than new code.

**Constitutional alignment:**
- Art.1 (경량) — ✅ after cleanup. Current bloat (3 621 LOC dead) is the blocker; once removed, opt-3-prime is the *minimum* viable session-control surface.
- Art.3 (역할 침범) — ✅ aterm = terminal+session, telepty = cross-terminal bridge. No overlap.
- Art.5 (최선) — ✅ no half-measures; opt-3 is the most direct route.
- Art.9 (독립) — ✅ aterm runs end-to-end with telepty absent.
- Art.13 (비판적+건설적+객관적) — ✅ recommendation grounded in code-evidence, not preference.
- Art.17 (무의존) — ✅ telepty stays internal-but-optional; no external lib.

**UX implication:** Immediate. Orchestrator can use `aterm <verb>` as drop-in for `cmux <verb>` for every daily command, today. Users on a single aterm window get full functionality without telepty installed.

**Risk:** lowest. Cleanup is the only invasive piece; everything else is alias additions and a semantic decision on `AttachExternal`.

**Trade-off vs. opt-1:** opt-3-prime *cannot* render PTY bytes from a session running in another terminal (ghostty/kitty). It only *lists* them via `telepty list`/bus. The user has not asked for cross-terminal rendering and has explicitly stated "aterm > cmux 우선" — so this is the right trade.

### 3.x Comparison summary

| Axis | opt-1 (viewer) | opt-2 (replace cmux) | opt-3-prime (standalone) |
|---|---|---|---|
| Effort (person-weeks, incl. cleanup) | ~3.5 | 8–14 | ~1.5 |
| Net new LOC | +1 200 to +1 800 | +6 000 to +12 000 | −3 600 (cleanup) +200 |
| Constitutional fit | mid (Art.9 ❌) | low (Art.1+3 ❌❌) | high (✅ across the board) |
| User benefit timing | weeks | months | days |
| Rollback cost | high (wire protocol) | very high | low (alias additions) |
| Parallel-codepath risk | high | very high | none (consolidates) |
| Memory-feedback alignment (`aterm > cmux 우선`, `v3 only`) | ➖ | ❌ | ✅ |

---

## §4 Cmux Integration Architecture (Task 4)

### 4.1 How cmux currently integrates with telepty

Verified via `aigentry-telepty/terminal-backend.js` and `cli.js:984–1015`:

1. **Sidebar source of truth.** cmux daemon does **not** read telepty's session DB. Instead, cmux's terminal-backend (telepty's own helper, not cmux's) periodically calls `cmux list-windows` and `cmux list-pane-surfaces --workspace workspace:N` (`terminal-backend.js:37–58`), then **regex-parses surface titles** for `/telepty\s*::\s*(\S+)/` (referenced in agent's analysis). This is **pattern-based discovery** — no shared state.
2. **Inject path.** `cmux send` and `cmux send-key` are PTY-level operations invoked **by telepty's terminal-backend** (`terminal-backend.js:81–117`) when an inject targets a session whose backend is `cmux`. cmux does not call telepty; telepty calls `cmux send …` via `execSync`.
3. **Workspace ↔ session mapping.** Stored in **telepty's** `~/.config/aigentry-telepty/sessions.json` (`daemon.js:24`), with `cmux_workspace_id` and `cmux_surface_id` populated from `process.env.CMUX_WORKSPACE_ID` / `CMUX_SURFACE_ID` set by cmux when it spawns the wrapped child. Reverse lookup is the regex above.
4. **Loose coupling.** No shared protocol; environment variables + PTY bytes + surface-title pattern parsing.

### 4.2 Does aterm need an equivalent? Where is the seam?

**No.** Because:

- `aterm-ipc` UnixSocket already maps `workspace name → InjectQueue → PTY writer` (`app.rs:319–360`, `pty.rs:566–580`). This is a **direct, in-process** join — no env vars, no regex.
- `bin/aterm.js` calls UnixSocket directly; it does not need to shell out to a sibling.
- Cross-terminal sessions (sessions hosted in ghostty/kitty) are surfaced via `TeleptyBusClient` for **listing only**. No control plane is needed inside aterm because the user can run `telepty inject` directly to address those.

If, in the future, aterm needs to inject *into* a session that lives in another terminal, the right seam is:

```
bin/aterm.js → detects target is external (via `telepty list` lookup)
            → shells out to `telepty inject <session> <text>` (or `telepty send-key`)
```

This is the **inverse** of cmux's pattern (cmux is the inner; telepty wraps it). For aterm, telepty is **outer-when-needed**, **absent-otherwise**.

### 4.3 Proposed seam (for the rare cross-terminal inject case in opt-3-prime)

```
                ┌────────────────────────────────────┐
                │  bin/aterm.js                      │
                │   1. parse args                    │
                │   2. UnixSocket → SessionAction    │
                │      (aterm-internal)              │
                │   3. if target unknown locally,    │
                │      shell out to                  │
                │       `telepty inject` …           │
                └─────┬───────────────────┬──────────┘
                      │                   │
       ┌──────────────▼──────┐    ┌───────▼────────────┐
       │ aterm-ipc           │    │ telepty (optional) │
       │  /tmp/aterm-{pid}   │    │  daemon :3848      │
       │  .sock              │    │                    │
       └──────────────┬──────┘    └────────────────────┘
                      │
                      ▼
              app.dispatch(...) → PtyManager → child
```

Total new code for the seam: **~30 LOC in `bin/aterm.js`** (if local IPC returns `Workspace not found`, fall back to `telepty inject`).

### 4.4 What aterm explicitly does **not** need

- Surface-title pattern parsing — aterm has typed `SessionAction` IPC.
- Env-var mapping — aterm spawns its own children directly.
- Persisted JSON of cmux IDs — `session.rs` persists workspaces by name.

---

## §5 ADR-Ready Recommendation (Task 5)

> Format mirrors `~/projects/aigentry-orchestrator/docs/adr/2026-05-04-phase6-conclusion.md`. **Not committed** — orchestrator will decide timing after grill completion.

---

### Decision

Adopt **opt-3-prime**: aterm v3 is the **primary, standalone session-controller**. Model B (`aterm-core::PtyManager`) owns PTYs; `bin/aterm.js` is the canonical orchestrator-daily CLI; telepty is retained as the **optional cross-terminal discovery + cross-machine bus**, never as the in-aterm PTY owner. The change is **mostly a cleanup + documentation effort (~1.5 person-weeks)**, not new functionality.

### Context

The grill surfaced two competing framings: aterm-as-viewer (opt-1) versus aterm-replaces-cmux (opt-2). Code audit shows neither is needed. aterm already exposes a typed `SessionAction` IPC (`aterm-session/src/action.rs`), peer-UID-authed `aterm-ipc` UnixSocket, and a Node CLI (`bin/aterm.js`) covering every orchestrator-daily verb — and `SessionAction::SendKey` accepts arbitrary keysyms, which `telepty send-key` does not. The architectural debt is concentrated in clearly-dead zones: `aterm-v3` workspace bin, `aterm-core/src/renderer*.rs` (~3 621 LOC), and `archived/*` snapshots. Removing them clarifies intent and unlocks Art.1 alignment without affecting runtime behavior.

### Consequences

**Pros**
- Smallest constitutional surface area (Art.1, 3, 9, 13, 17 all aligned post-cleanup).
- Smallest delivery cost (~1.5 person-weeks vs. 3.5 / 8–14).
- Removes 3 600+ LOC dead code; raises code clarity for future contributors.
- Drop-in compatibility with cmux orchestrator scripts via `bin/aterm.js` aliases.
- aterm runs without telepty for the within-aterm-only case (`aigentry headless` lite).

**Cons**
- aterm cannot **render** PTY bytes from sessions hosted in other terminals (only **list** them via `TeleptyBusClient` pill). Users wanting unified visual rendering across terminals must adopt opt-1 or opt-2 later.
- `AttachExternal` SessionAction must be either deleted or downgraded to a UI-pill scaffolding only — orchestrator must pick one (Open Q1).
- Cross-terminal `aterm inject <external> "…"` requires a small fallback to `telepty inject`; users who never run telepty cannot reach external sessions.

**Side effects**
- `aterm-core` Cargo.toml stays minimal; root `Cargo.toml` becomes a workspace metadata file only (no `[[bin]]`).
- Onboarding doc and `aterm-structure-map.md` get rewritten to drop renderer*/wgpu narrative.
- npm `bin/aterm.js` gets ~5 new aliases (`rename`, `focus`, `restart`, `identify`, `wait-until`).

### Alternatives Considered

- **opt-1 (aterm = viewer)**: rejected. ~3.5 weeks for a benefit (cross-terminal rendering) the user has not requested and which violates Art.9 (독립).
- **opt-2 (aterm replaces cmux)**: rejected. 8–14 weeks; explicit Art.3 violation; conflicts with stated principle "Claude Code 기능 재발명 X."
- **status quo**: rejected. Dead-code debt + ambiguous Model A scaffolding leak grill-grade ambiguity into every future architectural conversation.

### Open Questions

1. **AttachExternal semantics** — delete the SessionAction + Swift host callback (`lib.rs:2499/2563`, `app.rs:851–860`, `action.rs:59–62`) until there is a concrete need, or downgrade it to "select sidebar pill (read-only)" without bus-byte streaming?
2. **Cross-terminal inject fallback** — should `bin/aterm.js` shell out to `telepty inject` when the workspace is unknown locally, or fail-fast with a helpful message?
3. **Headless mode** — user's dogfood feedback on headless still pending; opt-3-prime makes it cheaper (no GUI dep on PTY ownership) but does not implement it. Track separately?
4. **Telepty `send-key` keysym patch** — owned by telepty repo; coordinate with telepty session, or defer?
5. **Workspace ↔ identity mapping** — `aterm identify` alias parity with cmux: include hostname + pid + terminal in payload, or just workspace name?

### Concrete File / Module Touch List (no implementation)

> Cleanup pre-req

- `Cargo.toml` (workspace root): remove `[[bin]] aterm-v3`; remove unused deps (`wgpu`, `glyphon`, `winit` path-dep, `pollster`); drop `[package]` block if root is purely a workspace.
- `aterm-core/src/renderer.rs` — delete (2 613 LOC).
- `aterm-core/src/renderer_atlas.rs` — delete (385 LOC).
- `aterm-core/src/renderer_glyph.rs` — delete (623 LOC).
- `aterm-core/src/lib.rs` — remove all `#[cfg(feature = "wgpu")]` blocks (~30 sites in 56–907) and the matching `pub mod renderer*;` declarations at L51–67.
- `archived/src-v3-future/` — delete or move outside repo.
- `archived/src-tauri-v1/` — delete or move outside repo.
- `aterm-structure-map.md` — refresh with the new minimal topology.

> opt-3-prime additions (alias / docs only)

- `bin/aterm.js` — add commands: `rename`, `focus`, `restart`, `identify`, `wait-until`, `subscribe`. Each ≈ 8–15 LOC.
- `bin/aterm.js` — optional: cross-terminal fallback to `telepty inject` (Open Q2).
- `aterm-session/src/action.rs` — decision per Open Q1: either delete `AttachExternal` variant or annotate it as discovery-only.
- `aterm-core/src/app.rs:851–860` — match decision.
- `aterm-core/src/lib.rs:2499/2563` — match decision.
- `macos/Sources/AppDelegate.swift` (AttachExternal handler) — match decision.
- `aterm-core/src/telepty_bridge.rs` — top-of-file doc comment: "discovery + registration only; aterm never streams PTY bytes through the bus."
- `macos/Sources/SessionSidebarView.swift` — keep telepty pill rendering; add doc comment that `terminal != "aterm"` rows are read-only and click-through opens an external terminal hint, not an embedded view.
- `docs/architecture/session-control.md` (new) — short architecture note linking this report.
- `tests/ipc_roundtrip.rs` (new or extended) — IPC roundtrip for SendKey arbitrary keysym, ListWorkspaces, CreateWorkspace.

> opt-3-prime adjacent (telepty repo, optional)

- `aigentry-telepty/cli.js:1831–1860` — drop enter-only guard.
- `aigentry-telepty/daemon.js:1475–1548` — accept `keysym` in submit body.
- `aigentry-telepty/terminal-backend.js:102+` — add `cmuxSendKey(sessionId, keysym)`; mirror for kitty backend.

---

## Appendix A — Verified file:line refs (this audit)

- `aterm-core/Cargo.toml` deps (no wgpu/glyphon/winit): confirmed.
- `Cargo.toml:10-12` → `[[bin]] aterm-v3 / src-v3/main.rs`: target dir absent (`ls /Users/.../src-v3 → No such file or directory`).
- `aterm-core/src/lib.rs:51–67` `pub mod {…, renderer, renderer_atlas, renderer_glyph, …}`.
- `aterm-core/src/lib.rs:56,58,60,75,158,177,179,190,198,203,205,220,281,283,293,298,300,302,308,313,550,647,656,666,678,685,692,721,873,907 …` → `#[cfg(feature = "wgpu")]` guards.
- `aterm-core/src/telepty_bridge.rs:1–104` `try_connect()` retry + version + auto-restart, fire-and-forget.
- `aterm-core/src/app.rs:851–860` `SessionAction::AttachExternal { session_id }` → `host.attach_external_session(&session_id)`.
- `aterm-core/src/lib.rs:2499` `pub attach_external_session: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>`.
- `aterm-core/src/lib.rs:2563` `fn attach_external_session(&self, session_id: &str)`.
- `aterm-session/src/action.rs:5–80` full `SessionAction` enum.
- `aterm-ipc/src/server.rs` 252 LOC; `aterm-ipc/src/auth.rs` 45 LOC.
- `bin/aterm.js` 242 LOC; commands `send / send-key / read-screen / list-workspaces|ls / new-workspace|new / close-workspace|close / status`.
- `archived/src-v3-future/main.rs`, `archived/src-tauri-v1/src/main.rs`.
- `aigentry-telepty/cli.js:1831–1860` `send-key` enter-only guard with `force=true` submit.
- `aigentry-telepty/terminal-backend.js:7–117` `cmux ping` detection, `cmuxSendText`, `cmuxSendEnter`; **no `cmuxSendKey` exists today**.

## Appendix B — Stats

- aterm-core src LOC: 14 855 (largest: lib.rs 2 918, renderer.rs 2 613 💀, pty.rs 2 290, inject.rs 1 155, app.rs 1 018, terminal.rs 754, renderer_glyph.rs 623 💀, tailscale.rs 516, session.rs 504, renderer_atlas.rs 385 💀, telepty_bridge.rs 358, telepty.rs 188, sync.rs 49, cli_presets.rs 46).
- macos Swift LOC: 8 821 (largest: AppDelegate 2 522, SessionSidebarView 1 573, TerminalView 1 404, SettingsView 1 182, MetalRenderer 768, GlyphAtlas 668, OrchestratorInputBar 653, OnboardingView 523, TeleptyBusClient 342, OrchestratorCommands 317, AtermTheme 206, OrchestratorHistory 81, GlyphAtlas-shaders 12, AtermLocalization 12).
- Dead Rust: ~3 621 LOC (renderer trio) + workspace ghost.
- Net opt-3-prime delta: **−3 600 LOC + ~200 LOC alias/docs = ~−3 400 LOC**.

---

*Compiled 2026-05-06 by `aigentry-architect-aterm-session-control` · model: claude-opus-4-7 · verified against repo HEAD.*
