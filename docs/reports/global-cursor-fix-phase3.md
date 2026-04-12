# REPORT: Global cursor fix Phase 3 — CJK-aware cellH in applySettingsToView

**Date:** 2026-04-12
**Priority:** P0

## Fix B: COMPLETE (Approach 2 — Swift-side CJK max, no Rust API change)

**Root cause confirmed by Phase 2 evidence:**
- Primary font (Menlo-Regular): ascent=21.3 descent=5.4 leading=0.0 total=26.8 → cellH=27
- CJK fallback (STIXTwoMath-Regular): total=28.8 — exceeds cellH by 1.8px
- Korean fallback (AppleSDGothicNeo/PingFang): expected total ~28-30 — exceeds cellH by 1-3px
- Glyphs overflow cell bottom → cursor anchored at cell TOP appears top-aligned

**Fix:** `AppDelegate.swift:applySettingsToView` now measures CJK fallback font metrics via `CTFontCreateForString(primaryFont, "한"/"漢")` and takes `max(primaryTotal, cjkMaxTotal)`. The resulting `cellHeight` value passed to `aterm_core_set_line_height` is the MAX across primary + CJK fallback — cells always fit the tallest glyph.

**Expected cellH change:** 27 → ~29 (if CJK total ≈ 28.8, rounded = 29). Terminal may show 1 fewer row on screen. Acceptable.

**cargo check:** PASS exit 0
**swiftc -typecheck:** PASS exit 0

## Files modified

| File | Change |
|---|---|
| `macos/Sources/AppDelegate.swift` | `applySettingsToView`: replaced `round(ascent+descent+leading)` with `round(max(primaryTotal, cjkMaxTotal))` via CTFontCreateForString CJK probe loop ("한", "漢") |

No Rust API change (Approach 2). No shader change. No TerminalView change (cursor uses cellH from core which now has the correct max value).

## Fix A status (OrchestratorInputBar)
Already included in Phase 1 — CJK-aware `lineHeight` via dummy NSTextStorage("한글") + NSLayoutManager.usedRect + NSParagraphStyle min/max line height forcing. Bar grows from 48 → ~54pt to accommodate CJK.

## Diagnostic logs
[CURSOR-DIAG] logs in 5 files remain for now. Remove after visual verification on Rebuild #93.
