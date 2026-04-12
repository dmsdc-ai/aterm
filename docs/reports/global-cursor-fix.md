# REPORT: Global cursor top-alignment fix

**Date:** 2026-04-12
**Priority:** P0 (blocks 0.2.11)

---

## Fix A — OrchestratorInputBar: COMPLETE

**Root cause:** `lineHeight = ceil(NSLayoutManager().defaultLineHeight(for: monospacedSystemFont(13)))` returns Latin-only metrics (~14-16pt). Korean CJK glyphs use font fallback (Hiragino Sans) whose actual layout height is ~20-22pt. With `textContainerInset.height = verticalPadding = 16pt` and `containerContentHeight = lineHeight + 32 = 48pt`, the text area = 48-32 = 16pt cannot fit a 22pt CJK line → top-aligned overflow.

**Fix:** CJK-aware `lineHeight` measurement via dummy `NSTextStorage("한글")` + `NSLayoutManager.usedRect`. Takes `max(latin, cjk)`. Bar grows from 48 to ~54pt. Added `NSMutableParagraphStyle.min/maxLineHeight` forcing to `textView.typingAttributes` so Latin and CJK lines render at the same height, centering identically.

**swiftc -typecheck:** PASS exit 0

---

## Fix B — TerminalView cell cursors: SCOPE EXTENSION NEEDED

**Investigation findings** (read-only audit, no code changes):

The terminal cell cursor is positioned at `(padX + col*cellW, padY + row*cellH)` in Rust (`aterm-core/src/lib.rs:1200-1203`). Glyphs are positioned in the Metal shader (`Shaders.metal:274+312-313`) at `cell_origin.y + cell_size.y - bearing_y/scale` where `bearing_y` is computed from CoreText glyph bounds in `GlyphAtlas.swift:376`.

The cursor covers the FULL cell from `cell_top` to `cell_bottom`. The glyph is anchored from `cell_bottom - bearing_y/scale`, which places the baseline near `cell_bottom`. If `cellH` (from `CTFontGetAscent + CTFontGetDescent + CTFontGetLeading`) includes generous leading, the glyph occupies the UPPER portion of the cell with empty space at the bottom — creating a "top-aligned text" appearance within the cursor block.

**Why this is beyond current scope:** fixing requires coordinating changes across:
1. `aterm-core/src/lib.rs` — cell metric FFI (`aterm_core_cursor_position`, `no_wgpu_cell_height`)
2. `macos/Sources/Shaders.metal` — `cell_text_vertex` bearing_y formula
3. `macos/Sources/GlyphAtlas.swift` — `bearingY` computation with Retina scale
4. `macos/Sources/AppDelegate.swift` — `CTFontGetAscent/Descent/Leading` → `cellHeight` conversion
5. Rust cell grid reflow implications if cellH changes

**This is a Rust + Swift + Metal cross-cutting issue, not a pure Swift fix.**

**Recommendation:** orchestrator delegates Fix B to an analyst/architect for the Retina-scale glyph pipeline audit, with benchmark comparisons against ghostty/alacritty/kitty cell metric formulas. It may be a pre-existing visual characteristic (cellH includes leading = intentional line spacing) rather than a bug.
