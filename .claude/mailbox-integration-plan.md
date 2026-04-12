# Mailbox Integration Plan — aterm (#190)

## 1. Current InjectQueue Lifecycle

### Enqueue Path

Two entry points push messages into the same `InjectQueue`:

| Entry Point | File | Line | Context |
|---|---|---|---|
| IPC dispatch | `app.rs` | 544-573 | External: telepty → Unix socket → `SessionAction::Inject` |
| PtyManager | `pty.rs` | 505-528 | Internal: `queue_inject()` called by host |

Both follow the same pattern:
```
lock queue → push(InjectMessage) → notify InjectSignal
```

**Capacity behavior (inject.rs:254-257):**
- `INJECT_QUEUE_CAPACITY = 256`
- `push()` returns `Err("inject queue full")` when at capacity
- **Message is rejected** — sender gets an error response, but there is no retry, backpressure, or overflow buffer
- IPC caller receives `ActionResponse::Error` — telepty gets the HTTP error but has no built-in retry

### Idle Gate: `should_inject()` (inject.rs:341-354)

All 4 conditions must be true simultaneously:

| Condition | Source | Threshold |
|---|---|---|
| `last_output_has_prompt` | PTY output contains prompt pattern OR OSC 133;B | boolean |
| `queue_has_messages` | InjectQueue non-empty | boolean |
| `last_user_input.elapsed()` | No keyboard input recently | ≥ 2s (`IDLE_THRESHOLD`) |
| `last_output_at.elapsed()` | Output settled (heuristic only) | ≥ 1s (`OUTPUT_SETTLE`) |

OSC 133;B (FinalTerm) is definitive — skips OUTPUT_SETTLE wait.

**Escape valve:** `FORCE_INJECT_TIMEOUT = 30s` — if oldest message has been queued ≥30s, inject fires regardless of idle gate. This is the only protection against indefinite blocking.

**Blocking scenarios:**
- Shell doesn't emit recognizable prompt (custom PS1, non-standard shell) → idle gate never opens → force-inject at 30s
- User is actively typing → `IDLE_THRESHOLD` keeps resetting → messages queue up
- Both combined → 30s worst-case latency per message

### Dequeue and Delivery (inject.rs:392-526)

`run_injector_loop()` runs in a **dedicated thread per workspace**:

1. Condvar wait (zero CPU when idle, instant wake on signal)
2. Check lifecycle — break if `"dead"` or `"closing"`
3. Check `should_inject()` — if false, check force-inject
4. Pop one message from queue
5. Write text bytes to PTY writer (without trailing \r\n)
6. Flush
7. **50ms pause** (for TUI frameworks to process text before Enter)
8. Write `\r` (Enter keypress)
9. Flush
10. Reset prompt state
11. **25ms throttle coalesce** before next iteration

### Error Handling on PTY Write Failure (inject.rs:507-516)

**Catastrophic failure mode — no recovery:**
```
write_result.is_err() → clear entire queue → replace writer with ClosedWriter → set status "dead" → break loop
```

- **No retry** on transient write failure
- **All pending messages destroyed** on first error
- Workspace marked permanently dead
- Same behavior in `mark_workspace_dead_handles()` (pty.rs:655-670)

### ACK Gap

- `InjectDelivered` event is **defined** in `aterm-session/src/action.rs:115` but **never emitted** from `aterm-core`
- Sender only gets `{"queued": N}` — confirmation of enqueue, NOT delivery
- No way for telepty or any external caller to know if a message was actually delivered to the PTY

---

## 2. IPC Server → Dispatch → InjectQueue Flow

### Socket Setup (aterm-ipc/src/server.rs)

```
AtermApp::start_ipc() (app.rs:490)
  → IpcServer::start(socket_path, dispatcher)
    → UnixListener::bind(socket_path)  [permissions: 0o600]
    → spawn accept loop thread
      → per-connection: verify_peer() [UID match] → spawn handler thread
```

- Socket path: `/tmp/aterm-{random}.sock`
- Auth: kernel-level `getpeereid()` (macOS) / `SO_PEERCRED` (Linux) — same UID only
- Each connection gets its own thread with a `BufReader` line-by-line JSON parser

### Telepty → aterm Message Flow

```
telepty daemon
  → connects to aterm Unix socket (address from registration payload)
  → sends: {"action":"Inject","workspace":"name","text":"...","from":"sender"}
  → aterm IPC handler parses as SessionAction::Inject
  → dispatcher(action) → app.dispatch(action)
  → app.dispatch():
      if force==true  → direct PTY write (bypass idle gate)
      if force==false → InjectQueue.push() + InjectSignal.notify()
  → returns ActionResponse to telepty
```

### Registration Payload (telepty_bridge.rs:167-188)

```json
{
  "session_id": "workspace-name",
  "alias": "workspace-name",
  "command": "claude",
  "cwd": "/path/to/project",
  "delivery_type": "aterm",
  "delivery": {
    "transport": "unix_socket",
    "address": "/tmp/aterm-xxx.sock"
  },
  "term_program": "aterm",
  "term": "xterm-256color"
}
```

Telepty uses `delivery.address` to know WHERE to send inject messages. If the socket is stale (aterm crashed), telepty's inject attempt will fail with connection refused.

---

## 3. Integration Plan: InjectQueue → Mailbox Crate

### 3a. inject.rs — Core Queue Replacement

**Current:** `InjectQueue` (VecDeque, in-memory, capacity 256)
**Target:** Mailbox crate's persistent queue

| Component | Current | Mailbox Target |
|---|---|---|
| Storage | `VecDeque<InjectMessage>` | Persistent (file/mmap/sqlite) |
| Capacity | 256 hard limit, reject on full | Configurable, backpressure signal |
| ACK | None | Per-message delivery confirmation |
| Retry | None (write fail = dead) | Configurable retry with backoff |
| Crash recovery | All lost | Replay from persistent store |

**Changes needed:**

1. **`InjectQueue` struct**: Replace `VecDeque` backing with mailbox crate's queue handle. Keep the same public API (`push`, `pop`, `len`, `is_empty`, `clear`, `snapshot`, `oldest_enqueued_at`) for backward compatibility during migration.

2. **`InjectMessage`**: Add `message_id: u64` field for ACK tracking. The mailbox crate should assign IDs.

3. **`run_injector_loop()`**: After successful PTY write+flush of Enter:
   - Call `mailbox.ack(message_id)` to mark delivered
   - Emit `InjectDelivered` event (currently defined but never emitted)
   - On write failure: instead of clearing queue + marking dead, call `mailbox.nack(message_id)` for retry. Only mark dead after N consecutive failures.

4. **`SharedInjectQueue` type alias**: Update from `Arc<Mutex<InjectQueue>>` to mailbox crate's thread-safe handle (likely still Arc-wrapped).

5. **`InjectSignal`**: Keep as-is — the Condvar wake mechanism is sound. Mailbox crate should call `signal.notify()` on enqueue.

**Estimated scope:** ~150 lines changed in inject.rs

### 3b. app.rs — IPC Dispatch Updates

**Changes needed:**

1. **`dispatch(SessionAction::Inject)`** (line 544-576):
   - Currently gets `InjectQueue` from `inject_queues` HashMap
   - Change to get mailbox handle instead
   - Push returns a `message_id` for tracking
   - Response changes from `{"queued": N}` to `{"queued": N, "message_id": id}`

2. **`register_workspace()`** (line 240-279):
   - Currently accepts `SharedInjectQueue`
   - Change to accept mailbox handle
   - Mailbox initialization should happen here (or in PtyManager) with workspace-specific persistent path

3. **`deregister_workspace()`** (line 284-293):
   - Add: flush/close mailbox for workspace
   - Ensure pending messages are persisted before removal

4. **`workspace_exists()` / `workspace_state()`**: Update type references

**Estimated scope:** ~50 lines changed in app.rs

### 3c. pty.rs — PtyManager Wiring

**Changes needed:**

1. **Workspace struct** (line ~120):
   - `inject_queue: SharedInjectQueue` → mailbox handle type

2. **`PtyManager::spawn()`** (line ~279):
   - Create mailbox instance instead of `InjectQueue::new()`
   - Pass persistent storage path: `~/.aigentry/mailbox/{workspace-name}/`

3. **`queue_inject()`** (line 505-528):
   - Same flow but via mailbox API
   - Returns message_id in addition to queue length

4. **`inject_queue_for()` / `inject_signal_for()`** (line 532-538):
   - Update return types

5. **`mark_workspace_dead_handles()`** (line 655-670):
   - Instead of `queue.clear()`, call `mailbox.suspend()` — preserve messages for crash recovery
   - Messages can be replayed after workspace restart

**Estimated scope:** ~80 lines changed in pty.rs

### 3d. telepty_bridge.rs — Registration Payload

**Minimal changes:**

1. Registration payload may need new fields if mailbox requires delivery metadata:
   - `"ack_support": true` — tells telepty that aterm supports ACK
   - `"mailbox_version": 1` — protocol version

2. No structural changes to the bridge itself — it's fire-and-forget HTTP.

**Estimated scope:** ~5 lines changed

### 3e. aterm-session/src/action.rs — Event/Action Updates

1. **`ActionResponse` for Inject**: Include `message_id` field
2. **`AtermEvent::InjectDelivered`**: Already defined — needs to be actually emitted from `run_injector_loop()` after successful delivery
3. **New event**: `InjectFailed { workspace, message_id, reason }` for delivery failures
4. **New action**: `InjectAck { workspace, message_id }` — explicit ACK from external callers (optional, for bidirectional mailbox)

**Estimated scope:** ~20 lines changed

### 3f. aterm-ipc/src/server.rs — No Changes Expected

The IPC server is a generic JSON-over-Unix-socket transport. It doesn't know about InjectQueue internals. No changes needed unless the mailbox crate requires a different transport.

### 3g. aterm-ipc/src/auth.rs — No Changes

Auth is orthogonal to mailbox.

---

## 4. File Inventory

| File | Change Scope | Priority |
|---|---|---|
| `aterm-core/src/inject.rs` | **Heavy** — Queue replacement, loop rewrite, ACK emission | P0 |
| `aterm-core/src/pty.rs` | **Medium** — Type changes, spawn wiring, dead-marking | P0 |
| `aterm-core/src/app.rs` | **Medium** — Dispatch updates, register/deregister | P0 |
| `aterm-session/src/action.rs` | **Light** — New event/action variants | P1 |
| `aterm-core/src/telepty_bridge.rs` | **Minimal** — Payload fields | P2 |
| `aterm-core/src/lib.rs` | **Light** — FFI wrappers if new APIs exposed | P1 |
| `aterm-core/Cargo.toml` | **Minimal** — Add mailbox crate dependency | P0 |
| `macos/Sources/AppDelegate.swift` | **None** — Swift side doesn't touch inject internals | — |

**Total: 7 files modified, ~310 lines estimated**

---

## 5. Backward Compatibility

### Incremental Migration is Possible

The mailbox crate can be adopted incrementally:

**Phase 1: Drop-in replacement (no protocol change)**
- Replace `VecDeque` backing with mailbox storage
- Keep same `push/pop` API surface
- Keep same IPC JSON protocol
- External callers (telepty) see no difference
- **Risk: Low** — internal refactor only

**Phase 2: Add ACK support (additive protocol change)**
- Return `message_id` in inject response
- Emit `InjectDelivered` events
- External callers can opt-in to tracking
- Old callers ignore new fields (backward compatible)
- **Risk: Low** — additive only

**Phase 3: Persistent recovery (new behavior)**
- Messages survive crash/restart
- On workspace restart, replay undelivered messages
- Requires mailbox storage path convention
- **Risk: Medium** — new behavior on restart, needs testing

**Phase 4: Retry on write failure (behavior change)**
- Replace "first failure = dead" with retry logic
- Configurable retry count before marking dead
- **Risk: Medium** — changes failure semantics

### Critical Constraint

The `InjectSignal` (Condvar) mechanism must be preserved or the mailbox crate must provide equivalent event-driven wake. Polling-based alternatives would regress CPU efficiency.

---

## 6. Critical Findings

1. **InjectDelivered never emitted**: The event type exists but zero code paths emit it. Any ACK system must fix this first.

2. **Single write failure = total queue destruction**: `run_injector_loop()` clears ALL pending messages on first write error and marks workspace dead. No retry, no persistence. This is the highest-severity data loss vector.

3. **30s force-inject is the only escape valve**: If prompt detection fails (custom shell, no OSC 133), messages wait up to 30s. No configurable override, no per-workspace tuning.

4. **Capacity 256 with hard reject**: No backpressure, no overflow buffer. High-throughput inject scenarios (automated pipelines) can hit this limit.

5. **Two inject paths, one queue**: IPC (app.rs) and internal (pty.rs) both push to the same InjectQueue. Mailbox crate must handle concurrent access from both paths.

6. **Socket stale = silent failure**: If aterm crashes, telepty's socket address becomes stale. Telepty gets connection refused but has no retry/fallback. The stale session cleanup we just added (#129) mitigates discovery but not delivery.

---

## 7. Blockers

- **None for planning** — analysis is self-contained
- **For implementation**: Mailbox crate spec needed (API surface, persistence format, thread-safety model)
- **Dependency**: `aterm-core/Cargo.toml` must add the mailbox crate once published
