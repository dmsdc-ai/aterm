# Session Persistence Architecture (v3)

## Problem

aterm has two independent session persistence systems that are incompatible:
- **Rust** `session.rs`: `~/.aterm/sessions.json` — `{sessions: [{id, cwd, command, args}]}`
- **Swift** `AppDelegate`: `~/.aigentry/config/sessions.json` — `[{name, command, customCommand, cwd, isActive, isSystem, resumeCommand}]`

This causes: duplicate state, format mismatch, stale sessions surviving crashes, no health check, and platform-specific persistence logic that can't be reused across 5 target platforms.

## Decision

**Rust core = single owner of session persistence.** Swift (and future platform UIs) call Rust FFI only.

Rationale: Write once in Rust, reuse across macOS/Linux/Windows/Android/iOS. Platform UI layers handle rendering and IME input only.

## Codebase Context

### `src-v3/` vs `aterm-core/` relationship

| Crate | Type | Purpose | session.rs |
|-------|------|---------|------------|
| `src-v3/` | standalone binary | Development/testing entry point (`main.rs`) | `src-v3/core/session.rs` |
| `aterm-core/` | cdylib (FFI) | Production library linked by Swift via `lib.rs` | `aterm-core/src/session.rs` |

Both contain nearly identical `session.rs` code. **Target for this spec: `aterm-core/src/session.rs`** — this is the FFI crate used by production Swift app. `src-v3/core/session.rs` should be kept in sync or deprecated (out of scope for this spec).

### Existing code we build on

- `SessionStore` with `load()`, `save(&PtyManager)`, `restore_into(&PtyManager)` — already in `aterm-core/src/session.rs`
- `SessionEntry` with `#[serde(rename_all = "camelCase")]` — fields: `{id, cwd, command, args}`
- `workspace_is_alive(&self, id: &str) -> bool` in `aterm-core/src/pty.rs` line 496 — uses `child.try_wait()` with mutex lock + dead status fallback
- `session_entries(&self)` in `PtyManager` — derives entries from live workspaces, already filters dead

## Design

### Architecture

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│ Swift/AppKit │  │  GTK / Qt   │  │ Kotlin/Swift │
│   (macOS)    │  │   (Linux)   │  │ (Android/iOS)│
└──────┬───────┘  └──────┬──────┘  └──────┬───────┘
       │  FFI only        │                │
       └────────┬─────────┴───────┬────────┘
         ┌──────▼─────────────────▼──────┐
         │        aterm-core (Rust)       │
         │  PtyManager · SessionStore     │
         │  InjectQueue · HealthCheck     │
         └──────┬────────────────────────┘
                │
         ┌──────▼──────────────────────┐
         │ ~/.aterm/sessions.json      │
         │ (single source of truth)    │
         └─────────────────────────────┘
```

### Extend existing `SessionEntry`

```rust
// aterm-core/src/session.rs
// Existing: #[serde(rename_all = "camelCase")]
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    // NEW fields (migrated from Swift)
    #[serde(default)]                    // backward compat: missing → None
    pub custom_command: Option<String>,   // serializes as "customCommand" (camelCase)
    #[serde(default)]                    // backward compat: missing → false
    pub is_system: bool,                 // serializes as "isSystem" (camelCase)
    #[serde(default)]                    // backward compat: missing → None
    pub resume_command: Option<String>,  // serializes as "resumeCommand" (camelCase)
}
```

Note: `rename_all = "camelCase"` already exists on the struct. New field names intentionally match Swift format (`customCommand`, `isSystem`, `resumeCommand`) for migration compatibility.

### Extend `Workspace` struct to carry new fields

The derive-on-save pattern derives `SessionEntry` from live `Workspace` state. For new fields to survive save cycles, `Workspace` must carry them:

```rust
// aterm-core/src/pty.rs — Workspace struct
pub struct Workspace {
    // ... existing fields (id, child, writer, status, etc.)
    // NEW fields
    pub custom_command: Option<String>,
    pub is_system: bool,
    pub resume_command: Option<String>,
}
```

**Data flow for new fields:**
1. `restore_session_entry()` reads `SessionEntry` from disk → passes new fields to `create()`
2. `create()` stores new fields in `Workspace` struct
3. `session_entries()` reads new fields from `Workspace` → populates `SessionEntry`
4. `save()` writes `SessionEntry` to disk

Functions that must be updated:
- `Workspace` struct: add 3 new fields
- `create()` / `restore_session_entry()`: accept and store new fields
- `session_entries()`: read new fields from `Workspace` into `SessionEntry`

### Keep derive-on-save pattern (existing design)

- `save(&PtyManager)` derives session list from live workspace state — no in-memory cache, no drift
- `load()` reads from disk
- `restore_into(&PtyManager)` recreates workspaces from loaded entries

### Health Check on Startup

Reuse existing `workspace_is_alive()` from `pty.rs` (line 496). This already:
1. Locks `child` mutex
2. Calls `child.try_wait()` — returns `None` if alive
3. Falls back to workspace `status` field check
4. Marks dead workspaces

**Cold start flow** (no live PtyManager yet):
1. `SessionStore::load()` reads entries from disk
2. `restore_into(&PtyManager)` spawns workspaces for each entry
3. If CLI fails to start after 3 retries (existing auto-restart limit), workspace is marked dead
4. Next `save()` call filters dead workspaces via `session_entries()` (already filters `status == "dead"`)
5. Stale entries self-clean within one app lifecycle

No PID persistence needed. No new health check API needed.

### Atomic File Writes

```rust
fn save_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;

    // POSIX: fs::rename is atomic replacement
    // Windows: rename fails if destination exists, so remove first
    #[cfg(target_os = "windows")]
    { let _ = fs::remove_file(path); }

    fs::rename(&tmp, path)
}
```

### Storage Path

Single path with OS routing via `dirs` crate:

```rust
fn sessions_path() -> PathBuf {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    { dirs::home_dir().unwrap().join(".aterm/sessions.json") }

    #[cfg(target_os = "windows")]
    { dirs::data_dir().unwrap().join("aterm/sessions.json") }
}
```

Mobile (Android/iOS) deferred — PTY model fundamentally different on mobile.

### FFI Bridge (new exports in `aterm-core/src/lib.rs`)

```rust
/// C-compatible session entry for platform UI consumption
#[repr(C)]
pub struct SessionEntryFFI {
    pub id: *const c_char,
    pub cwd: *const c_char,
    pub command: *const c_char,
    pub args_json: *const c_char,        // JSON array string: ["--flag", "value"]
    pub custom_command: *const c_char,    // nullable
    pub is_system: bool,
    pub resume_command: *const c_char,    // nullable
}

// All FFI functions take core: *mut AtermCore as first parameter,
// matching the existing FFI pattern in lib.rs where PtyManager lives inside AtermCore.

#[no_mangle]
pub extern "C" fn aterm_session_count(core: *mut AtermCore) -> u32;

#[no_mangle]
pub extern "C" fn aterm_session_get(core: *mut AtermCore, index: u32) -> SessionEntryFFI;

#[no_mangle]
pub extern "C" fn aterm_session_free(entry: SessionEntryFFI);  // free CString allocations

#[no_mangle]
pub extern "C" fn aterm_sessions_save(core: *mut AtermCore);

#[no_mangle]
pub extern "C" fn aterm_sessions_restore(core: *mut AtermCore);
```

Platform UI calls these FFI functions instead of managing its own persistence.

### Dual Session Systems (unchanged)

| System | Source | Purpose |
|--------|--------|---------|
| Internal workspaces | SessionStore (Rust core) | aterm's own sessions |
| External sessions | TeleptyBusClient (read-only) | display-only in sidebar |

These remain independent.

## Migration

### Swift → Rust handoff

1. Add new fields to Rust `SessionEntry` with `#[serde(default)]` for backward compat
2. Add FFI functions to `lib.rs` with `SessionEntryFFI` C-compatible struct
3. Swift `AppDelegate`: remove persistence code:
   - `saveWorkspaces()` (~22 lines)
   - `restoreWorkspaces()` (~23 lines)
   - `restoreEntries()` (~30 lines)
   - `workspacesFileURL` static (~4 lines)
   - Save call sites at lines 105, 811, 954 → replace with `aterm_sessions_save(core)` FFI call
   - Restore call site at line 89 (`restoreWorkspaces()`) → replace with `aterm_sessions_restore(core)` FFI call
4. Swift calls `aterm_sessions_restore(core)` / `aterm_sessions_save(core)` via FFI

### Format migration (one-time)

On first `load()`, if `~/.aterm/sessions.json` is missing but `~/.aigentry/config/sessions.json` exists:
1. Attempt to read Swift format (bare JSON array)
2. Convert to Rust format (`{sessions: [...]}` with field mapping)
3. Write to `~/.aterm/sessions.json` via atomic write
4. **If Swift file is corrupt / parse fails**: log warning, start fresh (empty sessions). Do NOT propagate error or block app startup.

After successful migration, the Swift file is ignored (not deleted — user may want to rollback).

## Scope

- **In scope (desktop)**: Extend `SessionEntry`, health check via existing `workspace_is_alive()`, atomic writes, FFI bridge (`SessionEntryFFI`), Swift cleanup, format migration
- **Out of scope**: `src-v3/core/session.rs` sync/deprecation, telepty integration changes, UI redesign, mobile (Android/iOS)

## Success Criteria

1. App restart restores only live sessions (no ghost workspaces)
2. Corrupted sessions.json → graceful recovery (fresh start)
3. Same Rust logic works on macOS/Linux/Windows
4. Swift persistence code removed — zero duplication
5. Existing `~/.aterm/sessions.json` files deserialize correctly (backward compat via `#[serde(default)]`)
6. Zero regression on existing session create/close/restore flow
