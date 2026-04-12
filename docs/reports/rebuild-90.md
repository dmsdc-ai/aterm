# Rebuild #90 — v3 Direction E Corrected + borderless integration

**Date:** 2026-04-12 10:50 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS (build + launch); v3 visual verification is tester/user GUI scope
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Scope

aterm-claude v3 Direction E Corrected + borderless directive (REPORT ref 63f6b22b935b). Files modified:

1. **OrchestratorInputBar.swift**
   - cornerRadius 8 → 16
   - verticalPadding 9 → 16
   - horizontalPadding → 20
   - separatorView REMOVED (borderless)
   - border α = 0
   - prompt regular weight + α = 0.65
   - placeholder non-italic + α = 0.55
   - focus lightness shift 0.08
   - 48pt minHeight single-line preserved

2. **SessionSidebarView.swift**
   - line 850 Divider above Settings button REMOVED
   - line 726 top Divider preserved
   - Settings button `contentShape(Rectangle())` added (hit area fix)

aterm-claude self-audit: all v2 functionality preserved. swiftc -typecheck PASS on 6-file closure.

## Build

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/OrchestratorInputBar.swift macos/Sources/SessionSidebarView.swift && \
  make app > /tmp/rebuild-90-build.log 2>&1
EXIT=0
```

### Results

- **Build: PASS** (`[build] App bundle created: /Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app`)
- **Zero errors** across full 14-file closure (TerminalView size_t errors still absent — 4th consecutive rebuild confirming this)
- **Zero warnings in OrchestratorInputBar.swift** (the primary target of this rebuild)
- **Zero warnings in SessionSidebarView.swift**
- Bundle size: **20 MB** (unchanged)

### Observation — 3 new(ish) SettingsView deprecation warnings

```
SettingsView.swift:932:18: warning: 'onChange(of:perform:)' was deprecated in macOS 14.0
SettingsView.swift:948:14: warning: 'onChange(of:perform:)' was deprecated in macOS 14.0
SettingsView.swift:1150:14: warning: 'onChange(of:perform:)' was deprecated in macOS 14.0
```

These come from sub1's macOS 14+ API compatibility fix which downgraded closure signatures to restore buildability but kept using the deprecated `onChange(of:perform:)` API. The new `.onChange(of:value) { old, new in ... }` / `.onChange(of:value) { ... }` signatures are the modern replacements but sub1's fix stayed conservative.

**Functional impact: none.** Deprecated APIs still work on macOS 14+, they just emit warnings. Not a builder-scope issue to fix — flagging for sub1's future tracking.

I probably missed these warnings in the #89 report because my grep filters were different there. They would have been present after sub1's landing too. Not new to #90 per se, but first time I'm counting them explicitly.

### Total warning count: 81

- 75 legacy rust wgpu-cfg dead-code warnings (unchanged, known since #86)
- 3 SettingsView `onChange` deprecation warnings (sub1 fix tradeoff, non-blocking)
- 3 other legacy rust warnings (`NO_WGPU_FONT_SIZE`, `NO_WGPU_LINE_HEIGHT`, field `is_active`, etc.)

## macOS 14+ deployment target concern — resolved

aterm-claude flagged a concern that SessionSidebarView has 30 errors under `swiftc -typecheck -target arm64-apple-macos13.0` (single-file closure). I ignored this per orchestrator guidance and the evidence from Rebuild #89, which built clean with the same file state under `make app`'s full deployment-target closure (macOS 14+).

**Confirmed again in #90:** SessionSidebarView.swift compiles clean under the real make-app build. The 30-error narrow-closure concern is an artifact of sub1/aterm-claude sessions running `swiftc -typecheck` with a different target flag than the real build uses. Not a blocker.

## Launch

```
pkill -f 'build/aterm.app/Contents/MacOS/aterm'     # killed prior sandbox cleanly
> /tmp/aterm-debug-live.log
cd ~/projects/aigentry-aterm && \
  ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data \
  ATERM_TELEPTY_PORT=13848 \
  ./build/aterm.app/Contents/MacOS/aterm > /tmp/aterm-debug-live.log 2>&1 &
```

### Process stats

```
PID=87804 RSS=125.9MB CPU=0.0% ETIME=00:14
```

Healthy boot, idle CPU after init, low RSS. Same shape as #89 baseline.

### Launch log highlights (68 lines)

**Font plumbing preserved:**
```
[FONT-WARN] fallback chain for 'System Default' → userFixedPitchFont (Menlo-Regular)
```

Fourth consecutive rebuild with this observation. v3 Direction E edits to OrchestratorInputBar chrome did not touch the font resolution path.

**Zero NSLayoutConstraint warnings:**
```
grep -iE "NSLayoutConstraint|unable to simultaneously|cannot break|constraint.*will attempt"
(zero matches)
```

The v3 corner-radius (8→16), vertical padding (9→16), horizontal padding (20), and borderless separator removal are pure constant tweaks — the layout formulas themselves are unchanged. No ambiguous or conflicting constraints emerged at startup.

**Zero runtime errors** (excluding expected telepty-bus WebSocket errors and shell-ready fallback timeouts which are harmless).

**Zero runtime log output from the modified components.** This is expected — `OrchestratorInputBar` and `SessionSidebarView` don't carry log instrumentation. The visual result of the v3 changes can only be verified by the user opening the sandbox aterm window and looking at it.

## What this rebuild cannot verify

Per SAWP (builder scope), these require manual GUI verification by the user/tester:

1. **v3 Direction E visual correctness:**
   - 16pt corner radius renders as a softer rounded rectangle
   - 16pt vertical padding produces the taller 48pt single-line bar
   - Separator above the input bar is gone (borderless)
   - Prompt character (`>`) is regular weight at α=0.65
   - Placeholder text is non-italic at α=0.55
   - Focus state shifts lightness by 0.08

2. **Sidebar Settings button:**
   - No divider above it anymore
   - Top sidebar divider still present
   - Clicking anywhere on the Settings cell (not just icon) opens Settings

3. **Korean IME preedit alignment** still renders correctly at the new 16pt vertical padding (no clipping of composition underline).

4. **P0 default workspace + popover focus + all prior fixes** still work in the v3-styled UI.

## Artifacts

- Build log: `/tmp/rebuild-90-build.log`
- Launch log: `/tmp/aterm-debug-live.log`
- App bundle: `/Users/duckyoungkim/projects/aigentry-aterm/build/aterm.app` (20 MB, same size as #89)
- Running PID: 87804

## Concerns / follow-ups

1. **SettingsView 3 deprecation warnings** — sub1 fix retained `onChange(of:perform:)`. Non-blocking but worth migrating to the modern `.onChange(of:value) { _, _ in ... }` or `.onChange(of:value) { ... }` signatures in a follow-up cleanup PR. Sub1 territory, not builder scope.

2. **v3 visual verification gate for 0.2.11 publish** — per task instructions, do NOT publish 0.2.11 until the user visually approves the v3 appearance in the running sandbox (PID 87804). After user OK, rebuild-with-same-build can be used as publish artifact (no code change needed since the running sandbox IS the publish candidate).

3. **Rust wgpu dead-code (75 warnings)** — unchanged from #86. Future refactor.

4. **narrow typecheck divergence** — sub1 + aterm-claude sessions keep producing non-real errors in narrow `swiftc -typecheck -target arm64-apple-macos13.0` closures. Worth a follow-up to align their session invocations with the real build's deployment target. Out of this rebuild's scope.
