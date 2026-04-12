# REPORT: Task A (CURSOR-DIAG cleanup) + Task B (auto-restart retry strip)

**Date:** 2026-04-12
**Priority:** 0.2.11 release prep

## Task A — CURSOR-DIAG log removal: COMPLETE

Removed all 7 diagnostic log lines from 6 files:
1. `aterm-core/src/renderer.rs` — removed eprintln at renderer init
2. `aterm-core/src/lib.rs` — removed eprintln in aterm_core_set_line_height
3. `macos/Sources/GlyphAtlas.swift` — removed NSLog on first CJK fallback + removed `didLogFontDiag` property
4. `macos/Sources/TerminalView.swift` — removed NSLog on first render
5. `macos/Sources/OrchestratorInputBar.swift` — removed NSLog at setup end
6. `macos/Sources/AppDelegate.swift` — removed NSLog for intermediate values, collapsed `rawMax` back to inline `ceil(max(...))`

Verification: `grep -rn 'CURSOR-DIAG'` returns zero matches.

## Task B — Auto-restart retry flag stripping: COMPLETE

**Root cause:** `pty.rs:1495-1499` stripped only `--continue` on retry. Codex's `resume` and `--last` were NOT stripped, so codex retries repeated the same failing `resume --last` command 3 times then gave up.

**Fix:** expanded the filter at `pty.rs:1495-1505` to strip ALL resume/continuation flags:
- `--continue` (claude, gemini)
- `resume` (codex subcommand)
- `--last` (codex flag)

So `codex resume --last --dangerously-bypass-approvals-and-sandbox` retries as `codex --dangerously-bypass-approvals-and-sandbox` (starts fresh session).

**B2 (exit-0 fast-exit):** investigation showed auto_restart already fires on ANY process exit regardless of exit code (code at pty.rs:1492 only checks `!is_closing && auto_restart`, no exit code gate). B1's flag stripping is the missing piece — once resume flags are stripped, the fresh-start retry succeeds even for exit-0 fast-exits.

**cargo check:** PASS | **swiftc:** PASS
