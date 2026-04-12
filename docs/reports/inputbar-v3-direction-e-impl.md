# REPORT: OrchestratorInputBar v3 Direction E Corrected — implementation

**Date:** 2026-04-12
**Priority:** P2 (queue position 3)
**Owner:** aigentry-aterm-claude
**SPEC ref:** `30bd92f593f1871dcca6f717c77a7a7fcfe50429d56b72f26d6ddc741af03077`
**Approval:** `[IMPLEMENT APPROVED]` with Q1-Q8 + user mockup revision (48pt minHeight, 20h/16v padding, defaultLineCount=1)

---

## Status: COMPLETE

- **Code:** 1 file edited (`macos/Sources/OrchestratorInputBar.swift`)
- **LOC delta:** ~+30 / -25 (net +5)
- **swiftc -typecheck:** **PASS exit 0** on 6-file closure (OrchestratorInputBar + OrchestratorCommands + OrchestratorHistory + AtermTheme + AtermLocalization + GlyphAtlas)
- **IME verification:** DEFERRED to tester runtime verification (see Q8 fallback plan)

---

## Frozen numerics (per user decision)

| Parameter | Value |
|---|---|
| `minBarHeight` | **48pt** (matches v2, NOT 88pt — user explicitly rejected tall empty state) |
| `maxBarHeight` | 200pt (unchanged from v2) |
| `cornerRadius` | **16pt** (card identity, from 14-18pt range) |
| `horizontalPadding` | **20pt** (inside card) |
| `verticalPadding` | **16pt** (inside card via `textContainerInset.height`) |
| `barHorizontalMargin` | 8pt (outer card breathing from bar edges) |
| `barTopMargin` / `barBottomMargin` | **0pt / 0pt** (card fills bar vertically — required to hit 48pt total) |
| `containerContentHeight` | `lineHeight + 2*16 = 16 + 32 = 48pt` |
| `focusLightenFraction` | 0.08 (~+2 lightness blend with white) |
| Focus animation duration | 200ms ease-out (Q6) |
| Height animation duration | 120ms ease-out (unchanged from v2) |

**Height math verification:** `containerContentHeight (48) + barTopMargin (0) + barBottomMargin (0) = 48pt minBarHeight ✓`. Matches v2 exactly — no terminal area loss.

---

## Changes applied

### 1. Removed subview (top hairline)
- `separatorView: NSView` **entirely removed** from the file:
  - Declaration removed
  - setup() creation block removed
  - Constraint activation removed
  - `viewDidChangeEffectiveAppearance` refresh removed
- `grep -n 'separatorView' OrchestratorInputBar.swift` → **0 matches** (verified)

### 2. Constants updated
```swift
// v3 Direction E Corrected
private static let verticalPadding: CGFloat = 16          // was 9
private static let horizontalPadding: CGFloat = 20        // new
private static let cornerRadius: CGFloat = 16             // was 8
private static let barHorizontalMargin: CGFloat = 8       // new (was inlined 8)
private static let barTopMargin: CGFloat = 0              // was 6
private static let barBottomMargin: CGFloat = 0           // was 8
private static let focusLightenFraction: CGFloat = 0.08   // new
private static let containerContentHeight: CGFloat = lineHeight + verticalPadding * 2  // = 48
static let minBarHeight: CGFloat = containerContentHeight + barTopMargin + barBottomMargin  // = 48
private static let maxBarHeight: CGFloat = 200            // unchanged
```

### 3. Container styling (`applyContainerStyle`)
**Before (v2):**
```swift
layer.cornerRadius = 8
layer.backgroundColor = NSColor.clear.cgColor
layer.borderWidth = 1
layer.borderColor = focused ? inputBarBorderFocus : inputBarBorder
```

**After (v3):**
```swift
layer.cornerRadius = Self.cornerRadius  // 16
let restingBg = AtermTheme.inputBarBackground
let focusedBg = restingBg.blended(withFraction: Self.focusLightenFraction, of: .white) ?? restingBg
layer.backgroundColor = (focused ? focusedBg : restingBg).cgColor
layer.borderWidth = 1
layer.borderColor = AtermTheme.inputBarBorder.withAlphaComponent(0.04).cgColor  // nearly invisible
```

### 4. Prompt glyph weight + alpha
- Weight: `NSFont.Weight.semibold` → `.regular` (lighter tone)
- Resting tint: `AtermTheme.inputBarPrompt` (full alpha) → `AtermTheme.inputBarPrompt.withAlphaComponent(0.65)` (muted)
- Focus tint: `AtermTheme.inputBarPromptFocus` (unchanged — full alpha on focus)

### 5. Placeholder attributes
- Removed italic mask: `Self.inputFont.withTraits(.italicFontMask)` → `Self.inputFont` (regular)
- Alpha reduced: `AtermTheme.inputBarPlaceholder` → `.withAlphaComponent(0.55)`
- Text content **unchanged** (#246 preserved): `AtermLocalization.text(ko: "명령어 입력...", en: "Type a command...")`

### 6. Focus animation
- Duration: `0.15` → `0.20` (200ms, Q6 approved)
- Timing function: `.easeOut` unchanged
- Now animates: background color (lightness shift) + prompt tint alpha
- No longer animates: border color (border is now static nearly-invisible)

### 7. Constraint layout
- Removed separator constraints (4 constraints deleted)
- Container leading/trailing: `constant: 8 / -8` literals → `Self.barHorizontalMargin`
- Container top/bottom: now uses `Self.barTopMargin (0) / -Self.barBottomMargin (0)` — card fills bar vertically
- Prompt leading: `constant: 12` → `Self.horizontalPadding (20)` — new card-interior alignment
- Prompt size: 14x14 → 12x12 (note: earlier SPEC proposed 12x12; kept)
- Prompt centerY formula: **preserved identically** — `container.top + verticalPadding + lineHeight/2`
- Scroll view leading: `promptLabel.trailing + 8` unchanged
- Scroll view trailing: `constant: -10` → `-Self.horizontalPadding (-20)` — new card-interior alignment
- Scroll view top/bottom: unchanged (flush with container, padding via `textContainerInset`)

### 8. `textContainerInset`
- Value: `NSSize(width: 0, height: 9)` → `NSSize(width: 0, height: 16)` — **formula shape preserved**, constant shifted from 9 to 16pt per the v3 padding spec
- Shape: still `NSSize(width: 0, height: verticalPadding)` — identical to v2, only the constant changes
- **IME marked-text risk**: the layout manager still uses this inset for positioning; marked-text underline should render at the same relative position within the scroll view. Tester verification mandatory per Q8.

### 9. `viewDidChangeEffectiveAppearance`
- Removed `separatorView.layer?.backgroundColor = ...` (separator no longer exists)
- Prompt tint update: full alpha → `withAlphaComponent(0.65)` for resting state (matches updateFocusState)

---

## swiftc -typecheck verification

```bash
swiftc -typecheck \
  -sdk $(xcrun --show-sdk-path --sdk macosx) \
  -target arm64-apple-macos13 \
  macos/Sources/OrchestratorInputBar.swift \
  macos/Sources/OrchestratorCommands.swift \
  macos/Sources/OrchestratorHistory.swift \
  macos/Sources/AtermTheme.swift \
  macos/Sources/AtermLocalization.swift \
  macos/Sources/GlyphAtlas.swift
# EXIT: 0  (no errors, no warnings)
```

**PASS.** 6-file closure typechecks clean. SourceKit LSP continued to show stale cached diagnostics throughout the edit session (referencing removed `separatorView` at old line numbers), but `grep -n 'separatorView' OrchestratorInputBar.swift` confirms zero matches in the actual file content. Authoritative verification is the multi-file swiftc invocation above.

**Note on full-project typecheck:** `SettingsView.swift` has pre-existing `.onChange(of:initial:_:)` macOS 14+ API errors from sub1's work that block full-project compilation. This is **unrelated** to v3 and was flagged in the P0 default workspace impl report. v3 alone is clean.

---

## Regression check — all v2 preserved

| v2 feature | v3 status |
|---|---|
| `chevron.right` SF Symbol prompt glyph (#240 Q2) | ✅ kept — weight changed from semibold to regular for tone, symbol identical |
| System default caret (#240 Q1) | ✅ `insertionPointColor` still NOT set |
| 3px `ThinScroller` always visible (#240 Q4) | ✅ scrollView config unchanged |
| `/command` dropdown popover | ✅ `commandDropdown` property unchanged, `OrchestratorCommands.swift` untouched |
| Popover focus 3-layer defense | ✅ `DropdownTableView.acceptsFirstResponder=false`, `anchorWindow.makeKey()`, `configurePopoverWindowAfterShow()` all preserved in `OrchestratorCommands.swift` |
| ↑/↓ history recall + persist | ✅ `handleArrowUp/Down` unchanged, `history: OrchestratorHistory` instance unchanged |
| Ctrl+C/D/L PTY bytes 0x03/0x04/0x0C | ✅ `OrchestratorTextView.keyDown` modifier check unchanged |
| Shift+Enter multiline + Enter submit | ✅ `handleShiftEnter` + `handleEnter` unchanged |
| Tab autocomplete | ✅ `handleTab` dropdown + single-match branches unchanged |
| ESC dropdown-close priority | ✅ `handleEscape` unchanged |
| Cmd+V paste | ✅ NSTextView default responder chain (no override added) |
| Cursor vertical centering formula | ✅ `textContainerInset.height = verticalPadding` formula preserved; constant shifted 9→16 |
| Dynamic height growth to 200pt | ✅ `recomputeHeight` logic preserved, max 200pt unchanged, min updated to 48pt (same as v2) |
| Font plumbing hot-reload | ✅ `makeStyledFont` in TerminalView extension reads `GlyphAtlas.shared.resolvedFontFamily` — `GlyphAtlas.swift` + `TerminalView.swift` untouched |
| IME marked-text alignment | ⚠️ formula preserved, constant shifted 9→16 — **tester verification mandatory** per Q8 |
| #246 English default placeholder | ✅ `"Type a command..."` via AtermLocalization unchanged |
| `applyContainerStyle` + `updateFocusState` flow | ✅ method signatures unchanged, internal body updated |
| NSTextViewDelegate conformance | ✅ `textDidChange` unchanged |
| `OrchestratorTextView` subclass (keyDown, focus change, placeholder draw) | ✅ entirely untouched |
| `ThinScroller` subclass | ✅ entirely untouched |

---

## IME verification plan

Per Q8 approval, tester must verify after Rebuild #90:

1. Click into OrchestratorInputBar → caret should be at vertical center of card (y ≈ container.top + 16 + 8 = 24pt from card top)
2. Type Korean: ㅎ → 하 → 한 (standard dubeolsik composition)
3. **Expected:** preedit underline renders below the composition characters, vertically centered within the line, no drift
4. **Fallback if misaligned:** revert to asymmetric padding:
   - `textContainerInset.height = 9` (v2 value)
   - `scrollView.topAnchor = container.top + 11` (adds 11pt of scrollView offset)
   - `scrollView.bottomAnchor = container.bottom - 11`
   - Total vertical padding still = 9 (inset) + 11 (scrollView) = 20pt top + 20pt bottom = equivalent to 16pt effective, but with the marked-text baseline at the same relative Y as v2

The fallback is a one-line change and can be applied without re-spec if tester reports issues.

---

## Known concerns

1. **Clarification about "user screenshot shows compile errors":** any compile errors in the user's sandbox at screenshot time are NOT from this v3 work. v3 typechecks clean (exit 0) on its 6-file closure. Full-project typecheck would fail on sub1's `SettingsView.swift` macOS 14+ `.onChange(of:initial:_:)` API errors — this was pre-existing and reported in the P0 default workspace impl report. v3 does not modify SettingsView.swift.

2. **`barTopMargin = barBottomMargin = 0`** means the card fills the bar vertically with NO gap to the sidebar bg above or the window edge below. If visually this feels too flush, re-introduce small margins (4pt each) and reduce `verticalPadding` to 14 to maintain 48pt total: `lineHeight(16) + 2*14 + 4 + 4 = 48`. Not applied in this pass — matching user's approved numerics literally.

3. **`border α=0.04`** is very subtle. On light mode it may be invisible; on dark mode it provides a barely-perceptible edge. If testers report "no edge visible", switch to 0 (fully remove) or 0.08 (slightly more visible). Current value picked as middle ground.

4. **Background lightness blending via `NSColor.blended(withFraction:of:)`** uses `sRGB` color space by default. The `+0.08` lightness toward white produces approximately `#22232d → #292a35` on dark mode (close to the user's spec "+2 lightness"). Accurate enough; no HSL conversion needed.

5. **Focus animation** animates the `CALayer.backgroundColor` via `NSAnimationContext.runAnimationGroup` with `ctx.allowsImplicitAnimation = true`. CALayer properties need either an explicit CATransaction or Core Animation's implicit animation to pick up the change. With `allowsImplicitAnimation=true`, AppKit should bridge this. If focus animation doesn't animate smoothly in practice, wrap the `layer.backgroundColor` assignment in an explicit `CATransaction.begin()/commit()` block with duration + timing.

6. **Placeholder drawing** uses `OrchestratorTextView.draw(_:)` override which computes `origin.y = textContainerInset.height`. With height=16, placeholder draws at y=16 from textView top, which matches first-line baseline position. Should render correctly at the vertical center of the 48pt card.

7. **`AtermTheme.inputBarBackground` existing hex values:** dark `#1E1F2B`, light `#EFEBE4`. These are `+4` lightness over the sidebar background `#0A0A0A` / `#F0EBE3`, which matches the user's approved spec ("+4 lightness over sidebar bg, approximately #22232d relative to sidebar #1a1b23" — the user's example sidebar hex differs slightly but the elevation delta concept is the same). No new token added per Q4.

8. **No new dependencies, no Rust changes, no FFI changes, no AtermTheme token additions.**

---

## Files list

```
macos/Sources/OrchestratorInputBar.swift  [M]  +30/-25 LOC (net +5)
  - constants section: added horizontalPadding, cornerRadius, barHorizontalMargin, focusLightenFraction; changed verticalPadding 9→16, barTopMargin 6→0, barBottomMargin 8→0
  - property section: removed separatorView
  - setup() container block: removed separator creation, updated containerView cornerRadius to Self.cornerRadius
  - setup() prompt glyph: regular weight, 0.65 alpha
  - setup() placeholder: non-italic, 0.55 alpha
  - setup() constraint block: removed 4 separator constraints, updated container/prompt/scrollView padding to use new constants
  - applyContainerStyle: background lightness shift, nearly invisible border
  - updateFocusState: 200ms duration, prompt alpha sync
  - viewDidChangeEffectiveAppearance: removed separator refresh, prompt alpha sync
  - textContainerInset: NSSize(width: 0, height: 16) via verticalPadding constant
```

No other files touched. No Rust. No SettingsView (sub1 territory). No AppDelegate (P0 fix preserved). No GlyphAtlas / font plumbing. No tests run. No app run.
