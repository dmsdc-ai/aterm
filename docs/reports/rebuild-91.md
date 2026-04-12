# Rebuild #91 — v3.1 transparent edge-to-edge integration

**Date:** 2026-04-12 11:05 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS (build + launch); v3.1 visual verification is user GUI scope
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Scope

aterm-claude v3.1 (REPORT ref cda0d8ab9d35). Single file change:

**OrchestratorInputBar.swift:**
- Background: `NSColor.clear` (transparent)
- cornerRadius: 16 → 0
- Edge-to-edge: all margins 0
- 1px hairline top separator restored (formerly removed in v3 Direction E)
- Focus animation: prompt tint only (no background shift)

Compared to v3 Direction E (Rebuild #90): `cornerRadius` reverts to 0, bar becomes fully transparent, and a thin top separator comes back as a minimal visual anchor. The intent is to let the terminal content show through to the bar area while still having a clear separation line.

## Narrow-typecheck false alarm — confirmed non-issue (5th time)

aterm-claude flagged the same "30 errors in SessionSidebarView under `swiftc -typecheck -target arm64-apple-macos13.0`" concern as in #89/#90. Per orchestrator guidance and prior evidence, this is an artifact of the narrow single-file typecheck closure not matching the real build's deployment target. Real `make app` build compiles clean under the project's macOS 14+ deployment target.

**Confirmed in #91:** full build PASS, zero errors.

## Build

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/OrchestratorInputBar.swift && \
  make app > /tmp/rebuild-91-build.log 2>&1
EXIT=0
```

### Results

- **Build: PASS** (`[build] App bundle created: /Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`)
- **Zero errors** across full 14-file closure
- **Zero warnings in OrchestratorInputBar.swift** (the only modified file this rebuild)
- Bundle size: **20 MB** (unchanged)
- Warning inventory identical to #90: 75 legacy rust wgpu-cfg dead-code + 3 SettingsView `onChange` deprecation warnings from sub1's conservative macOS 14+ fix (non-blocking, sub1 territory)
- TerminalView size_t errors absent for 5th consecutive rebuild (narrow-closure divergence)

## Launch

```
pkill -f 'build/aterm.app/Contents/MacOS/aterm'     # killed prior #90 sandbox cleanly
> /tmp/aterm-debug-live.log
cd ~/projects/aigentry-aterm && \
  ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data \
  ATERM_TELEPTY_PORT=13848 \
  ./build/aterm.app/Contents/MacOS/aterm > /tmp/aterm-debug-live.log 2>&1 &
```

### Process stats

```
PID=30933 RSS=125.4MB CPU=0.0% ETIME=00:13
```

Healthy boot, idle CPU, low RSS — same shape as #89/#90 baselines (125–126 MB at boot with all workspaces spawned, CPU settles to 0.0%).

### Launch log highlights (65 lines)

**Font plumbing preserved:**
```
[FONT-WARN] fallback chain for 'System Default' → userFixedPitchFont (Menlo-Regular)
```
Fifth consecutive rebuild where this fires. v3.1 edits are entirely visual (background color, corner radius, margins, separator) and did not touch the font resolution path.

**Zero NSLayoutConstraint warnings:**
```
grep -iE "NSLayoutConstraint|unable to simultaneously|cannot break|constraint.*will attempt"
(zero matches)
```

The v3.1 changes are:
- A bg color swap (`NSColor.clear`) which is a paint-time change, not a layout change
- A cornerRadius 16 → 0 which is a `layer.cornerRadius` float and does not affect Auto Layout at all
- Margins → 0 which collapses the bar's surrounding insets; still a coherent constraint set
- A re-added 1px hairline top separator view — this is a new view but with trivial top-pinned constraints (leading/trailing to container, height = 1)

None of these disturb the constraint graph. Zero NSLayoutConstraint complaints at startup confirms the layout math is still consistent.

**Zero runtime errors** (excluding expected telepty-bus WebSocket errors and shell-ready fallback timeouts — daemon on 13848 not running and claude/gemini/codex workspaces don't emit OSC 133;B).

**Zero runtime log output from the modified component.** OrchestratorInputBar doesn't carry log instrumentation — the v3.1 visual result (transparent bg, edge-to-edge, 1px hairline, prompt-tint focus fade) can only be confirmed by the user opening the sandbox window and looking at it.

## What this rebuild cannot verify

Per SAWP (builder scope), these require manual GUI verification by the user:

1. **Transparent background** — terminal content (or window chrome) visible through the input bar
2. **Edge-to-edge** — bar fills the full window width with zero side margins
3. **1px hairline top separator** — thin visible line separating the input bar from the terminal area above
4. **Focus animation** — prompt character (`>`) tint shifts on focus; background does NOT shift
5. **Comparison to mockup** — matches `/tmp/inputbar-v3-mockup.html` visual spec
6. **Korean IME preedit** — composition underline still clean at the new zero-corner zero-margin layout

## 0.2.11 publish gate

Per task instructions: **DO NOT publish 0.2.11** until user visually approves the running v3.1 sandbox. The build artifact (`./build/aterm.app`, 20 MB) is ready and can be published as-is with no further code change once approved.

## Artifacts

- Build log: `/tmp/rebuild-91-build.log`
- Launch log: `/tmp/aterm-debug-live.log`
- App bundle: `/Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`
- Running PID: 30933
- Visual spec mockup: `/tmp/inputbar-v3-mockup.html` (per task, for user comparison)

## Concerns / follow-ups

1. **v3.1 visual gate** — user must compare the running sandbox PID 30933 against the mockup and approve before publish. Build is ready; just waiting.

2. **Narrow-typecheck divergence (5th consecutive rebuild)** — aterm-claude keeps flagging 30 phantom errors in SessionSidebarView under their narrow single-file typecheck. Worth a follow-up to align their session's `swiftc -typecheck` target with the project deployment target so they stop seeing false positives. Out of this rebuild's scope.

3. **SettingsView 3 `onChange` deprecation warnings** — unchanged from #90. Sub1's macOS 14+ fix kept the deprecated API. Future migration cleanup recommended.

4. **Rust wgpu dead-code (75 warnings)** — unchanged, long-standing.

5. **Transparent background interaction with scrollback / selection** — if the terminal area behind the input bar has content that visually clashes with the prompt character, it could impair readability. User to judge during visual verification. This is a design tradeoff not a bug.
