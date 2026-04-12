# SPEC: aterm fontFamily plumbing (sub1 picker companion)

**Status:** Draft — awaiting user approval before implementation
**Owner:** aigentry-aterm-claude
**Parallel work:** sub1 implementing picker in SettingsView.swift (do NOT touch)
**SAWP envelope:** Code + swiftc -typecheck + (optional) cargo check only. No app run, no tests.

---

## Goal

When the user picks a font in Settings (sub1's picker), the selected font **takes effect immediately in the running terminal** (hot-reload, stretch goal achieved) — not just on next launch. The picker already writes `appearance.fontFamily` to `~/.aigentry/data/config.json` and calls `onApply()` which invokes `applySettingsToView()`. That existing cascade is the hook to extend.

---

## Audit findings (current state, 2026-04-12)

| # | Location | Current | Notes |
|---|---|---|---|
| 1 | `AppDelegate.swift:1042` | `CTFontCreateWithName("Menlo-Regular" as CFString, ...)` | Hardcoded. Used only for computing cell width/height (metrics feed Rust core via `aterm_core_set_line_height` / `aterm_core_set_cell_width`). |
| 2 | `TerminalView.swift:36` | `GlyphAtlas.makeStyledFont { let base = CTFontCreateWithName("Menlo-Regular" as CFString, size, nil) ... }` | Hardcoded. Called per-glyph from render loop via `rasterize()`. This is the **actual rendering font** — atlas stores bitmaps from this CTFont. |
| 3 | `SettingsView.swift:13,229,277,447,511` | `@Published var fontFamily: String = "System Default"` with Picker, `settings.save()` + `onApply()` on change | Owned by sub1. Writes raw family name strings or `"System Default"` sentinel. |
| 4 | `AppDelegate.swift:1035-1077` | `applySettingsToView(_ view:)` — reads `settings.colorScheme` and `settings.fontSize`, recomputes cell metrics, calls Rust FFI, triggers `aterm_core_resize` | Already wired as the hot-reload pathway. Adding fontFamily means extending this function. |
| 5 | `GlyphAtlas.swift:200` | `func invalidate()` — clears cache, recreates texture, bumps `modified` counter | Needed when font changes so stale glyphs from old font don't render. |
| 6 | `aterm-core/src/lib.rs` | `aterm_core_set_font_size`, `aterm_core_set_line_height`, `aterm_core_set_cell_width` all exist | **No new FFI needed.** Rust core is font-name-agnostic — it only consumes pixel metrics. |
| 7 | `GlyphAtlas.swift:307-334` | `rasterize(codepoint:font:)` has CTFont fallback chain via `CTFontCreateForString` for CJK / missing glyphs | CJK fallback handled automatically per-glyph — user can pick any Latin-only font and 한글 still renders. |

---

## Plumbing cascade (proposed)

```
SettingsView Picker
  ↓ settings.fontFamily = "Monaco"
  ↓ settings.save()    → ~/.aigentry/data/config.json: appearance.fontFamily = "Monaco"
  ↓ onApply()          → AppDelegate.applySettingsToView(view)
  ↓
AppDelegate.applySettingsToView()
  ├─ GlyphAtlas.shared.setFontFamily(settings.fontFamily)   ← NEW (before metric calc)
  │    └─ internal: resolveFontName → invalidate() → modified += 1
  ├─ let fontName = GlyphAtlas.shared.resolvedFontFamily    ← NEW (resolved name)
  ├─ CTFontCreateWithName(fontName as CFString, ...)        ← MODIFIED (was Menlo hardcode)
  ├─ compute cellWidth / cellHeight from new font
  ├─ aterm_core_set_line_height(core, cellHeight)           ← existing FFI
  ├─ aterm_core_set_cell_width(core, cellWidth)             ← existing FFI
  ├─ aterm_core_resize(core, backingW, backingH)            ← existing FFI (grid re-flow)
  └─ aterm_core_render(core)                                 ← existing

TerminalView render loop (next frame)
  └─ atlas.rasterize(cp, fontSize, bold, italic)
       └─ GlyphAtlas.makeStyledFont(size, bold, italic)
            └─ CTFontCreateWithName(GlyphAtlas.shared.resolvedFontFamily as CFString, ...)  ← MODIFIED
       └─ atlas cache miss (invalidated) → new glyph rasterized from new font
```

**Single source of truth:** `GlyphAtlas.shared.resolvedFontFamily` (String). Both the cell-metric path in AppDelegate and the per-glyph rasterize path in TerminalView read from it.

---

## Files to modify

| File | Change |
|---|---|
| `macos/Sources/GlyphAtlas.swift` | Add `private(set) var resolvedFontFamily: String = "Menlo-Regular"`, public `setFontFamily(_:)`, private `static resolveFontName(_:)` with sentinel handling + validation + fallback logging. `setFontFamily` calls `resetState()` under lock only when name actually changes. |
| `macos/Sources/TerminalView.swift` | Line 36: `makeStyledFont` reads `GlyphAtlas.shared.resolvedFontFamily` instead of literal `"Menlo-Regular"`. |
| `macos/Sources/AppDelegate.swift` | Line 1042 area: call `GlyphAtlas.shared.setFontFamily(settings.fontFamily)` **before** `CTFontCreateWithName`; then use the resolved name for `ctFont` creation. |

**NOT touched:** `SettingsView.swift` (sub1 owns), `MetalRenderer.swift` (texture sampling is font-agnostic), `aterm-core/src/**` (Rust core is font-name-agnostic — no FFI changes), `Shaders.metal` (no shader changes).

---

## Sentinel handling — `"System Default"`

`resolveFontName("System Default")` returns `"Menlo-Regular"` — preserves current behavior as the explicit default. Empty string handled the same way.

Rationale for Menlo (not `NSFont.userFixedPitchFont`):
- Current behavior is Menlo — no visual regression
- `userFixedPitchFont` may return non-monospace on some system configs
- Future: can add separate `"SF Mono"` option in sub1's picker for the modern Apple monospace

---

## Invalid font name fallback

```swift
static func resolveFontName(_ requested: String) -> String {
    let trimmed = requested.trimmingCharacters(in: .whitespaces)
    if trimmed.isEmpty || trimmed == "System Default" {
        return "Menlo-Regular"
    }
    // Probe: CTFontCreateWithName returns *some* font always (LastResort if unknown).
    // Validate by comparing PostScript names.
    let probe = CTFontCreateWithName(trimmed as CFString, 12, nil)
    let postScript = (CTFontCopyPostScriptName(probe) as String)
    // Accept if postScript equals requested OR starts with it (handles "Menlo" → "Menlo-Regular")
    if postScript == trimmed
       || postScript.hasPrefix(trimmed)
       || postScript.replacingOccurrences(of: "-", with: "") == trimmed.replacingOccurrences(of: " ", with: "")
    {
        return trimmed
    }
    NSLog("[font] invalid family '%@' (resolved to '%@'), falling back to Menlo-Regular", trimmed, postScript)
    return "Menlo-Regular"
}
```

**Never blocks launch.** Always returns a non-nil usable font name. Logs a warning for the diag trail.

---

## Hot-reload vs restart-required

**Decision: HOT-RELOAD (stretch goal achieved).**

Rationale — the existing wiring already supports it for `fontSize` and `colorScheme`. The cascade from `SettingsView.onChange → settings.save() → onApply() → applySettingsToView → Rust FFI → resize → render` is fully live-swap capable. Adding fontFamily is a one-call extension of that same cascade:

```swift
// In applySettingsToView, BEFORE cell metric calculation:
GlyphAtlas.shared.setFontFamily(settings.fontFamily)
// (internally: if changed, clears cache + increments atlas.modified)
```

`GlyphAtlas.setFontFamily` calls `resetState()` under the existing `NSLock`, which:
1. Clears the `cache: [GlyphKey: AtlasRect]` dictionary
2. Resets shelf allocator state
3. Re-creates the MTLTexture (MetalRenderer observes `modified` counter and re-binds)

On the next render frame, `rasterize()` hits cache-miss for every glyph and re-rasterizes from the new font. Atlas rebuilds progressively, typically within 2-3 frames for a visible screen's worth of glyphs.

No app restart required. No MetalRenderer or shader changes required.

---

## Font metric invalidation cascade

| Step | Component | Action |
|---|---|---|
| 1 | GlyphAtlas | `setFontFamily` → `resetState` → atlas cleared, `modified += 1` |
| 2 | AppDelegate | New CTFont created from resolved family → `cellWidth`, `cellHeight` computed (may differ from old font) |
| 3 | AppDelegate | `aterm_core_set_line_height(newH)` + `aterm_core_set_cell_width(newW)` — existing FFI pushes metrics to Rust core |
| 4 | AppDelegate | `aterm_core_resize(w, h)` — Rust core recomputes grid dimensions (cols × rows), triggers PTY `SIGWINCH`, reflows terminal buffer |
| 5 | AppDelegate | `aterm_core_render(core)` — kicks a redraw |
| 6 | TerminalView (next frame) | `render_data` from Rust (new grid dimensions) → per-cell `atlas.rasterize(cp, fontSize, bold, italic)` cache-miss for each glyph → `makeStyledFont` uses new family → CoreText rasterizes → atlas fills progressively |
| 7 | MetalRenderer | Observes `atlas.modified` change, re-binds new texture on next `drawFrame` |

**Race concern:** between steps 1 and 6, if a render frame is in flight, it could sample the cleared atlas texture. GlyphAtlas's existing NSLock serializes `setFontFamily` with `rect(for:font:)` (the rasterize entry point), so in-flight cache lookups either see old cleared state (cache miss → re-rasterize) or new state. No corruption.

**Stale-metric concern:** if step 3 (FFI push) completes before step 1 (atlas clear), one frame could render with new Rust metrics but old atlas glyphs — visible as misaligned characters for 1-2 frames. **Mitigation: call `setFontFamily` FIRST in `applySettingsToView`, before `CTFontCreateWithName`.** The ordering in the spec above enforces this.

---

## Verification plan

1. **`swiftc -typecheck`** — must pass (exit 0) on all 3 edited Swift files + dependencies.
2. **Self-diff review** — confirm no changes to:
   - `settings.fontSize` handling / `round()` drift fix (#157)
   - `[DIAG-LOAD]` `configPath` logging
   - sRGB color pipeline (`applySchemeBackground`, default color FFI)
   - NFC normalization path (#166)
   - IME marked-text layout (just fixed in #240 — TerminalView's own NSTextInputClient)
3. **(Optional) `cargo check`** — only if Rust FFI changes were needed. Per audit, **no Rust changes needed**, so this step is N/A. Will run it anyway as a smoke test.
4. **Tester handles the runtime verification:**
   - Change `fontFamily` in Settings (Menlo → Monaco → SF Mono → invalid name → back to System Default)
   - Verify each change applies immediately (visible on screen within ~1 second)
   - Verify "System Default" reverts to Menlo
   - Verify invalid name logs `[font] invalid family...` and falls back to Menlo
   - Verify no atlas corruption (no garbled glyphs, no missing characters)
   - Verify 한글/CJK still renders via CoreText fallback chain
   - Verify IME composition (ㅎ→하→한) after font swap — preedit underline aligned
   - Verify cell grid re-flows on cell-width change (no overflow, no gap)

---

## Risks

| # | Risk | Mitigation |
|---|---|---|
| 1 | **Stale atlas after font change** — cached glyphs from old font render alongside new glyphs | `setFontFamily` calls `resetState()` under lock → full cache clear before new rasterize |
| 2 | **Cell size mismatch Swift ↔ Rust** — Rust thinks grid is 80×24 but Swift rasterizes for a wider font → misaligned columns | Enforce ordering: `setFontFamily` → metric recalc → FFI push → `aterm_core_resize` → render. Swift metric computation must use the SAME font name as GlyphAtlas. Both read from `GlyphAtlas.shared.resolvedFontFamily` (single source of truth). |
| 3 | **Metal pipeline cached glyphs** — MetalRenderer may cache texture binding across frames | MetalRenderer already observes `GlyphAtlas.modified` counter and re-binds when it changes. `resetState()` bumps `modified`. Already wired. |
| 4 | **IME preedit re-layout after font swap** — 한글 composition may flicker or misalign during switch | TerminalView's `firstRectForCharacterRange` uses `currentFontSize` + cell metrics, both refreshed in same `applySettingsToView` call. Single-frame flicker acceptable (same as fontSize changes today). |
| 5 | **fontSize drift interaction (#157)** — fontSize round() fix still applies | Unrelated code path. Not touched. `settings.fontSize` read is identical to current. |
| 6 | **Bold/italic variant missing** — some fonts don't have bold/italic | `CTFontCreateCopyWithSymbolicTraits` already has `?? base` fallback. Preserved. |
| 7 | **CJK font fallback chain breaks** — non-CJK primary font should still render 한글 via `CTFontCreateForString` | The fallback chain in `GlyphAtlas.rasterize(codepoint:font:)` (lines 317-334) is **per-glyph** and **font-name-agnostic** — it takes the current primary font and asks CoreText for a CJK substitute. Works regardless of primary font. Preserved. |
| 8 | **.systemFont vs CTFont name divergence** — `NSFont.monospacedSystemFont` is SF Mono but its name is not `"SF Mono"` in PostScript terms (`"SFMono-Regular"`) | `resolveFontName` probe handles this via postScript prefix match. If sub1's picker offers `"SF Mono"` as a display label mapped to `"SFMono-Regular"` PostScript, that's sub1's mapping responsibility. Ours is to accept what we receive and validate. |
| 9 | **Ligature rendering for programming fonts** (Fira Code, JetBrains Mono) | Not in scope. CTFont default substitution applies. Not handled — ligatures require GPOS shaping which our atlas doesn't support. |

---

## Open questions (for user approval)

1. **Hot-reload vs restart-required:** I'm recommending HOT-RELOAD. Existing wiring supports it at no additional complexity. Agreed?
2. **Fallback font:** falling back to `"Menlo-Regular"` hardcoded in `resolveFontName`. Is this acceptable, or should the fallback be configurable via another config key?
3. **CJK handling:** keeping the existing per-glyph `CTFontCreateForString` fallback chain (transparent to user). Any concerns?
4. **Font validation scope:** I probe only by PostScript name match. Should I also verify the font is actually monospaced (reject proportional fonts)? Proportional fonts will render misaligned in a terminal — validation could prevent user foot-gun. Tradeoff: `CTFontCopyAttribute(font, kCTFontFixedAdvanceAttribute)` check — 1 extra line, saves users from confusion.
5. **Bold/italic substitution policy:** current code falls back to regular if bold/italic variant missing. Should we instead emit a visible warning (log only, or also a status indicator)? Currently silent fallback.

---

## Out of scope for v1

- Font ligature rendering (GPOS shaping)
- Font feature flags (stylistic alternates, OpenType features)
- Web font loading / custom font URLs
- Per-workspace font override (global only for now)
- Live font-size-adaptive line height (uses the computed `ascent + descent + leading` once, not re-computed on DPI change)

---

## NO CODE CHANGES MADE. Awaiting `[IMPLEMENT APPROVED]` from orchestrator.
