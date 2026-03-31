# Session Persistence Architecture — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify session persistence into Rust core, eliminating Swift's duplicate persistence and enabling cross-platform reuse.

**Architecture:** Extend existing `SessionStore` in `aterm-core/src/session.rs` with new fields, atomic writes, and FFI bridge. Remove Swift persistence from `AppDelegate.swift`. One-time migration from Swift format.

**Tech Stack:** Rust (serde, dirs crate), Swift/AppKit (FFI calls only), portable-pty

**Spec:** `docs/superpowers/specs/2026-03-31-session-persistence-design.md`

---

## Chunk 1: Rust Core — Extend Data Model

### Task 1: Extend `SessionEntry` with new fields

**Files:**
- Modify: `aterm-core/src/session.rs:7-20` (SessionEntry struct)

- [ ] **Step 1: Add new fields to SessionEntry**

Add `custom_command`, `is_system`, `resume_command` with `#[serde(default)]`:

```rust
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub custom_command: Option<String>,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default)]
    pub resume_command: Option<String>,
}
```

- [ ] **Step 2: Verify backward compatibility**

Run: `cargo build -p aterm-core`
Expected: PASS. Existing sessions.json files with only 4 fields should still deserialize (serde default fills missing).

- [ ] **Step 3: Commit**

```bash
git add aterm-core/src/session.rs
git commit -m "feat(session): add custom_command, is_system, resume_command to SessionEntry"
```

### Task 2: Extend `Workspace` struct with new fields

**Files:**
- Modify: `aterm-core/src/pty.rs:95-113` (Workspace struct)

- [ ] **Step 1: Add new fields to Workspace**

```rust
pub struct Workspace {
    // ... existing fields ...
    pub custom_command: Option<String>,
    pub is_system: bool,
    pub resume_command: Option<String>,
}
```

- [ ] **Step 2: Update all Workspace construction sites**

Find every place `Workspace { ... }` is constructed (in `create()` at pty.rs:226-341) and add the new fields with defaults:

```rust
custom_command: None,
is_system: false,
resume_command: None,
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p aterm-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add aterm-core/src/pty.rs
git commit -m "feat(pty): add session metadata fields to Workspace struct"
```

### Task 3: Wire new fields through data flow

**Files:**
- Modify: `aterm-core/src/pty.rs` — `create()`, `restore_session_entry()`, `session_entries()`

- [ ] **Step 1: Update `create()` to accept new fields**

Add parameters to `create()` (or use a struct param if >10 params):

```rust
pub fn create(
    &mut self,
    id: &str,
    cwd: &str,
    command: &str,
    args: &[String],
    // NEW
    custom_command: Option<String>,
    is_system: bool,
    resume_command: Option<String>,
    // ... existing params ...
) -> Result<...>
```

Store in the `Workspace` struct.

- [ ] **Step 2: Update `restore_session_entry()` to forward new fields**

At pty.rs:343-357, pass `entry.custom_command`, `entry.is_system`, `entry.resume_command` to `create()`.

- [ ] **Step 3: Update `session_entries()` to read new fields**

At pty.rs:464-483, populate new fields when constructing `SessionEntry`:

```rust
SessionEntry {
    id: ws.id.clone(),
    cwd: ws.cwd.clone(),
    command: ws.command.clone(),
    args: ws.args.clone(),
    custom_command: ws.custom_command.clone(),
    is_system: ws.is_system,
    resume_command: ws.resume_command.clone(),
}
```

- [ ] **Step 4: Fix all callers of `create()`**

Search for all `create()` call sites. Add default values for new params where not applicable:

```rust
// Non-restored workspaces: use defaults
custom_command: None,
is_system: false,
resume_command: None,
```

- [ ] **Step 5: Build and verify**

Run: `cargo build -p aterm-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add aterm-core/src/pty.rs
git commit -m "feat(pty): wire session metadata through create/restore/save cycle"
```

---

## Chunk 2: Rust Core — Atomic Writes & Migration

### Task 4: Add atomic file writes

**Files:**
- Modify: `aterm-core/src/session.rs` — `save()` method

- [ ] **Step 1: Add `save_atomic` helper**

```rust
use std::io;

fn save_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;

    #[cfg(target_os = "windows")]
    { let _ = fs::remove_file(path); }

    fs::rename(&tmp, path)
}
```

- [ ] **Step 2: Replace `fs::write` in `save()` with `save_atomic`**

In `SessionStore::save()` (session.rs:60-68), replace the direct `fs::write` call with `save_atomic`.

- [ ] **Step 3: Build and verify**

Run: `cargo build -p aterm-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add aterm-core/src/session.rs
git commit -m "fix(session): use atomic write to prevent corruption on crash"
```

### Task 5: Add one-time Swift format migration

**Files:**
- Modify: `aterm-core/src/session.rs` — `load()` method

- [ ] **Step 1: Define Swift session format struct**

```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwiftSessionEntry {
    name: String,
    command: Option<String>,
    custom_command: Option<String>,
    cwd: Option<String>,
    is_active: Option<bool>,
    is_system: Option<bool>,
    resume_command: Option<String>,
}
```

- [ ] **Step 2: Add migration logic to `load()`**

In `SessionStore::load()`, after `NotFound` error on primary path:

```rust
// Check for Swift format at ~/.aigentry/config/sessions.json
let swift_path = dirs::home_dir().unwrap().join(".aigentry/config/sessions.json");
if swift_path.exists() {
    match fs::read_to_string(&swift_path) {
        Ok(content) => {
            match serde_json::from_str::<Vec<SwiftSessionEntry>>(&content) {
                Ok(swift_entries) => {
                    let entries: Vec<SessionEntry> = swift_entries.into_iter().map(|s| {
                        SessionEntry {
                            id: s.name,
                            cwd: s.cwd.unwrap_or_default(),
                            command: s.command.unwrap_or_default(),
                            args: vec![],
                            custom_command: s.custom_command,
                            is_system: s.is_system.unwrap_or(false),
                            resume_command: s.resume_command,
                        }
                    }).collect();
                    let data = SessionData { sessions: entries.clone() };
                    let json = serde_json::to_string_pretty(&data).unwrap();
                    let _ = save_atomic(&self.path, json.as_bytes());
                    return Ok(entries);
                }
                Err(e) => {
                    eprintln!("[session] Swift migration parse failed: {e}. Starting fresh.");
                }
            }
        }
        Err(e) => {
            eprintln!("[session] Swift migration read failed: {e}. Starting fresh.");
        }
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build -p aterm-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add aterm-core/src/session.rs
git commit -m "feat(session): one-time migration from Swift sessions.json format"
```

---

## Chunk 3: FFI Bridge

### Task 6: Add SessionEntryFFI and FFI exports

**Files:**
- Modify: `aterm-core/src/lib.rs` — add FFI functions
- Modify: `aterm-core/src/session.rs` — add `SessionEntryFFI`

- [ ] **Step 1: Define SessionEntryFFI in session.rs**

```rust
use std::ffi::{CString, c_char};

#[repr(C)]
pub struct SessionEntryFFI {
    pub id: *const c_char,
    pub cwd: *const c_char,
    pub command: *const c_char,
    pub args_json: *const c_char,
    pub custom_command: *const c_char, // nullable
    pub is_system: bool,
    pub resume_command: *const c_char, // nullable
}

impl SessionEntryFFI {
    pub fn from_entry(entry: &SessionEntry) -> Self {
        let to_ptr = |s: &str| CString::new(s).unwrap().into_raw() as *const c_char;
        let opt_ptr = |s: &Option<String>| match s {
            Some(v) => CString::new(v.as_str()).unwrap().into_raw() as *const c_char,
            None => std::ptr::null(),
        };
        SessionEntryFFI {
            id: to_ptr(&entry.id),
            cwd: to_ptr(&entry.cwd),
            command: to_ptr(&entry.command),
            args_json: to_ptr(&serde_json::to_string(&entry.args).unwrap()),
            custom_command: opt_ptr(&entry.custom_command),
            is_system: entry.is_system,
            resume_command: opt_ptr(&entry.resume_command),
        }
    }
}
```

- [ ] **Step 2: Add FFI functions to lib.rs**

```rust
#[no_mangle]
pub extern "C" fn aterm_session_count(core: *mut AtermCore) -> u32 {
    let core = unsafe { &*core };
    let store = SessionStore::with_path(sessions_path());
    store.load().map(|e| e.len() as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn aterm_session_get(core: *mut AtermCore, index: u32) -> SessionEntryFFI {
    let core = unsafe { &*core };
    let store = SessionStore::with_path(sessions_path());
    let entries = store.load().unwrap_or_default();
    if (index as usize) < entries.len() {
        SessionEntryFFI::from_entry(&entries[index as usize])
    } else {
        // return empty/null entry
        SessionEntryFFI { id: std::ptr::null(), cwd: std::ptr::null(), command: std::ptr::null(), args_json: std::ptr::null(), custom_command: std::ptr::null(), is_system: false, resume_command: std::ptr::null() }
    }
}

#[no_mangle]
pub extern "C" fn aterm_session_free(entry: SessionEntryFFI) {
    unsafe {
        let free_ptr = |p: *const c_char| { if !p.is_null() { drop(CString::from_raw(p as *mut c_char)); } };
        free_ptr(entry.id);
        free_ptr(entry.cwd);
        free_ptr(entry.command);
        free_ptr(entry.args_json);
        free_ptr(entry.custom_command);
        free_ptr(entry.resume_command);
    }
}

#[no_mangle]
pub extern "C" fn aterm_sessions_save(core: *mut AtermCore) {
    let core = unsafe { &mut *core };
    let store = SessionStore::with_path(sessions_path());
    let _ = store.save(&core.pty_manager);
}

#[no_mangle]
pub extern "C" fn aterm_sessions_restore(core: *mut AtermCore) -> u32 {
    let core = unsafe { &mut *core };
    let store = SessionStore::with_path(sessions_path());
    match store.load() {
        Ok(entries) => {
            let count = entries.len() as u32;
            let _ = store.restore_into(&mut core.pty_manager);
            count
        }
        Err(_) => 0,
    }
}
```

Note: `aterm_sessions_restore` returns `u32` count for Swift caller compatibility (line 89 uses return value).

- [ ] **Step 3: Build and verify**

Run: `cargo build -p aterm-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add aterm-core/src/session.rs aterm-core/src/lib.rs
git commit -m "feat(ffi): add session persistence FFI bridge for platform UIs"
```

---

## Chunk 4: Swift Cleanup

### Task 7: Replace Swift persistence with FFI calls

**Files:**
- Modify: `macos/Sources/AppDelegate.swift` — remove persistence, add FFI calls

- [ ] **Step 1: Add Swift FFI declarations**

At top of AppDelegate.swift (or a bridging header):

```swift
// Session persistence FFI
func aterm_session_count(_ core: OpaquePointer?) -> UInt32
func aterm_sessions_save(_ core: OpaquePointer?)
func aterm_sessions_restore(_ core: OpaquePointer?) -> UInt32
```

- [ ] **Step 2: Replace restore call site (line 89)**

Replace:
```swift
let restoredCount = restoreWorkspaces()
```
With:
```swift
let restoredCount = Int(aterm_sessions_restore(atermCore))
```

- [ ] **Step 3: Replace save call sites (lines 105, 811, 954)**

Replace all `saveWorkspaces()` calls with:
```swift
aterm_sessions_save(atermCore)
```

- [ ] **Step 4: Remove Swift persistence functions**

Delete:
- `workspacesFileURL` (lines 118-121)
- `saveWorkspaces()` (lines 123-144)
- `restoreWorkspaces()` (lines 156-178)
- `restoreEntries()` (lines 180-210)

- [ ] **Step 5: Build and verify**

Run: `make rust && make swift`
Expected: PASS.

- [ ] **Step 6: Smoke test**

Run: `make app && open build/aterm.app`
- Create a workspace
- Close aterm
- Reopen aterm
- Verify workspace is restored

- [ ] **Step 7: Commit**

```bash
git add macos/Sources/AppDelegate.swift
git commit -m "refactor(macos): replace Swift session persistence with Rust FFI"
```

---

## Chunk 5: Integration & Verification

### Task 8: End-to-end verification

- [ ] **Step 1: Test backward compatibility**

Create a `~/.aterm/sessions.json` with old format (4 fields only). Launch aterm. Verify it loads without error and new fields default correctly.

- [ ] **Step 2: Test Swift migration**

Remove `~/.aterm/sessions.json`. Create `~/.aigentry/config/sessions.json` with Swift format. Launch aterm. Verify one-time migration creates `~/.aterm/sessions.json` in Rust format.

- [ ] **Step 3: Test corruption recovery**

Write garbage to `~/.aterm/sessions.json`. Launch aterm. Verify it starts fresh with no crash.

- [ ] **Step 4: Test session lifecycle**

Create workspace → close aterm → reopen → verify restored → close workspace → reopen → verify NOT restored.

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "test: verify session persistence migration and edge cases"
```

---

## Summary

| Chunk | Tasks | Key Changes |
|-------|-------|-------------|
| 1 | 1-3 | Extend SessionEntry + Workspace + data flow |
| 2 | 4-5 | Atomic writes + Swift format migration |
| 3 | 6 | FFI bridge (SessionEntryFFI + 5 functions) |
| 4 | 7 | Swift cleanup (remove ~80 lines, add FFI calls) |
| 5 | 8 | Integration testing |

**Total estimated changes:** ~150 lines Rust added, ~80 lines Swift removed, ~20 lines Swift added (FFI declarations + calls).
