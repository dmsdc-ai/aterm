# Rebuild #92 — cursor diagnostic logs + OrchestratorInputBar CJK centering fix

**Date:** 2026-04-12 12:35 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS (build + launch + 5/5 CURSOR-DIAG points captured)
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Scope

aterm-claude Phase 1 complete (REPORT ref 5e2b158893b1):
- 5 CURSOR-DIAG log points added across Rust + Swift
- OrchestratorInputBar CJK-aware lineHeight fix (Fix A) included

## Build

```
cd ~/projects/aigentry-aterm && \
  touch macos/Sources/OrchestratorInputBar.swift \
        macos/Sources/TerminalView.swift \
        macos/Sources/GlyphAtlas.swift && \
  make app > /tmp/rebuild-92-build.log 2>&1
EXIT=0
```

Zero errors, zero warnings in the modified files. Bundle size 20 MB (unchanged).

## CURSOR-DIAG Log Capture — VERBATIM

All 5 diagnostic points fired. Captured from combined stdout+stderr via `./build/aterm.app/Contents/MacOS/aterm > /tmp/aterm-rebuild-92.log 2>&1`.

### Line 1: OrchestratorInputBar (Swift NSLog)
```
2026-04-12 12:35:27.112 aterm[41422:81487900]
[CURSOR-DIAG] InputBar: lineHeight=16.0 verticalPadding=16.0 containerContent=48.0 minBar=48.0 textContainerInset.h=16.0
```

### Line 2: lib.rs set_line_height (Rust eprintln!)
```
[CURSOR-DIAG] set_line_height: requested=27 clamped=27 default_const=21
```

### Line 3: TerminalView cursor metrics (Swift NSLog)
```
2026-04-12 12:35:31.076 aterm[41422:81488082]
[CURSOR-DIAG] TerminalView cursor: x=4.0 y=4.0 cellW=14.0 cellH=27.0 padX=4.0 padY=4.0 fontSize=23.0 style=block backing=2080x1440
```

### Line 4: GlyphAtlas primary font metrics (Swift NSLog)
```
2026-04-12 12:35:31.670 aterm[41422:81488082]
[CURSOR-DIAG] primary font=Menlo-Regular ascent=21.3 descent=5.4 leading=0.0 total=26.8
```

### Line 5: GlyphAtlas CJK fallback font metrics (Swift NSLog)
```
2026-04-12 12:35:31.670 aterm[41422:81488082]
[CURSOR-DIAG] CJK fallback font=STIXTwoMath-Regular ascent=17.5 descent=5.5 leading=5.8 total=28.8 cp=0x23FA char='⏺'
```

## Key Diagnostic Values Summary

| Parameter | Source | Value |
|---|---|---|
| **Terminal line height** | lib.rs set_line_height | requested=27, clamped=27, default_const=21 |
| **Terminal cell size** | TerminalView | cellW=14.0, cellH=27.0 |
| **Terminal padding** | TerminalView | padX=4.0, padY=4.0 |
| **Terminal font size** | TerminalView | fontSize=23.0 |
| **Terminal cursor style** | TerminalView | block |
| **Terminal backing size** | TerminalView | 2080x1440 |
| **Primary font** | GlyphAtlas | Menlo-Regular |
| **Primary ascent** | GlyphAtlas | 21.3 |
| **Primary descent** | GlyphAtlas | 5.4 |
| **Primary leading** | GlyphAtlas | 0.0 |
| **Primary total** | GlyphAtlas | 26.8 |
| **CJK fallback font** | GlyphAtlas | STIXTwoMath-Regular |
| **CJK ascent** | GlyphAtlas | 17.5 |
| **CJK descent** | GlyphAtlas | 5.5 |
| **CJK leading** | GlyphAtlas | 5.8 |
| **CJK total** | GlyphAtlas | 28.8 |
| **CJK test char** | GlyphAtlas | U+23FA '⏺' |
| **InputBar lineHeight** | OrchestratorInputBar | 16.0 |
| **InputBar verticalPadding** | OrchestratorInputBar | 16.0 |
| **InputBar containerContent** | OrchestratorInputBar | 48.0 |
| **InputBar minBarHeight** | OrchestratorInputBar | 48.0 |
| **InputBar textContainerInset.h** | OrchestratorInputBar | 16.0 |

## Analysis for Phase 3

Key observations from the diagnostic data:

1. **Terminal cellH (27.0) matches set_line_height clamped (27)** — the line height pipeline is consistent from Rust to Swift.

2. **Primary font total (26.8) < cellH (27.0)** — 0.2px gap. The cursor block should nearly fill the cell but 26.8/27.0 means there's almost zero headroom.

3. **CJK fallback total (28.8) > cellH (27.0)** — CJK glyph metrics exceed the cell height by 1.8px. This could cause CJK glyphs to clip or the cursor block to be shorter than the glyph, which is the root of the "cursor doesn't cover full CJK glyph height" issue.

4. **CJK fallback is STIXTwoMath-Regular** (not a CJK font like PingFang or Noto CJK) — the test char is U+23FA '⏺' (a symbol, not a CJK ideograph). The real CJK fallback font for Korean/Chinese characters would likely be AppleSDGothicNeo or PingFang. The diagnostic only fires once for the first CJK-fallback lookup.

5. **InputBar lineHeight (16.0)** — this is the Fix A CJK-aware value. The container content height is 48.0 = lineHeight (16.0) + 2×verticalPadding (2×16.0). The textContainerInset.height of 16.0 centers the text within the 48pt bar.

6. **fontSize (23.0) in TerminalView vs config fontSize=20.0** — there may be a DPI scaling factor applied (23.0 / 20.0 = 1.15×, or rounding from a non-integer DPI adjustment). This discrepancy is worth noting for Phase 3.

## Process stats

```
PID=41422 RSS=125.3MB CPU=0.0% ETIME=00:38
```

FONT-WARN fallback chain fires (6th consecutive rebuild). Zero errors.

## Visual check limitations

Builder cannot perform interactive GUI verification (typing Korean in InputBar, inspecting cursor visual centering). User/tester must:

1. Click orchestrator row → InputBar appears at bottom
2. Type Korean text (e.g., 안녕하세요) → cursor should be vertically centered (Fix A)
3. Look at terminal cursor → should still be top-aligned (Fix B not yet applied)
4. Compare terminal cursor height to cell height — the 27px cellH vs 26.8px primary font total means cursor should fill most but not all of the cell

## Artifacts

- Build log: `/tmp/rebuild-92-build.log`
- Launch log: `/tmp/aterm-rebuild-92.log`
- CURSOR-DIAG extracted: `/tmp/cursor-diag-92.txt` (5 lines)
- Running PID: 41422
