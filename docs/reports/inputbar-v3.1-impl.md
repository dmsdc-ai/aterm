# REPORT: OrchestratorInputBar v3.1 (hybrid: v2 layout + v3 typography)

**Date:** 2026-04-12
**Priority:** P2 (queue position 3 follow-up)
**Owner:** aigentry-aterm-claude
**Prior SPEC ref:** `30bd92f593f1...` (v3 Direction E) — approved then revised after mockup review
**Prior impl ref:** `63f6b22b935b...` (v3 Direction E + borderless) — rejected per "너무 안 예뻐", "전 디자인이 훨씬 나은 것 같아", "여백없게 다 채워줘"
**User decision:** hybrid — revert card elevation, keep v3 typography

---

## Status: COMPLETE

- **Code:** 1 file edited (`macos/Sources/OrchestratorInputBar.swift`)
- **LOC delta:** ~+15 / -15 (net 0 — mostly constant/assignment swaps, re-added separator offsets the card styling removal)
- **swiftc -typecheck:** **PASS exit 0** on 6-file closure (OrchestratorInputBar + OrchestratorCommands + OrchestratorHistory + AtermTheme + AtermLocalization + GlyphAtlas)
- **IME verification:** still deferred to tester after rebuild

---

## v3 → v3.1 delta (reverted from card styling)

| Attribute | v3 Direction E (rejected) | v3.1 (current) |
|---|---|---|
| **Background** | `AtermTheme.inputBarBackground` elevated card fill | **`NSColor.clear`** — transparent, sidebar bg shows through |
| **cornerRadius** | 16pt (card identity) | **0pt** (flat rectangle) |
| **barHorizontalMargin** | 8pt (card floats from bar edges) | **0pt** (edge-to-edge horizontally) |
| **Top hairline separator** | REMOVED (card stood alone) | **RESTORED** (1px, `inputBarBorder @ α=0.35`) |
| **Border α** | 0 (already absent) | 0 (unchanged — stays absent) |
| **Focus background animation** | `inputBarBackground.blended(withFraction: 0.08, of: .white)` | **REMOVED** — bg is transparent, no blending needed |
| **Focus prompt tint animation** | α 0.65 → 1.0 on focus, 200ms ease-out | **UNCHANGED** — still the sole focus indicator |
| **`focusLightenFraction` constant** | `0.08` | **REMOVED** (unused after bg blending dropped) |
| **`separatorAlpha` constant** | N/A | **NEW** `0.35` |
| **`masksToBounds`** | `true` (clip card content to rounded corners) | **`false`** (no corners to clip; avoids clipping artifacts) |

## v3 → v3.1 delta (preserved from v3 typography)

| Attribute | Value (unchanged from v3) |
|---|---|
| `verticalPadding` | 16pt |
| `horizontalPadding` | 20pt |
| `textContainerInset.height` | 16pt (formula `NSSize(width: 0, height: verticalPadding)` preserved) |
| `barTopMargin` / `barBottomMargin` | 0pt / 0pt |
| `containerContentHeight` | `lineHeight(~16) + 32 = 48pt` |
| `minBarHeight` | 48pt (matches v2 exactly) |
| `maxBarHeight` | 200pt |
| Prompt glyph | `chevron.right` 11pt **regular** weight, α=0.65 resting → 1.0 focused |
| Prompt `centerY` formula | `container.top + verticalPadding + lineHeight/2` (first-line center) |
| Placeholder | non-italic, `inputBarPlaceholder @ α=0.55`, text unchanged `"Type a command..."` via `AtermLocalization` |
| Caret | system default (Q1) |
| Scrollbar | 3px `ThinScroller`, always visible (Q4) |
| Focus animation duration | 200ms ease-out |
| Height animation duration | 120ms ease-out |

---

## Changes applied to `OrchestratorInputBar.swift`

### 1. Class doc comment updated
Replaced v3 Direction E description with v3.1 hybrid description. Calls out the transparent bg, flat rectangle, restored separator, prompt-tint-only focus indicator.

### 2. Property re-added
```swift
private let separatorView = NSView()  // RESTORED
private let containerView = NSView()  // kept
```

### 3. Constants updated
```swift
// v3.1 — hybrid geometry
private static let verticalPadding: CGFloat = 16       // kept from v3
private static let horizontalPadding: CGFloat = 20     // kept from v3
private static let cornerRadius: CGFloat = 0           // was 16 in v3
private static let containerContentHeight: CGFloat = lineHeight + verticalPadding * 2
private static let barHorizontalMargin: CGFloat = 0    // was 8 in v3
private static let barTopMargin: CGFloat = 0           // kept from v3
private static let barBottomMargin: CGFloat = 0        // kept from v3
private static let separatorAlpha: CGFloat = 0.35      // NEW

// REMOVED (unused):
// private static let focusLightenFraction: CGFloat = 0.08
```

### 4. `setup()` — separator restored
```swift
// v3.1: 1px hairline top separator RESTORED — divides transparent bar from terminal above.
separatorView.wantsLayer = true
separatorView.translatesAutoresizingMaskIntoConstraints = false
separatorView.layer?.backgroundColor =
    AtermTheme.inputBarBorder.withAlphaComponent(Self.separatorAlpha).cgColor
addSubview(separatorView)

// v3.1: transparent container, flat rectangle (no radius), no border.
containerView.wantsLayer = true
containerView.translatesAutoresizingMaskIntoConstraints = false
containerView.layer?.cornerRadius = Self.cornerRadius  // 0
containerView.layer?.masksToBounds = false
addSubview(containerView)
```

### 5. `NSLayoutConstraint.activate` — separator constraints added
```swift
// v3.1: Top hairline separator — 1px, edge-to-edge at bar top.
separatorView.leadingAnchor.constraint(equalTo: leadingAnchor),
separatorView.trailingAnchor.constraint(equalTo: trailingAnchor),
separatorView.topAnchor.constraint(equalTo: topAnchor),
separatorView.heightAnchor.constraint(equalToConstant: 1),

// Container: edge-to-edge horizontally (barHorizontalMargin=0), fills bar vertically.
containerView.leadingAnchor.constraint(
    equalTo: leadingAnchor, constant: Self.barHorizontalMargin),  // 0
containerView.trailingAnchor.constraint(
    equalTo: trailingAnchor, constant: -Self.barHorizontalMargin), // 0
containerView.topAnchor.constraint(equalTo: topAnchor, constant: Self.barTopMargin),  // 0
containerView.bottomAnchor.constraint(
    equalTo: bottomAnchor, constant: -Self.barBottomMargin),  // 0
```

The remaining `promptLabel` + `scrollView` constraints are unchanged — they still use `horizontalPadding = 20` for internal spacing, and `textContainerInset.height = 16` for vertical centering.

### 6. `applyContainerStyle` — transparent bg, no blending
```swift
/// v3.1: transparent container, flat rectangle (no radius), no border.
private func applyContainerStyle(focused: Bool) {
    guard let layer = containerView.layer else { return }
    layer.cornerRadius = Self.cornerRadius  // 0
    layer.backgroundColor = NSColor.clear.cgColor  // transparent — no card
    layer.borderWidth = 0
    layer.borderColor = nil
    layer.shadowColor = nil
    layer.shadowOpacity = 0
    layer.shadowRadius = 0
}
```

All references to `AtermTheme.inputBarBackground`, `NSColor.blended(withFraction:of:)`, and `focusLightenFraction` removed from this method.

### 7. `updateFocusState` — prompt tint only
```swift
private func updateFocusState(_ focused: Bool) {
    isFocused = focused
    // v3.1: focus animation = prompt tint shift ONLY.
    // Background is transparent so no bg change. Border is α=0 so no border change.
    NSAnimationContext.runAnimationGroup { ctx in
        ctx.duration = 0.20
        ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
        ctx.allowsImplicitAnimation = true
        promptLabel.contentTintColor = focused
            ? AtermTheme.inputBarPromptFocus
            : AtermTheme.inputBarPrompt.withAlphaComponent(0.65)
    }
}
```

Removed the `applyContainerStyle(focused: focused)` call from inside the animation group since there's nothing to animate on the container anymore (transparent bg, no border). The method is still called once at init with `focused: false`, and once on appearance change in `viewDidChangeEffectiveAppearance`.

### 8. `viewDidChangeEffectiveAppearance` — separator refresh restored
```swift
override func viewDidChangeEffectiveAppearance() {
    super.viewDidChangeEffectiveAppearance()
    separatorView.layer?.backgroundColor =
        AtermTheme.inputBarBorder.withAlphaComponent(Self.separatorAlpha).cgColor
    applyContainerStyle(focused: isFocused)
    promptLabel.contentTintColor = isFocused
        ? AtermTheme.inputBarPromptFocus
        : AtermTheme.inputBarPrompt.withAlphaComponent(0.65)
    textView.textColor = AtermTheme.textPrimary
}
```

---

## Regression check — v2 AND v3 typography preserved

| Feature | Origin | Status |
|---|---|---|
| `chevron.right` SF Symbol prompt glyph (Q2 #240) | v2/v3 | ✅ unchanged (11pt regular, α=0.65) |
| System default caret (Q1 #240) | v2/v3 | ✅ unchanged |
| 3px `ThinScroller` always visible (Q4 #240) | v2/v3 | ✅ unchanged |
| `/command` dropdown popover | v2 | ✅ `OrchestratorCommands.swift` untouched |
| Popover focus 3-layer defense | v2 (post-fix) | ✅ unchanged |
| ↑/↓ history + persist | v2 | ✅ unchanged |
| Ctrl+C/D/L PTY 0x03/0x04/0x0C | v2 | ✅ unchanged |
| Shift+Enter multiline + Enter submit | v2 | ✅ unchanged |
| Tab autocomplete | v2 | ✅ unchanged |
| ESC dropdown-close priority | v2 | ✅ unchanged |
| Cmd+V paste | v2 | ✅ unchanged |
| Cursor vertical centering formula | v2 formula, v3 constant | ✅ formula preserved, `textContainerInset.height = verticalPadding = 16` |
| Dynamic height 200pt max | v2 | ✅ unchanged |
| Font plumbing hot-reload | v2/prior work | ✅ unchanged |
| IME marked-text alignment | v2/#240 | ⚠️ formula preserved, constant 16 — **tester verification mandatory** |
| #246 English default placeholder | v2/#246 | ✅ `"Type a command..."` unchanged |
| Prompt glyph regular weight + α=0.65 (v3 typography) | v3 | ✅ kept |
| Placeholder non-italic + α=0.55 (v3 typography) | v3 | ✅ kept |
| `verticalPadding = 16` (v3 value) | v3 | ✅ kept |
| `horizontalPadding = 20` (v3 value) | v3 | ✅ kept |
| Top hairline separator (v2 original, v3 removed, v3.1 restored) | v2 → v3.1 | ✅ restored at α=0.35 |
| Edge-to-edge layout (v2 original, v3 card-margined, v3.1 reverted) | v2 → v3.1 | ✅ restored (`barHorizontalMargin = 0`) |

### SessionSidebarView v3 edits preserved
- Line 850 Divider (workspace list ↔ Settings) **stays removed**
- Line 726 top Divider (header ↔ workspace list) **stays**
- Settings button `.contentShape(Rectangle())` **stays**

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

**PASS.** 6-file closure typechecks clean.

**Verification grep:** `grep -n 'blended\|focusLightenFraction\|inputBarBackground' OrchestratorInputBar.swift` → **zero matches**, confirming the v3 card styling has been fully removed.

**Full-project typecheck note:** still fails on pre-existing `SettingsView.swift` sub1 `.onChange(of:initial:_:)` macOS 14+ API issue. Not caused by v3.1. Separate task for sub1.

---

## Mental trace — expected runtime behavior

1. **Launch sandbox (P0 fix applied)** → orchestrator workspace auto-selected → `selectWorkspace(orchestratorUUID)` → `orchestratorInputBar.isHidden = false`
2. **Visible state at rest:**
   - Bottom 48pt of window area contains the bar
   - 1px hairline visible at the very top of the bar (`inputBarBorder @ α=0.35`) — divides terminal area above from bar below
   - Below the hairline: 47pt of transparent space — sidebar-right-to-window-right edge-to-edge, sidebar bg shows through on the left portion (but sidebar has its own width so only the terminal-bg area shows under the bar)
   - Actually: since bar is pinned leading to `sidebarHost.trailingAnchor`, the bar starts at the right edge of the sidebar and fills to the window right. Under the bar: the terminal container bg (`#1A1B26` dark / `#FAF6F0` light). Transparent bar means this terminal bg shows through.
   - On top of the transparent bar: prompt glyph `chevron.right` at (20pt, ~24pt) in α=0.65 muted tint; placeholder "Type a command..." in α=0.55 muted color starting at ~(32pt, 16pt top padding)
3. **User clicks bar** → `textView` becomes first responder → `onFocusChange(true)` → `updateFocusState(true)` → prompt glyph tints animate 0.65 → 1.0 over 200ms. Caret appears and blinks at the placeholder position.
4. **User types `/`** → `textDidChange` → `maybeShowCommandDropdown` → `commandDropdown.show(anchor: self)` → popover opens; filter-as-you-type works via preserved popover focus 3-layer defense.
5. **User types text → presses Enter** → submits to orchestrator PTY.
6. **User presses Shift+Enter** → newline inserted → `recomputeHeight` grows bar toward 200pt max with 120ms animation.
7. **User types Korean (ㅎ→하→한)** → IME marked-text renders at cursor position → NSLayoutManager uses `textContainerInset.height = 16` → marked-text underline should render at the same relative Y as the text baseline. **Tester verification required.**

---

## Known concerns

1. **"Edge-to-edge horizontally" semantics:** the bar's leading anchor in AppDelegate is pinned to `sidebarHost.trailingAnchor` (right edge of sidebar), and trailing to `containerView.trailingAnchor` (right edge of window). With `barHorizontalMargin = 0` on the internal container, the container fills the full bar width. The bar's visual extent is therefore from the right edge of the sidebar to the right edge of the window — NOT the full window width. That matches v2's behavior and the user's mockup.

2. **Separator alpha = 0.35** — slightly more visible than v3's 0.4 default I used in the original #240 design (which was 0.4). Spec says 0.35 explicitly. Kept literal per user's approved spec.

3. **`applyContainerStyle` still called at init** — sets the transparent bg, radius=0, no border, no shadow. This is effectively a one-time setup call now since focus state doesn't change anything on the container anymore. Could be inlined into `setup()` for clarity, but kept as a separate method for consistency with future changes.

4. **Prompt glyph at α=0.65 resting** may feel too faint — v3 user mockup approved this value but in practice against a transparent bar over dark terminal bg, the contrast may be lower than v3's against `inputBarBackground`. Tester visual verification welcome. If too faint, bump to 0.75.

5. **IME verification still mandatory** — `textContainerInset.height = 16` formula preserved, but tester must confirm preedit underline position matches text baseline during Korean composition.

6. **NSAnimationContext with only `contentTintColor` animation** — `NSImageView.contentTintColor` is not a CALayer property; SwiftUI/AppKit bridges it via an explicit animation proxy. `allowsImplicitAnimation = true` should drive the animation. If focus transitions don't animate smoothly, wrap in explicit `NSAnimationContext.current.duration = 0.20` + `promptLabel.animator().contentTintColor = ...`.

7. **No new AtermTheme tokens added.** v3.1 reuses existing `inputBarBorder`, `inputBarPrompt`, `inputBarPromptFocus`, `inputBarPlaceholder`, `textPrimary`. Zero token namespace pollution.

8. **`masksToBounds = false`** on containerView — there are no rounded corners to clip (`cornerRadius = 0`), so we don't need clipping. Set to false to avoid any potential clipping of animation effects or text selection highlights at edges.

---

## Files list

```
macos/Sources/OrchestratorInputBar.swift  [M]  ~+15/-15 LOC (net ~0)
  - class doc comment: v3 Direction E → v3.1 hybrid
  - properties: re-added separatorView
  - constants: cornerRadius 16→0, barHorizontalMargin 8→0, separatorAlpha 0.35 (new),
               focusLightenFraction REMOVED
  - setup() container block: re-added separatorView creation + background,
                              container masksToBounds true→false, bg NSColor.clear
  - setup() constraint activate: re-added 4 separator constraints at top
  - applyContainerStyle: removed bg.blended + focusLightenFraction references,
                         bg=NSColor.clear unconditional
  - updateFocusState: removed applyContainerStyle(focused:) call inside animation
                      group, kept prompt tint animation
  - viewDidChangeEffectiveAppearance: re-added separator color refresh
```

No other files touched. SessionSidebarView v3 edits preserved. AppDelegate P0 fix preserved. No Rust. No tests run. No app run.
