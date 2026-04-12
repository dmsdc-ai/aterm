# Rebuild #93 — global cursor CJK fix verification

**Date:** 2026-04-12 12:41 KST
**Builder:** aigentry-builder-claude
**Outcome:** PASS — cellH increased 27→28 confirming CJK max() probe is effective
**Sandbox only:** ATERM_DATA_ROOT=~/projects/aigentry-sandbox/data, ATERM_TELEPTY_PORT=13848

## Build

```
EXIT=0, zero errors, zero warnings in AppDelegate.swift
```

## CURSOR-DIAG — #92 vs #93 comparison

| Metric | #92 (before) | #93 (after) | Delta |
|---|---|---|---|
| `set_line_height requested` | 27 | **28** | **+1** |
| `set_line_height clamped` | 27 | **28** | **+1** |
| `cellH` | 27.0 | **28.0** | **+1.0** |
| `cellW` | 14.0 | 14.0 | — |
| `padY` | 4.0 | **6.0** | **+2.0** |
| `padX` | 4.0 | 4.0 | — |
| `cursor y` | 4.0 | **6.0** | **+2.0** |
| `fontSize` | 23.0 | 23.0 | — |
| `cursor style` | block | block | — |
| primary font | Menlo-Regular | Menlo-Regular | — |
| primary total | 26.8 | 26.8 | — |
| CJK fallback font | STIXTwoMath-Regular | STIXTwoMath-Regular | — |
| CJK total | 28.8 | 28.8 | — |
| InputBar lineHeight | 16.0 | 16.0 | — |
| InputBar verticalPadding | 16.0 | 16.0 | — |
| InputBar containerContent | 48.0 | 48.0 | — |

### Verbatim #93 CURSOR-DIAG output

```
[CURSOR-DIAG] InputBar: lineHeight=16.0 verticalPadding=16.0 containerContent=48.0 minBar=48.0 textContainerInset.h=16.0
[CURSOR-DIAG] set_line_height: requested=28 clamped=28 default_const=21
[CURSOR-DIAG] TerminalView cursor: x=4.0 y=6.0 cellW=14.0 cellH=28.0 padX=4.0 padY=6.0 fontSize=23.0 style=block backing=2080x1440
[CURSOR-DIAG] primary font=Menlo-Regular ascent=21.3 descent=5.4 leading=0.0 total=26.8
[CURSOR-DIAG] CJK fallback font=STIXTwoMath-Regular ascent=17.5 descent=5.5 leading=5.8 total=28.8 cp=0x23FA char='⏺'
```

## Analysis

1. **CJK max() probe is working:** cellH increased from 27→28. The AppDelegate now computes `max(primary_total, CJK_total)` = `max(26.8, 28.8)` = 28.8, which truncates to integer 28 when passed to Rust as the line height.

2. **Remaining gap:** CJK total (28.8) still exceeds cellH (28.0) by 0.8px — down from the previous 1.8px gap (28.8−27.0). The truncation from 28.8→28 loses that last sub-pixel. Using `ceil()` instead of `Int()` truncation would yield cellH=29, fully enclosing CJK metrics with 0.2px headroom.

3. **padY adjusted automatically:** padY increased 4→6, and cursor y position shifted 4→6 accordingly. The taller cell height redistributes the remaining viewport space differently, giving more vertical padding. This means the cursor is positioned further from the top edge of the terminal viewport, which is a side effect of the larger cellH.

4. **InputBar unaffected:** all InputBar metrics unchanged — Fix A (CJK lineHeight) from #92 is independent of the terminal cellH fix in #93.

5. **Font metrics unchanged:** primary and CJK fallback metrics are stable between rebuilds (deterministic font probing).

## Process stats

```
PID=60148 RSS=125.7MB CPU=0.1% (settled to 0.0% at 38s)
```

FONT-WARN chain fires (7th consecutive rebuild). Zero errors.

## Artifacts

- Build log: `/tmp/rebuild-93-build.log`
- Launch log: `/tmp/aterm-rebuild-93.log`
- CURSOR-DIAG extracted: `/tmp/cursor-diag-93.txt` (5 lines)
- #92 comparison: `/tmp/cursor-diag-92.txt`
- Running PID: 60148
