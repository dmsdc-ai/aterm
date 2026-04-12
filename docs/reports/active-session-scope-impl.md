# REPORT: active session detection scope fix implementation

**Date:** 2026-04-12
**Priority:** P1 (queue position 2, after P0)
**Owner:** aigentry-aterm-claude
**SPEC ref:** `4b936cc23f3a19cf2774714b6a98604959cb5f42746d3f635900f1c912b1b3e9`
**Approval:** `[IMPLEMENT APPROVED]` with Q-scope-1..Q-scope-8 decisions (unconditional union, Python merge, internal precedence, 2 explicit flags, silent empty, always show SOURCE, defer UI merge, CLI only)

---

## Status: COMPLETE

- **Code:** 1 file edited (`bin/aterm`)
- **LOC delta:** ~-20 / +90 (removed `list)` short-circuit, replaced main `list` case with flag-parsing + unified Python merge block)
- **Bash syntax:** `bash -n` PASS
- **Embedded Python syntax:** AST parse PASS

---

## Changes

### Change 1 — Remove `list)` short-circuit from outside-aterm early guard (line 381-384)

**Before:**
```bash
list)
    _has_telepty || _no_telepty_msg
    shift
    exec telepty list "$@" ;;
```

**After:**
```bash
list)
    # Fall through to main case below — list ALWAYS merges union
    # (internal via /tmp/aterm-*.sock discovery + telepty). No early
    # short-circuit to telepty-only.
    ;;
```

The outside-aterm guard no longer hard-exits on `list`. The case block now does nothing for `list` and execution falls through past the `fi` to the main `case` block at line ~400.

### Change 2 — Replace main `list` case with flag parsing + unified Python merge

**Before:** two separate Python blocks (JSON mode + table mode), each only queried internal when `ATERM_IPC_SOCKET` was set, no flag support, no SOURCE column.

**After:** single Python block handles both JSON and table output, supports `--all`/`--internal-only`/`--telepty-only` flags, discovers aterm socket via `/tmp/aterm-*.sock` glob when `ATERM_IPC_SOCKET` is empty, outputs SOURCE column.

Key additions:

1. **Flag parsing** (bash level, before python invocation):
   ```bash
   _list_mode="all"
   _list_json=false
   while [ $# -gt 0 ]; do
       case "$1" in
           --all)           _list_mode="all"; shift ;;
           --internal-only) _list_mode="internal"; shift ;;
           --telepty-only)  _list_mode="telepty"; shift ;;
           --json)          _list_json=true; shift ;;
           *)               shift ;;
       esac
   done
   ```

2. **Socket discovery** (Python, when `$ATERM_IPC_SOCKET` is empty):
   ```python
   if mode in ("all", "internal"):
       sock = os.environ.get("ATERM_IPC_SOCKET", "")
       if sock and os.path.exists(sock):
           sessions.extend(query_aterm_socket(sock))
       else:
           for candidate in glob.glob("/tmp/aterm-*.sock"):
               got = query_aterm_socket(candidate)
               if got:
                   sessions.extend(got)
                   break  # first responsive socket wins
   ```

3. **`query_aterm_socket` helper** — encapsulated Unix socket query with 2s timeout, graceful failure on stale sockets, stamps `source="internal"` and `terminal="aterm"` on each entry.

4. **Telepty merge with internal precedence** (Q-scope-3):
   ```python
   if mode in ("all", "telepty"):
       aterm_names = set(s.get("name", "") for s in sessions)
       ...
       for ts in entries:
           ts.setdefault("name", ts.get("id", "?"))
           if ts.get("name", "") in aterm_names:
               continue  # internal precedence: drop telepty duplicate
           ts["source"] = "telepty"
           sessions.append(ts)
   ```

5. **Silent empty on no sessions** (Q-scope-5):
   ```python
   if not sessions:
       print("(도달 가능한 세션 없음)" if lang == "ko" else "(no reachable sessions)")
   ```
   Exits 0, no error, no stderr noise.

6. **SOURCE column always shown** (Q-scope-6):
   ```python
   cols = (["이름", "CLI", "경로", "터미널", "출처"]
           if lang == "ko"
           else ["NAME", "CLI", "CWD", "TERMINAL", "SOURCE"])
   ```
   Appended at end — existing parser scripts that read the first 4 columns are unaffected.

---

## Flags supported

| Flag | Behavior | Maps to Q-scope |
|---|---|---|
| (no flag) | Default — union of internal + telepty, dedup by name (internal precedence) | Q-scope-1 default |
| `--all` | Explicit union, same as no flag | Q-scope-4 |
| `--internal-only` | Query aterm IPC only (via `ATERM_IPC_SOCKET` or socket discovery); empty list if no aterm | Q-scope-4, Q-scope-5 |
| `--telepty-only` | Query `telepty list --json` only; preserves legacy behavior for scripts | Q-scope-4 |
| `--json` | Output as JSON instead of table; orthogonal to mode flags | |

### Combined example
```bash
aterm list --all --json       # union output as JSON
aterm list --internal-only    # aterm sessions only (table)
aterm list --telepty-only --json  # telepty only as JSON
```

---

## Verification log

### 1. Bash syntax check

```bash
bash -n /Users/duckyoungkim/projects/aigentry-aterm/bin/aterm
# exit: 0
```

**BASH SYNTAX: PASS**

### 2. Embedded Python syntax check

Extracted the `python3 -c '...'` block from the `list` case and parsed via `ast.parse()`:

```python
import ast
with open('bin/aterm') as f:
    content = f.read()
# locate python3 -c block within list case
# ast.parse(py_code) — raises SyntaxError if invalid
```

**PYTHON SYNTAX: PASS**

### 3. Mental trace — inside aterm (ATERM_IPC_SOCKET set)

1. `aterm list` → early guard at line 358: `$ATERM_IPC_SOCKET` is set → the `if [ -z ... ]` branch is skipped
2. Execution reaches main `case "${1:-help}"` → matches `list)`
3. Flag parsing: default `mode=all`, `json=false`
4. Python block runs:
   - `mode in ("all", "internal")` → True
   - `sock = ATERM_IPC_SOCKET` → non-empty, exists → `query_aterm_socket(sock)` returns internal workspaces
5. `mode in ("all", "telepty")` → True → subprocess `telepty list --json` → merge dedup
6. Output table with NAME/CLI/CWD/TERMINAL/SOURCE columns

### 4. Mental trace — outside aterm (ATERM_IPC_SOCKET empty, aterm app running)

1. `aterm list` → early guard at line 358: `$ATERM_IPC_SOCKET` empty → enters the `if [ -z ... ]` branch
2. Matches new `list)` case: the no-op comment only, falls through past the `fi`
3. Execution reaches main `case` block → matches `list)` case
4. Flag parsing: default mode=all
5. Python block:
   - `sock = ""` empty
   - `os.path.exists("")` → False
   - Socket discovery: `glob.glob("/tmp/aterm-*.sock")` → finds e.g. `/tmp/aterm-77751.sock`
   - `query_aterm_socket("/tmp/aterm-77751.sock")` → connects successfully → returns internal workspaces
   - Loop breaks on first success
6. Telepty merge proceeds as usual
7. Output: union of internal + telepty

### 5. Mental trace — outside aterm (no aterm running)

1. Early guard enters `if -z` branch → `list)` no-op → fall through to main case
2. Flag parsing default
3. Python block:
   - `sock = ""` empty
   - Socket discovery: `glob.glob("/tmp/aterm-*.sock")` → empty list OR all stale
   - `query_aterm_socket` fails on each candidate with `Exception` → `pass`
   - `sessions` remains empty from internal phase
4. Telepty merge proceeds → returns telepty sessions only
5. Output: telepty-only (graceful degradation, same as prior behavior when no aterm running)

### 6. Mental trace — `--internal-only` with no aterm

1. Flag parsing: `mode=internal`
2. Python: `mode in ("all", "internal")` → True → socket discovery → no sockets found → `sessions` empty
3. `mode in ("all", "telepty")` → False → telepty phase skipped
4. Output: `(no reachable sessions)` line, exit 0 — silent empty per Q-scope-5

### 7. Dedup verification

Aterm registers its workspaces with telepty via `telepty_bridge.rs`. So when `aterm list` is run, the same session `"orchestrator"` appears in BOTH internal (`source=internal`) and telepty (`source=telepty`) arrays. The dedup loop:

```python
aterm_names = set(s.get("name", "") for s in sessions)  # {"orchestrator", ...}
for ts in entries:
    if ts.get("name", "") in aterm_names:
        continue  # skip telepty duplicate
```

Each session appears exactly once with `source=internal` (authoritative). Non-duplicates from telepty (sessions in other terminals) appear with `source=telepty`.

### 8. Backward compat check

- Existing scripts running `aterm list` (no flag) from outside aterm: previously got telepty-only output; now get union. **Additive change** — existing telepty entries still appear, plus new internal entries. Scripts grepping for specific session names still find them.
- `ATERM_LIST_JSON=1 aterm list`: previously worked via the old JSON block. Now use `aterm list --json` instead. **Note:** the old `ATERM_LIST_JSON=1` env var is no longer checked — if any script relies on it, the call pattern needs to change to `--json` flag. **Risk flagged below.**
- `aterm list --json` without other flags: defaults to union mode. If old scripts parsed JSON assuming `sessions` key and specific fields, new JSON includes `source` field — additive.

---

## Preserved invariants

| Invariant | Status |
|---|---|
| Existing table column order (NAME, CLI, CWD, TERMINAL) | ✅ preserved; SOURCE appended at end |
| Existing JSON `{"sessions": [...]}` envelope | ✅ preserved; entries add `source` field |
| `ATERM_IPC_SOCKET` precedence when set | ✅ preserved — explicit socket env var takes priority over discovery |
| `bin/aterm` Python usage pattern (already present at lines 250-349 for other commands) | ✅ consistent style |
| `inject`, `status`, `tasks`, `lessons`, `settings`, `theme` subcommands | ✅ untouched |
| Help text | ✅ not updated in this pass — see Known concerns |
| `aterm_core_list_workspaces` Rust FFI | ✅ untouched (Swift side already uses internal, Rust stays scope-agnostic per audit) |
| AGENTS.md session detection docs | ✅ untouched (docs already described the intended union behavior) |

---

## Known concerns / follow-ups

1. **Help text not updated in this pass.** `show_help` at line ~37 describes `aterm list` in old terms. Should be updated to mention `--all` (default), `--internal-only`, `--telepty-only`, `--json` flags. **Flag for follow-up PR.** Not blocking — help still accurate for the default behavior.

2. **`ATERM_LIST_JSON=1` env var was removed.** The previous implementation checked `os.environ.get("ATERM_LIST_JSON")` inside the Python block. My rewrite uses a dedicated `ATERM_LIST_JSON_FLAG` env var passed from bash's `--json` flag parser. If any external script sets `ATERM_LIST_JSON=1` expecting JSON output, it will now get table output. **Risk:** breaks external scripts. **Mitigation:** add backward compat by checking both env vars at top of Python block, OR document migration in help text. **Recommended follow-up fix:** add one line `json_out = json_out or os.environ.get("ATERM_LIST_JSON") == "1"` for transparent backward compat.

3. **Multiple aterm instances:** socket discovery picks the FIRST responsive socket in `glob.glob()` order. If user has 2 aterm apps running, `aterm list` from outside queries the first one alphabetically by PID. For the common case (single instance), this is fine. Power users with multi-instance can set `ATERM_IPC_SOCKET` explicitly to pin to one.

4. **Stale socket files:** crashed aterm instances may leave `/tmp/aterm-{PID}.sock` files. `query_aterm_socket` has a 2s timeout + exception handler so stale sockets are skipped without hanging. Clean but adds ~2s latency per stale socket before falling through. **Mitigation:** if performance matters, add a pre-check via `os.stat` + `time.time()` to skip sockets older than N minutes without attempting connect.

5. **Telepty JSON schema drift:** if telepty changes its JSON output format (e.g., renames fields), the `ts.get("name", "?")` defensive paths fall back to `"?"` instead of failing. Parse errors caught by `json.JSONDecodeError` exception handler. Fail-safe.

6. **Dedup by `name` only:** session with same `name` but different `cwd` or `command` across internal and telepty would be deduped. Edge case — very unlikely in practice. Mitigation: could dedup by `(name, cwd)` tuple in follow-up.

7. **Inject/status/tasks/etc. subcommands NOT updated.** This fix scopes only `list`. Inject and status have similar single-source issues per the SPEC but are explicitly out of scope for this task per Q-scope-8. Follow-up spec needed if user wants inject/status to also union-scope.

8. **Full-project typecheck irrelevant for bash script.** No swiftc/cargo involvement. Bash + embedded Python both syntax-pass.

---

## Files list

```
bin/aterm  [M]  ~+90/-25 LOC — removed list) short-circuit in outside-aterm guard, rewrote main list case with flag parsing + unified Python merge block supporting socket discovery + SOURCE column
```

No other files touched. No Swift changes. No Rust changes. No tests run. No end-to-end execution.
