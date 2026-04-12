# SPEC: active session detection scope fix

**Status:** Draft — awaiting user approval
**Owner:** aigentry-aterm-claude
**Priority:** bug fix (session detection returns wrong scope by default)
**User report:** "aterm에서 액티브 세션 감지하라고 하면 aterm 내부 세션과 telepty로 연결된 세션 모두 감지해야돼. 그런데 지금은 telepty로 연결된 세션을 default로 감지하고 있어."
**Expected:** default query returns `union(internal_sessions, telepty_sessions)`
**Observed:** default query returns telepty-only (when outside aterm) OR internal-only (when inside aterm), never the union

---

## Goal

Make the default "list active sessions" query return the UNION of:
- aterm internal workspaces (from the running aterm app's IPC socket, via `ListWorkspaces` handler in `app.rs`)
- telepty external sessions (from `telepty list --json`)

regardless of whether the caller is running inside aterm (with `$ATERM_IPC_SOCKET` set) or outside.

Per `AGENTS.md > Session Detection`, the stated priority is (1) `aterm list`, (2) `telepty list`, (3) never `ps aux`. The current implementation silently loses one of the two sources depending on context, violating the documented contract.

---

## Audit findings (evidence, with file:line references)

### Q1 — Which entry points currently default to telepty-only?

| # | Entry point | File:line | Current default | Returns |
|---|---|---|---|---|
| 1 | `aterm list` CLI — **outside aterm** (no `$ATERM_IPC_SOCKET`) | `bin/aterm:381-384` | `exec telepty list "$@"` | **telepty only** — no aterm internal sessions |
| 2 | `aterm list` CLI — **inside aterm** (`$ATERM_IPC_SOCKET` set) | `bin/aterm:259` + Python block `bin/aterm:299-333` | Sends `{"action":"ListWorkspaces"}` to IPC socket | **aterm internal only** — no telepty sessions (unless they also happen to be registered aterm workspaces) |
| 3 | `aterm_core_list_workspaces` FFI | `aterm-core/src/lib.rs:2283` | `pty_manager.list_workspaces()` | aterm internal only |
| 4 | `ListWorkspaces` action handler | `aterm-core/src/app.rs:578-580` | `host.list_workspaces()` → AppDelegate callback `AppDelegate.swift:434` | aterm internal only |
| 5 | `TeleptyBusClient.loadInitialSessions()` | `TeleptyBusClient.swift:159-181` | Runs `/usr/bin/env telepty list --json` | **telepty only** — populates `self.sessions` |
| 6 | `TeleptyBusClient.refreshWorkspaces()` | `TeleptyBusClient.swift:142-151` | Calls `aterm_core_list_workspaces(core)` | aterm internal only — populates `self.workspaces` |
| 7 | `SessionSidebarView.fetchWorkspacesFromIPC()` | `SessionSidebarView.swift:382` | IPC `ListWorkspaces` request | aterm internal only |

**Key observation:** the UI (`TeleptyBusClient`) already has TWO separate buckets (`workspaces` for internal, `sessions` for telepty) but they're never merged into a unified list. The sidebar only shows internal workspaces via `fetchWorkspacesFromIPC` — telepty external sessions are loaded but not displayed in the workspace list. Merging is the responsibility of the consumer, and no consumer currently does it.

### Q2 — Where is internal session registry? Does it expose a getter?

- **Primary source:** `PtyManager::list_workspaces()` in `aterm-core/src/pty.rs:564` — returns `Vec<WorkspaceInfo>` for workspaces spawned by the aterm app process
- **IPC-exposed getter:** `ListWorkspaces` action in `app.rs:578-580` — serialized to JSON over the Unix socket at `/tmp/aterm-{PID}.sock`
- **FFI getter:** `aterm_core_list_workspaces` in `lib.rs:2283` — returns JSON string for Swift side
- **Swift AppDelegate callback:** `callbacks.list_workspaces` wired at `AppDelegate.swift:434` — forwards to `managedWorkspaces` dictionary which includes all aterm-created workspaces (user-created + isSystem orchestrator + ephemeral IPC-created)

### Q3 — What is the right default for 'active sessions' query?

**Recommended:** UNION of internal + telepty, with deduplication. Rationale:
- `AGENTS.md` explicitly documents internal + external as TWO separate scopes that should both be covered
- `telepty list` only shows external sessions (other terminals, other machines) if telepty registration is active
- aterm internal sessions should be the primary source of truth for workspaces running INSIDE aterm, since the aterm app has authoritative state
- A user asking "what sessions are active?" means ALL of them, not a subset filtered by transport

**Alternative:** default to union, but allow opt-in to filtered scope via flag (`--internal-only` or `--telepty-only`). Preserves backward compat for any caller that expected a single scope.

### Q4 — Backward compat: can callers opt-in to telepty-only?

- **Inside aterm:** current default is internal-only. No existing caller expects telepty-only from `aterm list` (the inside path has NEVER returned telepty entries). Adding telepty to the union is purely additive.
- **Outside aterm:** current default is telepty-only via `exec telepty list`. Callers running outside aterm and using `aterm list` currently see telepty results. Adding internal sessions is additive in the common case (aterm is running = internal entries appear; aterm not running = current behavior preserved).
- **Proposed opt-in flag:** `aterm list --telepty-only` and `aterm list --internal-only` as explicit filters. Default (no flag) = union.

### Q5 — Interaction with #241 telepty desync (registry staleness)

If `telepty list` can return stale/desynchronized data, the UNION default should PREFER internal entries when there's a conflict. Concretely:
- If a session with id `X` appears in BOTH `aterm list` (internal) and `telepty list` (stale), use the internal version — it has authoritative status, CWD, CLI, etc.
- Internal source of truth takes precedence on deduplication
- This actually MITIGATES #241: users get correct data for in-aterm sessions even when telepty registry is stale

### Q6 — Does architect #76 ADR (dynamic sub-session lifecycle) depend on this fix?

- #76 assumes that when a parent session spawns child sessions via dispatch/orchestration, the children should be discoverable via active session detection
- Child sessions are typically created INSIDE aterm as new workspaces → they appear in aterm internal list, NOT in telepty list until telepty registration completes (which may lag)
- Current default (telepty-only when outside aterm) would MISS freshly-spawned child sessions → #76's lifecycle assertions would silently fail
- This fix **unblocks #76** by ensuring the union default surfaces newly-spawned children immediately, before any telepty registration lag

---

## Root cause location

The scope filter lives in TWO places, each defaulting to a single source:

1. **`bin/aterm:381-384`** (outside-aterm fallback path):
   ```bash
   list)
       _has_telepty || _no_telepty_msg
       shift
       exec telepty list "$@" ;;
   ```
   The `exec` replaces the current process with telepty, so there's no chance to merge. It's a hard short-circuit.

2. **`bin/aterm:259-349`** (inside-aterm Python block):
   ```python
   payloads = {
       "list": lambda: {"action": "ListWorkspaces"},
       ...
   }
   ```
   Only sends `ListWorkspaces` to the IPC socket. Never invokes `telepty list` for merging.

Both paths are mutually exclusive (gated on `$ATERM_IPC_SOCKET`), and neither produces a union.

---

## Files to modify

| # | File | Change |
|---|---|---|
| 1 | `bin/aterm` | Rewrite `list` command handling in BOTH modes (inside + outside aterm) to produce the union. Inside mode: query IPC for internal + shell out to `telepty list --json` + merge. Outside mode: discover aterm IPC socket via `/tmp/aterm-*.sock` glob; if found, query it for internal; always shell out to `telepty list --json`; merge. |
| 2 | `bin/aterm` (help text) | Update `show_help` to document `--all` (default), `--internal-only`, `--telepty-only` flags |

**NOT touching:**
- `aterm-core/src/**` — Rust core remains scope-agnostic; the merge happens at the CLI layer
- `macos/Sources/**` — Swift side is not affected; `SessionSidebarView` already uses `fetchWorkspacesFromIPC` and that's fine for the in-app UI
- `TeleptyBusClient.swift` — already has separate `sessions` and `workspaces` buckets; merging at the consumer level is out of scope for this fix
- `AGENTS.md` session detection docs — the stated priority order is already correct; only the implementation needs to match

---

## Fix approach (proposed)

### Change 1 — Add a `_list_merged` helper function in `bin/aterm`

```bash
# Merge aterm internal + telepty sessions.
# Preference: internal > telepty on dedupe by session id.
# Output format: JSON array of {name, cli, cwd, terminal, status, source}
_list_merged() {
    local internal_json="[]"
    local telepty_json="[]"

    # 1. Internal sessions — query aterm IPC socket
    if [ -n "${ATERM_IPC_SOCKET:-}" ] && [ -S "$ATERM_IPC_SOCKET" ]; then
        internal_json="$(ATERM_LIST_JSON=1 _send_ipc list 2>/dev/null | jq -c '.sessions // []' 2>/dev/null || echo '[]')"
    else
        # Discover aterm IPC socket via /tmp/aterm-*.sock glob
        local candidate
        for candidate in /tmp/aterm-*.sock; do
            [ -S "$candidate" ] || continue
            internal_json="$(ATERM_IPC_SOCKET="$candidate" ATERM_LIST_JSON=1 _send_ipc list 2>/dev/null | jq -c '.sessions // []' 2>/dev/null || echo '[]')"
            [ "$internal_json" != "[]" ] && break
        done
    fi

    # 2. Telepty sessions — shell out to telepty list --json
    if command -v telepty >/dev/null 2>&1; then
        telepty_json="$(telepty list --json 2>/dev/null || echo '[]')"
    fi

    # 3. Merge with internal taking precedence on duplicate id/name
    jq -n \
        --argjson internal "$internal_json" \
        --argjson telepty "$telepty_json" \
        '[($internal[] | . + {source: "aterm"}),
          ($telepty[] | . + {source: "telepty"})]
         | group_by(.id // .name)
         | map(if any(.source == "aterm") then map(select(.source == "aterm"))[0] else .[0] end)'
}
```

### Change 2 — Rewrite `list` subcommand to use `_list_merged`

```bash
# Inside aterm path:
case "${1:-help}" in
    list)
        shift
        # Parse optional filter flags
        local mode="all"
        while [ $# -gt 0 ]; do
            case "$1" in
                --internal-only) mode="internal"; shift ;;
                --telepty-only) mode="telepty"; shift ;;
                --all) mode="all"; shift ;;
                *) break ;;
            esac
        done

        case "$mode" in
            internal) _send_ipc list ;;  # existing behavior
            telepty) exec telepty list "$@" ;;  # existing behavior
            all) _list_merged | _format_table ;;  # NEW default
        esac
        exit 0
        ;;
esac
```

And the outside-aterm branch:

```bash
# Outside aterm path (no $ATERM_IPC_SOCKET):
list)
    shift
    local mode="all"
    while [ $# -gt 0 ]; do
        case "$1" in
            --internal-only) mode="internal"; shift ;;
            --telepty-only) mode="telepty"; shift ;;
            --all) mode="all"; shift ;;
            *) break ;;
        esac
    done

    case "$mode" in
        internal)
            # Discover aterm socket; error if not found
            local sock
            for sock in /tmp/aterm-*.sock; do
                [ -S "$sock" ] || continue
                ATERM_IPC_SOCKET="$sock" _send_ipc list
                exit 0
            done
            echo 'Error: no running aterm app found' >&2
            exit 1
            ;;
        telepty)
            _has_telepty || _no_telepty_msg
            exec telepty list "$@"
            ;;
        all)
            _list_merged | _format_table  # NEW default
            exit 0
            ;;
    esac
    ;;
```

### Change 3 — Add a `_format_table` helper

```bash
_format_table() {
    # Read JSON array from stdin and format as NAME | CLI | CWD | TERMINAL | SOURCE
    jq -r '
        (["NAME","CLI","CWD","TERMINAL","SOURCE"] | @tsv),
        (.[] | [.name // .id, .cli // .command, .cwd, .terminal // "aterm", .source] | @tsv)
    ' | column -t -s$'\t'
}
```

### Change 4 — Update `show_help`

Document the new flags:

```
aterm list [--all | --internal-only | --telepty-only]
    --all           (default) show aterm internal + telepty sessions, merged
    --internal-only show only aterm internal workspaces
    --telepty-only  show only telepty external sessions
```

---

## Backward compatibility

| Caller | Before | After |
|---|---|---|
| `aterm list` (no flag, inside aterm) | Internal only | **Union** (internal + telepty) |
| `aterm list` (no flag, outside aterm, aterm running) | Telepty only | **Union** (internal via auto-discovered socket + telepty) |
| `aterm list` (no flag, outside aterm, aterm NOT running) | Telepty only | Telepty only (graceful fallback — internal list is empty) |
| `aterm list --internal-only` | (new flag, didn't exist) | Internal only |
| `aterm list --telepty-only` | (new flag, didn't exist) | Telepty only (equivalent to prior outside-aterm default) |
| `aterm list --all` | (new flag, didn't exist) | Union |
| `ATERM_LIST_JSON=1 aterm list` (JSON output for scripts) | Internal only (inside) or telepty only (outside) | Union JSON output — includes `source` field per entry |

**Breaking change risk:** scripts that parse `aterm list` table output may see additional columns and rows after the fix. Mitigation: the `SOURCE` column is appended, existing columns (`NAME`, `CLI`, `CWD`, `TERMINAL`) remain in the same order. Scripts that grep for specific session IDs will still find them (plus new entries).

---

## Verification (no app run required)

1. **Mental trace — inside aterm:**
   - Run `aterm list` inside aterm app
   - `_list_merged` called
   - Step 1: `$ATERM_IPC_SOCKET` is set and is a socket → query internal via `ATERM_LIST_JSON=1 _send_ipc list` → parse JSON → get internal workspace array
   - Step 2: `telepty` available → run `telepty list --json` → get telepty session array
   - Step 3: `jq` merges with internal precedence → output union
   - Pipe to `_format_table` → table output with NAME/CLI/CWD/TERMINAL/SOURCE columns

2. **Mental trace — outside aterm (aterm app running):**
   - Run `aterm list` from regular shell (no `$ATERM_IPC_SOCKET`)
   - Discovery loop finds `/tmp/aterm-{PID}.sock`
   - `ATERM_IPC_SOCKET=/tmp/aterm-{PID}.sock` is re-injected into `_send_ipc` scope
   - Same merge flow as inside aterm
   - Result: union

3. **Mental trace — outside aterm (aterm app NOT running):**
   - Discovery loop finds no sockets in `/tmp/aterm-*.sock`
   - `internal_json` stays `[]`
   - Telepty query returns its own sessions
   - Merge produces telepty-only result (same as prior behavior)
   - No regression for users who only use telepty

4. **Dedup verification:**
   - If a session `"orchestrator"` appears in BOTH `aterm list` and `telepty list` (which happens because aterm registers its workspaces via telepty_bridge), the merge logic picks the aterm entry (source priority) and drops the duplicate telepty entry
   - Result: each session appears exactly once in the output

5. **`swiftc -typecheck`:** N/A — this is a bash/Python CLI change, not Swift code. Shell-level testing suffices.

6. **Unit test plan (future):**
   - Create a test harness that mocks `/tmp/aterm-*.sock` with canned responses
   - Mock `telepty list --json` with canned responses
   - Run `aterm list`, `aterm list --all`, `aterm list --internal-only`, `aterm list --telepty-only`
   - Assert output format and dedup correctness

---

## Risks

1. **Dedup collision when same session ID exists in both sources** — aterm workspaces are registered with telepty_bridge, so the SAME session may appear in both `aterm list` and `telepty list`. Without dedup, user sees duplicates. Mitigation: `jq` `group_by` with internal precedence already handles this.

2. **`jq` dependency** — the merge uses `jq` for JSON manipulation. If `jq` is not installed on the user's system, the fix breaks. Mitigation: most macOS dev setups have `jq` (it's in homebrew defaults); add a `command -v jq` check and fall back to internal-only or telepty-only with a warning if absent. Alternative: rewrite in Python (which is already used extensively in `bin/aterm` at line 250-349).

3. **Socket discovery across multiple aterm instances** — if the user has TWO aterm app instances running, `/tmp/aterm-*.sock` matches both. The fix picks the first successful response. For the common case (single instance), this is fine. For power users with multiple instances, they can set `$ATERM_IPC_SOCKET` explicitly to pin to one.

4. **Performance** — the merged path spawns an additional `telepty list --json` subprocess on every `aterm list` call. Latency overhead is ~50-200ms. Mitigation: acceptable for CLI use (interactive, not a hot path); cache if needed later.

5. **`telepty list --json` schema drift** — if telepty changes its JSON output format, the merge breaks. Mitigation: use defensive `jq` paths (`.id // .name`, `.command // .cli`) and `|| echo '[]'` fallback on parse error.

6. **Discovered socket is stale** — if a previous aterm crashed without cleaning `/tmp/aterm-*.sock`, the stale socket file exists but no process listens. The `_send_ipc` call times out (5s). Mitigation: quick `socket.connect` with short timeout; skip stale sockets. Could also check `lsof` or `fuser` to verify a live listener, but that's slow.

7. **`ATERM_LIST_JSON=1` scope** — this env var triggers JSON output in the Python block (line 305). Setting it locally in `_list_merged` may leak to subsequent commands if not scoped properly. Mitigation: use subshell `( ATERM_LIST_JSON=1 ... )` for strict scope.

8. **Python block requires aterm IPC JSON payload** — the Python block at `bin/aterm:250-349` handles IPC communication. The merge helper needs to be reachable from Python output too, OR the merge should be done purely in bash+jq+telepty to avoid Python nesting complexity.

---

## Open questions (for user decision)

1. **Q-scope-1** — Default scope: `--all` (union) vs. `--internal-first-with-telepty-fallback`? Currently proposing unconditional union. An alternative is "if aterm is running, union; otherwise telepty-only". Both are defensible.

2. **Q-scope-2** — `jq` dependency acceptable, or should the merge be rewritten in Python (already used in `bin/aterm`)? Python is already present, so no new dep — this is the safer choice.

3. **Q-scope-3** — Dedup precedence: internal > telepty (proposed) or telepty > internal? Internal has more accurate status for in-aterm sessions; telepty has cross-terminal visibility. Going with internal > telepty per Q5 rationale.

4. **Q-scope-4** — `--internal-only` and `--telepty-only` flag names: acceptable? Or prefer `--scope=internal|telepty|all`?

5. **Q-scope-5** — Error behavior when `aterm list --internal-only` is run and no aterm app is discoverable. Current proposal: print error and exit 1. Alternative: silent empty output + exit 0.

6. **Q-scope-6** — Should the SOURCE column always be shown in the default `aterm list` output, or only when mixed sources are present? Always-shown is more predictable but adds visual noise.

7. **Q-scope-7** — `TeleptyBusClient.swift` has separate `sessions` and `workspaces` arrays. Should those also be merged at the UI level for consistency? This spec scopes the fix to the CLI only; UI merging can be a follow-up.

8. **Q-scope-8** — Does this fix need to update `macos/Sources/SessionSidebarView.swift` to also show telepty sessions in the sidebar? Currently the sidebar only shows aterm internal workspaces (via `fetchWorkspacesFromIPC`). If the user's intent is "UI shows all active sessions", this spec is insufficient. If the intent is "CLI returns all active sessions", this spec is complete. **Recommend deferring UI merge to a follow-up spec** — the CLI fix alone restores the documented `AGENTS.md` contract and unblocks #76.

---

## Out of scope for this fix

- `macos/Sources/SessionSidebarView.swift` merge (separate spec — UI consistency)
- `TeleptyBusClient.swift` bucket unification (separate spec — UI model refactor)
- `AGENTS.md` documentation changes (current docs are correct; only implementation needs to match)
- `#241` telepty desync root-cause fix (separate task — data pending)
- `#76` architect ADR (separate task — this fix unblocks but doesn't implement)
- `aterm inject`, `aterm status` scope — same pattern applies but out of scope for this fix; log as follow-up

---

## NO CODE CHANGES MADE. Awaiting `[IMPLEMENT APPROVED]` from orchestrator.
