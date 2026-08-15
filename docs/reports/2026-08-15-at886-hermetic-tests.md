# at886 (#886) — cargo test reached the production telepty daemon. Fixed, guarded, PR open.

**PR:** https://github.com/dmsdc-ai/aterm/pull/2 — branch `fix/886-hermetic-tests`, worktree
`~/.aigentry/worktrees/at886`, base `origin/main` @ `9b4cec5`. **Not merged. No tags, no publish.**

---

## 1. The question the brief left unmeasured — answered: YES, today, by default

`cargo test` on `main` **connects to the production daemon on :3848** and sends the user's real
auth token from `~/.telepty/config.json`. This is not "one refactor away" — it happens on every run.

**How I measured it.** Two PATH shims in the scratchpad, ahead of the real binaries:

- `curl` — logs every invocation, then **hard-blocks** any argument containing `3848` (exit 7)
  and passes everything else through to `/usr/bin/curl`.
- `telepty` — logs and exits 0, never invoking the real CLI.

Run 1 (`cargo test --workspace`, both shims), log:

```
CURL -s -o /dev/null -w %{http_code} --max-time 1 http://127.0.0.1:3848/api/sessions -H x-telepty-token: cd35ac83-...
BLOCKED-3848 ...                          <- x3, the try_connect retry loop
CURL -s --max-time 3 http://127.0.0.1:64583/api/sessions ...   <- the pre-existing auth-header test, own ephemeral port, fine
```

Run 2 (`telepty` shim only, real network allowed), stderr:

```
[telepty-bridge] connected to daemon v0.8.0 (attempt 3)
TELEPTY --version                          <- detect_version() ran against the shim
```

So the connection is real and completes. Path:
`global_app()` → `AtermApp::new()` (`app.rs:116`) → `TeleptyBridge::try_connect()`.

**Which tests.** I ran each of the 57 lib tests individually against the shim. Exactly four reach it:

- `pty::tests::mark_workspace_dead_closes_writer_and_clears_queue`
- `pty::tests::pty_eof_no_polling_timer_involved`
- `pty::tests::pty_eof_notifies_condvar_waiters`
- `pty::tests::pty_eof_sets_status_to_dead`

(Only the first to run pays the cost — `global_app()` is a `OnceLock` — but which one that is depends
on thread scheduling.) The integration binary `polling_event_tests` does **not** reach it today; it
easily could, since `TeleptyBridge` and `AtermApp` are both `pub`.

**Can it reach `restart_daemon()`?** Yes, and the guard rail holding today is luck, not design:
the branch is `if inst != dmn`, and I measured installed CLI `0.8.0` == daemon `/api/health` `0.8.0`
(read-only GET, no state change). Any skew in **either** direction — precisely the state between a
`telepty` upgrade and the daemon restarting — and `cargo test` runs `telepty daemon restart` on the
production daemon. I never let that fire: the `telepty` shim was in place for every run where the
branch could have been reached.

## 2. Rule 39 — deltas found in the brief

All four cited facts hold, with two corrections:

- `:71` default 3848 — **correct**, but the brief omits that an `ATERM_TELEPTY_PORT` override
  already exists at `:68`. That override is what the fix reuses instead of adding new config.
- `:93-114` version-compare + restart — the block actually spans **`:97-119`**; `restart_daemon()`
  is called at `:103`.
- `:191-192` and `:401+` with 3 `#[test]` fns — **correct**.
- Pre-existing failures: brief said 2 (from d309067). Today's `main` has **3**, not 2.

## 3. The fix — two guards at the two hazard sites

Rule 29 surgical: 3 files, +80 lines net, production auto-restart untouched.

| Site | Guard |
| --- | --- |
| `resolve_port` (new, pure) | Under test, refuses the production port — including an explicit `ATERM_TELEPTY_PORT=3848`. Production resolution is behaviourally identical to the old `unwrap_or(3848)`, unparseable value included. |
| `restart_daemon` | Under test, refuses to exec. **Not redundant with the first**: a test pointed at a legitimate stand-in daemon that reports a different version still lands here, and `telepty daemon restart` acts on the real daemon whatever port the bridge was talking to. |

`hermetic()` = `cfg!(test)` (unit tests) **or** `ATERM_HERMETIC` (integration tests, which link the
lib built *without* `cfg(test)`). The env var comes from `.cargo/config.toml`, which already had an
`[env]` block. There is no bin target in this workspace — the shipped artifact is a cdylib loaded by
the Swift host — so it reaches build scripts and test binaries only, never a running aterm.

Root, not symptom: no test was skipped or `#[ignore]`d. The bridge no longer hands a test process
production config, so every current and future test inherits the property.

## 4. RED before GREEN

Each guard shown failing against a deliberately broken tree, then restored (files verified
byte-identical afterwards):

| Deliberate break | Guard that fired |
| --- | --- |
| delete the `hermetic()` check in `restart_daemon` | `hermetic_guards_still_in_source` — "restart_daemon lost its hermetic guard" |
| restore main's unguarded port resolution in `try_connect` | `hermetic_guards_still_in_source` — "try_connect no longer resolves its port through the hermetic guard"; **and** the shim log showed :3848 reached again, 6 lines |
| delete `ATERM_HERMETIC` from `.cargo/config.toml` | `hermetic_env_reaches_integration_tests` — "ATERM_HERMETIC not set" |

What the four guards assert:

- `production_port_never_resolves_under_test` — the resolver in both worlds: hermetic → `None` for
  unset / "3848" / junk, `Some(49152)` for a stand-in; production → `Some(3848)` unchanged.
- `restart_is_disarmed_under_test` — calling it is a no-op.
- `hermetic_guards_still_in_source` — source assertion (same shape as the repo's existing
  `ffi_tests::all_ffi_entry_points_wrapped_in_catch_unwind`): 3848 appears in code exactly once,
  `try_connect` still routes through `resolve_port`, `restart_daemon` still opens with the guard.
  This is the only thing that can catch *deletion* of a guard, which no behavioural test can see.
- `tests/hermetic_guard.rs` — catches deletion of the config entry, invisible to every unit guard.

## 5. Suite numbers — same command, same flags

`cargo test --workspace --no-fail-fast`

| | before (`9b4cec5`) | after |
| --- | --- | --- |
| tests | 75 | 80 |
| passed | 72 | 78 |
| failed | 3 | 2 |
| requests to :3848 | 3 | **0** |

The "after" run was plain — no shims at all, production daemon live — and logged
`[telepty-bridge] hermetic run: not connecting` with zero `connected to daemon` lines.

Remaining 2 failures are pre-existing and untouched:
`ffi_tests::all_ffi_entry_points_wrapped_in_catch_unwind` (15 FFI fns lack a panic guard),
`inject::tests::inject_queue_rejects_when_full`.

The third pre-existing failure, `pty::tests::pty_eof_no_polling_timer_involved`
("Dead marking should be near-instant (no timer), got 1.507415917s"), **now passes** — the 1.5s was
`try_connect`'s retry loop. A side effect of the fix, not a separate change.

## 6. Flagged as a separate task — NOT changed here

`restart_daemon()` restarts a daemon **aterm does not own**, triggered by a version-string mismatch
in either direction. A user whose `$PATH` telepty is *older* than the running daemon gets their
daemon restarted on every aterm launch, interrupting every session on it — including this
orchestrator's. I judge this a real hazard and worth its own task, but it is shipped product
behaviour and Rule 29 says it is not mine to change here.

## 7. What I did NOT verify

- **Swift side.** `AppDelegate.swift:358,380,1365` and `TeleptyBusClient.swift:129` default to 3848
  the same way. Rust-only scope, and no GUI build was run (reflexivity rule — the user's aterm.app
  is live).
- **CI.** No CI in this repo runs `cargo test`, so this only ever bit local runs. Not re-verified
  against any CI config.
- **Snyk.** `snyk_code_scan` MCP on `aterm-core` returned 0 issues. The output does not confirm the
  Rust files were actually analysed, so read it as "no findings, coverage unconfirmed" — not "clean".
- **Cross-crate.** `aterm-ipc` and `aterm-session` have 0 tests; I did not audit them for other
  production-reaching paths.

## 8. Constraint compliance

Production daemon on :3848 never restarted, stopped or started. No `launchctl`, no `session-cleanup`,
no `telepty kill/clean/delete`. No process killed that I did not spawn. No writes to
`~/.telepty/config.json` or `peers.json`. No test daemon spawned at all — the shim approach needed
none. aterm GUI untouched. Branch pushed, PR open, not merged.
