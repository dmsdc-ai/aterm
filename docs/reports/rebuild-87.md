# Rebuild #87 — aterm full build + sandbox launch

**Date:** 2026-04-12 09:21 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Scope

Verifying two parallel fix tracks merging into one build:

1. **aterm-claude popover focus root fix** (ref cfba2e069157...)
   3-layer defense to prevent NSPopover `.transient` from stealing first responder:
   - `DropdownTableView.acceptsFirstResponder = false`
   - `popoverDidShow` observer restores anchor window focus
   - `anchorWindow.makeKey()` explicit restoration
   - `becomesKeyOnlyIfNeeded` via NSPanel cast (best-effort)
   - Prior `DropdownTableView.keyDown`-from-Enter belt-and-suspenders retained
   - Files: `OrchestratorCommands.swift`, `OrchestratorInputBar.swift`

2. **sub1 Settings UX fix** (ref 82f289e294d0...)
   `contentShape(Rectangle())` added to `SchemeButton` and `SettingsTab` button label.
   Root cause of theme live-apply issue and tab hit area issue was hit-test gap, not wiring.
   File: `SettingsView.swift`

## Build

Force-touched all 4 modified Swift files to make sure the compiler re-checked them:

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/TerminalView.swift \
        macos/Sources/OrchestratorCommands.swift \
        macos/Sources/OrchestratorInputBar.swift \
        macos/Sources/SettingsView.swift && \
  make app > /tmp/rebuild-87-build.log 2>&1
EXIT=0
```

### Results

- **Swift: 0 errors, 0 warnings** across all 14 source files
- **Rust: 75 warnings**, all legacy wgpu-cfg dead-code (unchanged from #86, known)
- Final: `[build] App bundle created: /Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Full log: `/tmp/rebuild-87-build.log` (768 lines)

### TerminalView.swift size_t errors — ABSENT again

Both sub1 and aterm-claude REPORTs still flag the 4 pre-existing errors at `aterm_core_write_pty` Int/UInt bridging sites (lines 764, 809, 1237, 1382) within their single-file typecheck closures. #86 already proved these are absent under the full 14-file build closure. #87 confirms again:

- `grep -c "error:"` → `0`
- `grep -E "TerminalView\.swift|aterm_core_write_pty|size_t"` → 0 matches (excluding the swiftc input-file argument line)

These 4 errors are a single-file-scope artifact and disappear under full closure. Not a blocker, not a STUCK.

## Launch

```bash
pkill -f 'build/aterm.app/Contents/MacOS/aterm'    # CORRECTED PATTERN — worked, 0 survivors
> /tmp/aterm-debug-live.log
cd ~/projects/aigentry-aterm && \
  ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data \
  ATERM_TELEPTY_PORT=13848 \
  ./build/aterm.app/Contents/MacOS/aterm > /tmp/aterm-debug-live.log 2>&1 &
```

**pkill verification:** the corrected binary-path pattern (committed in 7f407ca) killed the prior sandbox process cleanly with zero survivors. No stale-process cleanup needed this cycle.

### Process stats

```
PID=36435 RSS=165.8MB CPU=2.2% ETIME=00:17
```

RSS is noticeably lower than #86's 448.8MB measurement. #86's measurement was taken after 1:14 of runtime with 6 fully-spawned workspace PTYs and an atlas that had grown 512→1024; #87's is at 17s. Fair comparison would be taking #87's RSS at a similar runtime, but for our purposes 165.8MB at boot with healthy CPU drop (2.9% → 2.2%) is nominal.

### Launch log highlights (`/tmp/aterm-debug-live.log`, 180 lines)

**Font plumbing preserved across #87 edits:**
```
[FONT-WARN] fallback chain for 'System Default' → userFixedPitchFont (Menlo-Regular)
```

The 4-tier fallback chain from aterm-claude's prior font work is still operating end-to-end. Neither the popover fix nor the SettingsView hit-area fix regressed it.

**NSPopover / NSPanel / responder chain — CLEAN:**
```
grep -iE "popover|becomesKey|NSPanel|responder|anchorWindow" /tmp/aterm-debug-live.log
(zero matches)
```

Absence is **expected and good** — the popover focus fix is dormant at startup because no dropdown popover has been opened yet. The fix only kicks in when the user types `/`, `@`, or `#` into the input bar, which triggers dropdown presentation. Interactive verification of the 3-layer defense (acceptsFirstResponder=false, popoverDidShow observer, anchorWindow.makeKey, NSPanel cast) is tester scope.

No `becomesKeyOnlyIfNeeded NSPanel cast failure` warnings. No `first responder` complaints. No Metal or responder chain errors.

**Expected non-issues:**
- Several `[telepty-bus] WebSocket error: Could not connect to the server.` — daemon on 13848 not running, harmless
- Several `[shell-ready] fallback timeout (10s)` — workspaces launching claude/gemini/codex don't emit OSC 133;B, timer fallback fires

## Known concerns — none of them manifested at startup

| Concern | Status at startup |
|---|---|
| NSPopover internal window class changes — NSPanel cast may silently fail | Not triggered (no popover opened yet); primary `anchorWindow.makeKey` defense is independent of the cast and will work regardless |
| Settings tab click registration at startup | No crash observed; GUI hit-area verification is tester scope |
| First-frame flicker on popover show | Not triggered (no popover opened yet) |
| TerminalView 4 pre-existing size_t errors | **ABSENT under full build closure — confirmed for the second rebuild in a row** |

## Artifacts

- Build log: `/tmp/rebuild-87-build.log`
- Launch log: `/tmp/aterm-debug-live.log`
- App bundle: `/Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Running PID: 36435

## Concerns / follow-ups

1. **TerminalView single-file-typecheck divergence persists** — sub1 and aterm-claude sessions keep seeing 4 `size_t` errors in their narrow typecheck closures, but full-closure builds don't. Worth investigating at some point whether the sessions are running `swiftc -typecheck` without `-import-objc-header` or without the full file set, causing them to see a different view of `aterm_core_write_pty`'s signature. Not urgent — full build is authoritative.

2. **Popover focus fix effectiveness is tester-gated** — this rebuild only confirms the edits compiled and don't crash at startup. The 3-layer defense must be validated interactively (open a `/` slash-command popover, verify the anchor window keeps first responder, verify typing continues to land in the input field). Tester scope.

3. **Rust wgpu dead-code (75 warnings)** — unchanged from #86, should be cleaned up in a future refactor.
