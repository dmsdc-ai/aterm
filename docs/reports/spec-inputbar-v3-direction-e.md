# SPEC: OrchestratorInputBar v3 Direction E (corrected — tone-only borrow)

**Status:** Draft REVISED — awaiting user approval
**Priority:** P2 (queue position 3, after P0 + active session scope)
**Owner:** aigentry-aterm-claude
**User correction:** "디자인톤만 참고해. 그대로 디자인 하지 말고. 우리 기능에 맞추되 디자인톤만 참고해." — borrow only the design tone; do NOT copy layout literally; keep OUR existing v2 functionality, only the aesthetic style should inform the design
**Prior revision (incorrect):** ref `{prior inject}` — misinterpreted reference image as feature additions (attach/model/mic buttons). That version is **REPLACED** by this corrected SPEC.

---

## Goal

Apply the **visual tone** of the claude.ai-style reference card to OrchestratorInputBar v3, while keeping **100% of v2's existing functionality and single-text-area layout**. No new buttons, no action row, no attach/model/mic controls. The text area is restyled as a self-contained elevated card with generous padding, larger corner radius, and multi-line breathing room — but the content is identical to v2.

---

## What "tone" means here (user-specified from reference image)

| Tone element | Reference | Apply? |
|---|---|---|
| Larger corner radius (~14–18pt range) | ✅ | **Yes** — card identity |
| Elevated subtle card-like background | ✅ | **Yes** — use existing `inputBarBackground` token or new `inputBarCard` |
| Generous padding (18–22pt horizontal + vertical) | ✅ | **Yes** — breathing room |
| No hard border, nearly invisible or absent (α ≤ 0.15) | ✅ | **Yes** — remove v2's 1px opaque border |
| No top hairline separator | ✅ | **Yes** — card stands alone, not flush to sidebar edge |
| Top-left placeholder anchor (multi-line feel) | ✅ | **Yes** — text anchored top-left, not vertically centered in single-line |
| Default empty state taller than single line (~70–90pt) | ✅ | **Yes** — 2-line equivalent empty state |
| Regular weight, non-italic, muted placeholder | ✅ | **Yes** — drop italic from placeholder attributes |
| Subtle muted overall tone, no high-contrast | ✅ | **Yes** — align with reference aesthetic |

## What is NOT borrowed from the reference (tone only, not layout)

| Reference element | Borrow? | Reason |
|---|---|---|
| Attach button `[+]` | ❌ No | Not in v2 functional scope; user explicitly removed |
| Model label + chevron | ❌ No | User explicitly removed; orchestrator CLI is set via Settings |
| Microphone button | ❌ No | User explicitly removed; #78 voicecode deferred |
| Bottom action row layout | ❌ No | v2 is single-text-area; keep that structure |
| Korean reference placeholder text | ❌ No | v2 uses "Type a command..." via AtermLocalization (#246 English default) |

---

## Preserved from v2 (absolute)

- Single `OrchestratorTextView` input area in `NSScrollView` (no split zones, no action row, no subviews besides text)
- `chevron.right` small SF Symbol prompt glyph (Q2 decision from #240 — kept, possibly resized/recolored for tone match)
- System default caret (Q1)
- 3px `ThinScroller` always visible (Q4)
- `/command` dropdown via `CommandDropdown` NSPopover + `DropdownTableView` subclass
- ↑/↓ history recall + persist via `OrchestratorHistory`
- Ctrl+C/D/L PTY transmission (bytes 0x03, 0x04, 0x0C)
- Shift+Enter multiline + Enter submit
- Tab autocomplete (single-match + dropdown interaction)
- ESC dropdown-close priority → toggle focus to terminal when no dropdown
- Cmd+V paste (NSTextView default responder chain)
- Cursor vertical centering formula: `textContainerInset.height` derived from line metrics (constant may shift for new padding, FORMULA unchanged)
- Dynamic height growth toward `maxBarHeight` ceiling (unchanged 200pt)
- Popover focus 3-layer defense: `DropdownTableView.acceptsFirstResponder = false` + inline `configurePopoverWindowAfterShow` + `anchorWindow.makeKey()`
- Font plumbing hot-reload (`GlyphAtlas.resolvedFontFamily`)
- IME marked-text alignment (no `NSTextInputClient` override, `textContainerInset` symmetric)
- #246 English default placeholder: `"Type a command..."` via `AtermLocalization.text(ko:en:)`

---

## Structural changes vs v2 (tone-only refinement)

| Attribute | v2 current | v3 Direction E (corrected) |
|---|---|---|
| **Container background** | `NSColor.clear.cgColor` (transparent) | **`AtermTheme.inputBarBackground.cgColor`** (reused, subtle elevation +4 lightness over sidebar bg) |
| **Container corner radius** | 8pt | **16pt** (picked from 14–18pt range, sweet spot for card identity) |
| **Container border** | 1px opaque, `inputBarBorder` / `inputBarBorderFocus` color change | **None by default; α=0.10 hairline on focus** (barely visible, caret blink is primary focus signal) |
| **Container shadow** | None | None (unchanged — minimal tone) |
| **Top separator (1px hairline)** | Present, `inputBarBorder @ α=0.4` | **REMOVED** — card stands alone, no flush edge |
| **Horizontal padding (container interior)** | 12pt leading + 10pt trailing (prompt-centric) | **20pt uniform** (picked from 18–22pt range) |
| **Vertical padding (top of text area)** | 9pt via `textContainerInset.height` | **20pt via `scrollView.topAnchor = container.top + 20`** (moved out of textContainerInset to achieve top-anchored text for multi-line idiom) |
| **Vertical padding (bottom of text area)** | 9pt via `textContainerInset.height` | **20pt via `scrollView.bottomAnchor = container.bottom - 20`** (symmetric) |
| **`textContainerInset`** | `NSSize(width: 0, height: 9)` | **`NSSize(width: 0, height: 0)`** — padding moved to scrollView anchors so text naturally top-aligns within the (now-taller) scroll area |
| **Default empty state bar height** | ~48pt (single-line text: 16 + 18 padding + 14 margin) | **~84pt** (2-line text area + 40pt padding + 16pt margins, picked from 70–90pt range) |
| **Text area minimum content height** | `lineHeight` (~16pt, single line) | **`lineHeight * 2` (~32pt, 2 lines)** — creates multi-line breathing room even when empty |
| **Max bar height** | 200pt | 200pt (unchanged per user constraint) |
| **Prompt glyph** | `chevron.right` 11pt semibold, 12x12 frame | **`chevron.right` 11pt regular** (slightly lighter weight for tone), 12x12 frame, position at top-left corresponding to first line baseline — same anchor formula but with new padding constants |
| **Prompt glyph color** | `inputBarPrompt` / `inputBarPromptFocus` | `inputBarPrompt @ α=0.65` (muted) / `inputBarPromptFocus` (unchanged focus color) |
| **Placeholder style** | Italic, `inputBarPlaceholder` color | **Regular (non-italic)**, `inputBarPlaceholder @ α=0.55` (lower opacity) |
| **Placeholder text content** | "Type a command..." / "명령어 입력..." | **Unchanged** — same text via AtermLocalization (#246 preserved) |
| **Focus state visual** | Border color change (150ms) | **Background lightness shift** via `inputBarBackground` → `inputBarBackground @ ~+3% lightness`, plus optional α=0.10 hairline border fade-in, 200ms ease-out |
| **Focus animation duration** | 150ms ease-out | **200ms ease-out** (slightly slower for card feel, matches reference's relaxed aesthetic) |
| **Dynamic height animation** | 120ms ease-out via `heightConstraint.animator().constant` | **180ms ease-out** (slightly slower for larger card feel) |
| **Scrollbar** | 3px `ThinScroller` always visible | Unchanged (Q4 preserved) |

---

## New AtermTheme tokens — NONE required

**Decision:** reuse existing `AtermTheme.inputBarBackground` for the card background. DESIGN-002 originally provisioned this token for exactly this purpose (v2 minimal redesign temporarily dropped its use by setting `layer.backgroundColor = NSColor.clear.cgColor`; v3 restores its intended role).

Existing tokens reused:
- `AtermTheme.inputBarBackground` — card bg (dark: `#1E1F2B`, light: `#EFEBE4` — +4 lightness over sidebar `#0A0A0A`/`#F0EBE3`)
- `AtermTheme.inputBarBorder` — optional α=0.10 hairline on focus
- `AtermTheme.inputBarPrompt` / `inputBarPromptFocus` — prompt glyph tints
- `AtermTheme.inputBarPlaceholder` — placeholder text color (α=0.55 override at render time)
- `AtermTheme.textPrimary` — input text color (unchanged)

No namespace pollution. No new token migration risk.

---

## Content layout (constraint graph)

```
┌─ OrchestratorInputBar (NSView) ─────────────────────────┐
│                                                          │
│  ┌─ containerView (inputBarBackground, 16pt radius) ─┐  │
│  │                                                   │  │
│  │  ┌─ promptLabel (chevron.right, 11pt regular)     │  │
│  │  │  at (20pt from container.leading,              │  │
│  │  │       first-line center Y)                     │  │
│  │  │                                                │  │
│  │  ┌─ scrollView (pinned 20pt container insets) ─┐ │  │
│  │  │                                              │ │  │
│  │  │  textView                                    │ │  │
│  │  │  "Type a command..." top-left placeholder    │ │  │
│  │  │                                              │ │  │
│  │  │  (empty state ≥ 2 line heights breathing room)│ │  │
│  │  │                                              │ │  │
│  │  └──────────────────────────────────────────────┘ │  │
│  └───────────────────────────────────────────────────┘  │
│                                                          │
└──────────────────────────────────────────────────────────┘
     (no top separator, no bottom toolbar, no action row)
```

### Constraints

| View | Anchors |
|---|---|
| `containerView` | `leading=bar.leading+8, trailing=bar.trailing-8, top=bar.top+barTopMargin, bottom=bar.bottom-barBottomMargin` |
| `promptLabel` | `leading=container.leading+horizontalPadding, centerY=container.top + verticalPadding + lineHeight/2, width=12, height=12` |
| `scrollView` | `leading=promptLabel.trailing + 8, trailing=container.trailing-horizontalPadding, top=container.top+verticalPadding, bottom=container.bottom-verticalPadding` |
| `textView` | `scrollView.documentView`, fills |

### Constants

```swift
private static let inputFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
private static let lineHeight: CGFloat = {
    let lm = NSLayoutManager()
    return ceil(lm.defaultLineHeight(for: inputFont))
}()

// Direction E (corrected) geometry
private static let cornerRadius: CGFloat = 16
private static let horizontalPadding: CGFloat = 20
private static let verticalPadding: CGFloat = 20
private static let defaultLineCount: Int = 2  // empty state breathing room
private static let textAreaMinHeight: CGFloat = lineHeight * CGFloat(defaultLineCount)
private static let barTopMargin: CGFloat = 8
private static let barBottomMargin: CGFloat = 8

private static let containerContentHeight: CGFloat =
    verticalPadding       // top scroll padding
    + textAreaMinHeight   // 2-line text area
    + verticalPadding     // bottom scroll padding
//  = 20 + 32 + 20 = 72pt

static let minBarHeight: CGFloat =
    containerContentHeight + barTopMargin + barBottomMargin
//  = 72 + 8 + 8 = 88pt

private static let maxBarHeight: CGFloat = 200  // unchanged per user constraint
```

**Height computation:** lineHeight ≈ 16pt, `textAreaMinHeight = 32`, `containerContentHeight = 72`, `minBarHeight = 88pt`. That's **+40pt vs v2's 48pt** (~83% taller minimum, but still well under v2's 200pt max).

---

## Dynamic height (`recomputeHeight`)

```swift
private func recomputeHeight() {
    textView.layoutManager?.ensureLayout(for: textView.textContainer!)
    let used = textView.layoutManager?.usedRect(for: textView.textContainer!).size.height
        ?? Self.lineHeight
    // Text area grows with content but never shrinks below 2 lines minimum
    let textContentHeight = max(Self.textAreaMinHeight, used)
    // Container = top pad + text + bottom pad
    let contentHeight = Self.verticalPadding + textContentHeight + Self.verticalPadding
    let target = min(
        Self.maxBarHeight,
        max(Self.minBarHeight, contentHeight + Self.barTopMargin + Self.barBottomMargin))
    if abs(heightConstraint.constant - target) > 0.5 {
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.18
            ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
            ctx.allowsImplicitAnimation = true
            heightConstraint.animator().constant = target
        })
        onHeightChange?(target)
    }
}
```

---

## Files to modify

| # | File | Change | LOC delta |
|---|---|---|---|
| 1 | `macos/Sources/OrchestratorInputBar.swift` | Update constants (`cornerRadius 8→16`, `verticalPadding 9→20`, add `horizontalPadding=20`, `defaultLineCount=2`, `textAreaMinHeight`), rewrite `applyContainerStyle` (background instead of border color, optional α=0.10 hairline fade on focus), move vertical padding from `textContainerInset` to `scrollView` anchor constants, update prompt glyph weight and tint alpha, remove `separatorView` subview + constraints, update `recomputeHeight` formula, update placeholder attributes (drop italic, lower alpha) | ~+25 / -30 (net -5) |
| 2 | `macos/Sources/AtermTheme.swift` | **No changes** — reuse existing tokens | 0 |

**NOT touched:**
- `OrchestratorCommands.swift` — CommandDropdown + DropdownTableView unchanged, popover focus fix intact
- `SessionSidebarView.swift` — unrelated
- `AppDelegate.swift` — bar install constraints unchanged (only reads `OrchestratorInputBar.minBarHeight` static which auto-updates to 88pt)
- `GlyphAtlas.swift` / font plumbing — not affected
- `SettingsView.swift` (sub1 territory)
- `TerminalView.swift`

---

## Verification (mental trace, no app run)

1. **`swiftc -typecheck`** on 6-file closure — must pass exit 0 (with the caveat that SettingsView.swift sub1 pre-existing errors may block full-project compile; isolated OrchestratorInputBar.swift typecheck will pass)
2. **Keyboard handler trace — unchanged paths:**
   - `/` typed → `textDidChange` → `maybeShowCommandDropdown` → `commandDropdown.show(anchor: self)` → popover anchored at `self.bounds` (now 88pt tall) with `preferredEdge: .maxY` → NSPopover auto-flips up if off-screen → popover visible above bar
   - Enter → `handleEnter` dropdown-priority branch → `applyCommand` or submit
   - All arrow/Tab/ESC/Ctrl/Cmd+V paths unchanged
3. **Cursor vertical centering preservation:**
   - v2: `textContainerInset.height = 9` inside textView; scrollView flush with container; first line center Y = `container.top + 9 + lineHeight/2 = container.top + 17`
   - v3: `textContainerInset.height = 0`; scrollView top anchor = `container.top + 20`; first line center Y = `container.top + 20 + 0 + lineHeight/2 = container.top + 28`
   - **Formula preserved** — still derived from line metrics, just the constant shifted from 9 to 20. Caret is naturally top-anchored in the taller empty state (first line at 20pt from card top, additional space below for more lines).
   - Single-line input: text starts at y=20 from container top, lineHeight=16 consumed, 36pt of empty space below. This is INTENTIONAL — the multi-line breathing-room design.
4. **Prompt glyph alignment with new padding:**
   - `promptLabel.centerY = container.top + verticalPadding + lineHeight/2 = container.top + 28`
   - Matches first-line center of text in scrollView → prompt stays aligned with line 1 in single-line and multi-line state
5. **IME marked-text preservation:**
   - v2: `textContainerInset.height = 9` → marked-text baseline at scroll-view-relative y = 9 + ascender
   - v3: `textContainerInset.height = 0` → marked-text baseline at scroll-view-relative y = 0 + ascender = ascender
   - Different absolute position but still computed from the SAME layout manager and container formula. NSLayoutManager renders glyphs + underline consistently regardless of the inset value.
   - **Risk area** — needs tester verification with 한글 composition (ㅎ→하→한). Fallback: revert to `textContainerInset.height = 9` and move padding to `scrollView.topAnchor` only (asymmetric: top via scrollView, bottom via textContainer) — but that's more complex. Start with the clean symmetric approach.
6. **Popover anchor verification:**
   - `commandDropdown.show(anchor: self, prefix: ...)` → popover positioned relative to `self.bounds`
   - With 88pt-tall bar at window bottom, `preferredEdge: .maxY` (below) would be off-screen → NSPopover auto-flips to above
   - Popover opens above the card with 3px clearance → visible to user
   - **Needs runtime verification** — if auto-flip misbehaves, change explicit `preferredEdge: .minY` in `CommandDropdown.show()`
7. **Dropdown popover filter-as-you-type preservation:**
   - `DropdownTableView.acceptsFirstResponder = false` still in place → popover window non-key → OrchestratorTextView stays first responder → character keys still flow to text view → filter updates live
   - No regression from v3 structural changes

---

## Risks

| # | Risk | Severity | Mitigation |
|---|---|---|---|
| 1 | **+40pt vertical real estate consumed** (48 → 88pt). Terminal area shrinks by ~4% on 900pt window. | **Low** | Much smaller than prior Direction E misinterpretation (140pt). Users get more breathing room without severe terminal loss. If still too tall, lower `defaultLineCount` from 2 to 1.5 (lineHeight*1.5 = 24pt; minBarHeight = 80pt) |
| 2 | IME marked-text alignment may shift due to `textContainerInset.height` change from 9 → 0 (padding moved to scroll view anchors) | **Medium** | Mental trace shows formula preserved, just constants shifted. Tester MUST verify 한글 composition. If misaligned, fall back to keeping `textContainerInset.height = 9` and setting `scrollView.topAnchor = container.top + (verticalPadding - 9)` for asymmetric padding |
| 3 | Transparent background replaced with `inputBarBackground` — slight color shift visible at transition during focus state | **Low** | 200ms animation masks the shift; users perceive it as "bar lights up on focus" |
| 4 | Removed top separator + new background → card boundary may be unclear against sidebar bg | **Low** | `inputBarBackground` is +4 lightness over `sidebarBackground` — visible contrast. Corner radius 16pt provides additional visual boundary. If still unclear, add back 1px hairline above card at α=0.10 |
| 5 | Prompt glyph weight change (semibold → regular) may reduce visual prominence | **Low** | Alpha reduction to α=0.65 resting + full alpha on focus still provides affordance |
| 6 | Placeholder drop italic may confuse users expecting italic cue | **Low** | Italic vs regular is cosmetic; text content and position are the primary affordance |
| 7 | `recomputeHeight` animation timing (180ms) may compete with other UI transitions | **Low** | 180ms is within normal range; no compound animation cases observed |
| 8 | Popover `.transient` auto-dismiss behavior with taller bar | **Low** | Bar position at window bottom unchanged; popover anchor logic unchanged; auto-flip should work as before |
| 9 | Minimum window height constraint (current: 400pt per AppDelegate:80) leaves 312pt for terminal after 88pt bar — tight but usable | **Low** | Existing min window is already generous; 400 - 88 = 312pt = ~19 rows of terminal at 16pt cell height — acceptable for minimal use |

---

## Questions for user decision

### Q1 — Exact corner radius within 14–18pt range

**Recommendation:** 16pt (middle of range, card identity)
**Alternatives:** 14pt (subtler), 18pt (more pronounced)

### Q2 — Exact padding numerics (both horizontal and vertical)

**Recommendation:** 20pt uniform (horizontal = vertical = 20)
**Alternatives:**
- 18pt uniform → minBarHeight ≈ 84pt (+2pt savings)
- 22pt uniform → minBarHeight ≈ 92pt (+4pt more breathing room)
- Asymmetric: 20pt horizontal + 22pt vertical → minBarHeight ≈ 92pt

### Q3 — Exact empty state height (via `defaultLineCount`)

**Recommendation:** `defaultLineCount = 2` → textAreaMinHeight = 32pt → minBarHeight = 88pt
**Alternatives:**
- `defaultLineCount = 1.5` (non-integer requires Float casting) → minBarHeight ≈ 80pt — closer to v2's 48pt + 32pt breathing
- `defaultLineCount = 2` → minBarHeight = 88pt ← recommended
- `defaultLineCount = 3` → minBarHeight ≈ 104pt — too tall, closer to prior Direction E misinterpretation

### Q4 — New AtermTheme token or reuse existing

**Recommendation:** **reuse `AtermTheme.inputBarBackground`** (existing, same hex as what DESIGN-002 provisioned)
**Alternative:** add new `inputBarCard` token with identical value for semantic clarity — adds 4 lines of AtermTheme.swift, no functional difference

### Q5 — Prompt glyph size/weight adjustment or keep 11pt semibold

**Recommendation:** `chevron.right` 11pt **regular** weight (lighter than v2's semibold for tone match), 12x12 frame unchanged
**Alternatives:**
- 11pt semibold unchanged (v2 exact) — preserves #240 Q2 verbatim
- 10pt regular (smaller, more muted)
- 12pt regular (slightly larger, more prominent)

### Q6 — Focus state subtle tint color choice

**Recommendation:** **background lightness shift** — `inputBarBackground` at rest, `inputBarBackground @ +3% lightness` on focus, 200ms ease-out
**Alternatives:**
- Border fade-in only (α=0→0.10 on focus) — simpler, subtler
- Both: lightness shift + border fade-in
- Caret blink only (no container change) — absolute minimum

### Q7 — Dropdown anchor positioning with taller card

NSPopover auto-flip should handle it, but if the 88pt-tall bar causes clipping, should `CommandDropdown.show()` explicitly use `preferredEdge: .minY` (above) instead of the current `.maxY` (below)?

**Recommendation:** keep `.maxY` (default), rely on auto-flip. If runtime verification shows clipping, switch to `.minY` in a follow-up one-line fix.

### Q8 — IME preedit underline verification

**Recommendation:** tester MUST verify 한글 composition (ㅎ→하→한) after rebuild. If marked-text underline misaligns due to `textContainerInset.height` change (9→0), fallback to asymmetric padding approach: keep `textContainerInset.height = 9` and set `scrollView.topAnchor = container.top + (verticalPadding - 9) = container.top + 11`. This preserves marked-text baseline relative to textView top while still achieving the new visual padding.

---

## Preservation checklist

| v2 invariant | v3 Direction E status |
|---|---|
| `chevron.right` SF Symbol prompt glyph (Q2) | ✅ kept, weight/alpha adjusted for tone |
| System default caret (Q1) | ✅ unchanged (no `insertionPointColor` set) |
| 3px `ThinScroller` always visible (Q4) | ✅ unchanged |
| `/command` dropdown | ✅ `commandDropdown` property unchanged |
| ↑/↓ history with persist | ✅ `handleArrowUp/Down` + `history` unchanged |
| Ctrl+C/D/L PTY transmission | ✅ `OrchestratorTextView.keyDown` modifier check unchanged |
| Shift+Enter multiline + Enter submit | ✅ `handleShiftEnter` + `handleEnter` unchanged |
| Tab autocomplete | ✅ `handleTab` unchanged |
| ESC dropdown-close priority | ✅ `handleEscape` unchanged |
| Cmd+V paste | ✅ NSTextView default responder chain |
| Cursor vertical centering formula | ✅ formula preserved (constant shift 9→0, padding moved to scrollView anchor) |
| Dynamic height to 200pt max | ✅ unchanged (only min and animation duration tweaked) |
| Popover focus 3-layer defense | ✅ `OrchestratorCommands.swift` untouched |
| Font plumbing hot-reload | ✅ `makeStyledFont` reads `GlyphAtlas.shared.resolvedFontFamily` unchanged |
| IME marked-text alignment | ⚠️ formula preserved, constants shifted — tester must verify |
| #246 English default placeholder | ✅ "Type a command..." via AtermLocalization unchanged |

---

## Out of scope for this SPEC (follow-up ideas)

- User-configurable padding / corner radius in Settings
- Light mode specific tuning (inputBarBackground already has light variant)
- High-contrast accessibility mode
- Animation polish (hover states, press feedback)
- Attach/model/mic buttons — explicitly excluded per user correction; deferred if ever needed

---

## NO CODE CHANGES MADE. Awaiting `[IMPLEMENT APPROVED]` + answers to Q1–Q8 from orchestrator.

Queue position: **3** (after P0 default workspace impl COMPLETE and active session scope impl COMPLETE, both already reported in this session).
