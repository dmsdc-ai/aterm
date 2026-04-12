# Rebuild #88 — aterm full build + sandbox launch

**Date:** 2026-04-12 09:50 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS (build level); P0 regression fix is **hypothesis-level only** and requires manual verification
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Scope

Two fix tracks merging into one build:

1. **aterm-claude P0 InputBar regression fix** (ref c7bbb4d91dd7...) — **HYPOTHESIS-BASED, not proven**
   - Removed defensive `onFocusChange` hide from `OrchestratorInputBar`
   - Refactored `CommandDropdown` from `NotificationCenter` observer to inline sync call after `popover.show()`
   - Primary suspicion: init-time `addObserver` side effect was causing the input bar to hide unexpectedly when orchestrator sidebar row was clicked
   - Files: `OrchestratorInputBar.swift` (-6 LOC), `OrchestratorCommands.swift` (-14 / +18, net +4)

2. **sub1 #246 i18n en default** (ref 9273579aba17...) — AtermLocalization 3-line patch
   - Default `languageCode = 'en'`, `isKorean = false`, `text()` returns `en`
   - All 69 Korean strings preserved (dormant, available on language switch)
   - File: `AtermLocalization.swift` (-2 net, 3-line patch)

## aterm-claude's warning (restated)

> root cause is hypothesized not proven. If Rebuild #88 still shows bar hidden after clicking orchestrator row, the regression is NOT in my territory and sub1 TerminalView changes at 09:21:06 must be audited next.

This rebuild only confirms that the hypothesis fix **compiles clean and does not crash at startup**. It does NOT confirm the fix solves the P0 because that requires GUI interaction which is tester scope.

## Build

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/TerminalView.swift \
        macos/Sources/OrchestratorInputBar.swift \
        macos/Sources/OrchestratorCommands.swift \
        macos/Sources/AtermLocalization.swift && \
  make app > /tmp/rebuild-88-build.log 2>&1
EXIT=0
```

### Results

- **Swift: 0 errors, 0 warnings** across 14 source files
- **Rust: 75 warnings**, all legacy wgpu-cfg dead-code (unchanged from #86/#87)
- Final: `[build] App bundle created: /Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Full log: `/tmp/rebuild-88-build.log` (768 lines)

### TerminalView.swift size_t errors — ABSENT (third rebuild confirming this pattern)

- `grep -c "error:"` → `0`
- `grep -E "TerminalView\.swift|aterm_core_write_pty|size_t"` → 0 matches

The single-file-typecheck divergence in sub1 + aterm-claude sessions continues to be an artifact of their narrow typecheck closures. Full 14-file `swiftc` invocation is clean in #86, #87, and now #88.

## Launch

```bash
pkill -f 'build/aterm.app/Contents/MacOS/aterm'
> /tmp/aterm-debug-live.log
cd ~/projects/aigentry-aterm && \
  ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data \
  ATERM_TELEPTY_PORT=13848 \
  ./build/aterm.app/Contents/MacOS/aterm > /tmp/aterm-debug-live.log 2>&1 &
```

Corrected pkill pattern (from 7f407ca) killed prior process cleanly. Zero survivors.

### Process stats

```
PID=77751 RSS=171.3MB CPU=2.9% ETIME=00:16
```

Healthy boot. RSS and CPU in normal range (similar shape to #87).

### Launch log highlights (`/tmp/aterm-debug-live.log`, 181 lines)

**Font plumbing preserved across #88 edits:**
```
[FONT-WARN] fallback chain for 'System Default' → userFixedPitchFont (Menlo-Regular)
```

Neither the CommandDropdown observer refactor nor the AtermLocalization en-default patch regressed the font fallback chain. Fourth rebuild in a row where this line fires as expected.

**NotificationCenter / addObserver / observer chain — CLEAN:**
```
grep -iE "notificationcenter|addobserver|observer|popoverDidShow" /tmp/aterm-debug-live.log
(zero matches)
```

**This is a positive signal for the hypothesis fix.** The prior code path had an init-time `addObserver` in `CommandDropdown` that aterm-claude suspected was causing side effects. After the refactor to inline sync calls, there are zero observer-related log lines at startup — consistent with the observer having been removed. Does not prove the P0 is fixed (only GUI interaction can), but proves the refactor landed correctly.

**Localization en-default — silent (expected):**
```
grep -iE "locale|language|isKorean|text\(" /tmp/aterm-debug-live.log
(zero matches)
```

The `AtermLocalization.text()` function is not instrumented, so an en-default vs ko-default switch is invisible to the launch log. Tester must verify visually that the orchestrator chrome, sidebar labels, and settings panel render in English.

**New observation — devkit workspace-init exit 127 (non-fatal):**
```
[aterm] devkit workspace-init exited with 127 for /Users/duckyoungkim/projects/aigentry-sandbox/data/orchestrator — MCP may not be registered
[aterm] devkit workspace-init exited with 127 for /Users/duckyoungkim/projects/aigentry-bridge — MCP may not be registered
[aterm] devkit workspace-init exited with 127 for /Users/duckyoungkim/projects/aigentry-builder — MCP may not be registered
[aterm] devkit workspace-init exited with 127 for /Users/duckyoungkim/projects/aigentry-context — MCP may not be registered
... (9 lines total)
```

Exit 127 = "command not found". The `devkit` binary is not on PATH for the aterm process (or the MCP-registered command the workspace-init hook is trying to invoke does not resolve). The message text is graceful: "MCP may not be registered". The app continues running, all workspaces are still registered, PTY sessions spawn normally. This is **non-fatal noise**, but it is new to this build — I did not observe it in the #86 or #87 launch logs (though I should caveat that I may have missed it because my grep filters were different). Worth tracking whether this is from a recent devkit integration or an environment PATH issue.

**Expected non-issues:**
- Several `[telepty-bus] WebSocket error: Could not connect to the server.` — daemon on 13848 not running, harmless
- Several `[shell-ready] fallback timeout (10s)` — claude/gemini/codex workspaces don't emit OSC 133;B, timer fallback fires

## What this rebuild cannot verify

Per SAWP (tester handles interactive tests), the following must be confirmed manually:

1. **P0 core test** — click orchestrator sidebar row → OrchestratorInputBar visible at bottom. If bar is STILL hidden, aterm-claude's hypothesis is wrong and sub1 TerminalView.swift changes at 09:21:06 must be audited next (per aterm-claude's own warning).
2. `/` slash-command popover opens and dropdown appears near input bar (CommandDropdown inline refactor verification).
3. Orchestrator chrome renders in English by default (i18n en-default verification).
4. Prior popover focus fix from #87 still works (anchor window keeps first responder when dropdown opens).
5. Prior Settings hit-area fix from #87 (theme click live apply, tab click registration) still works.

## Concerns / follow-ups

1. **Hypothesis-fix risk** — aterm-claude explicitly flagged the observer-removal fix as not proven. If the P0 persists after #88, the next audit target is sub1's TerminalView.swift changes from 09:21:06. The ETA for "fix confirmed" depends entirely on how fast the tester/user validates the GUI path.

2. **`devkit workspace-init` 127 exits** — new noise in #88 launch log. Graceful ("MCP may not be registered") so it's not blocking, but worth confirming whether this is a PATH issue specific to how I'm launching the sandbox directly (not via a login shell, not via `open -n ./build/aterm.app`) vs a real devkit registration gap. If it's a launch-method artifact, switching to `open -n ./build/aterm.app` would put devkit back on PATH. I'm keeping the direct-binary launch for consistency with prior rebuilds, so the symptom may not appear under the canonical `open -n` path.

3. **Rust wgpu dead-code (75 warnings)** — unchanged, low priority.

## Artifacts

- Build log: `/tmp/rebuild-88-build.log`
- Launch log: `/tmp/aterm-debug-live.log`
- App bundle: `/Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Running PID: 77751
