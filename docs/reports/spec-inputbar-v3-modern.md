# SPEC: OrchestratorInputBar v3 modern redesign

**Status:** Draft — awaiting user approval
**Owner:** aigentry-aterm-claude
**User request:** "오케스트레이터 명령어바 좀더 심플하고 모던하게 디자인해줘" — more simple + more modern (evidence: Rebuild #88 screenshot of v2)
**Prior versions:** v1 DESIGN-002 (elevated card with shadow/glow), v2 #240 redesign (transparent + hairline + 1px border). v2 is current live state in Rebuild #88.

---

## Goal

Iterate on v2 to achieve "simpler AND more modern" per user feedback. Explore 4 directions (A/B/C/D) with explicit trade-offs; recommend one. NO implementation until user picks.

---

## v2 current state audit (from #240 REPORT ref ea1d288de553)

| Attribute | v2 value |
|---|---|
| **Prompt glyph** | `chevron.right` SF Symbol, 11pt semibold, 12x12 point frame |
| **Placeholder** | "Type a command..." / "명령어 입력..." via AtermLocalization, italic |
| **Top separator** | 1px hairline, `AtermTheme.inputBarBorder @ α=0.4`, full width |
| **Container background** | Transparent (`NSColor.clear.cgColor`) — sidebar shows through |
| **Container corner radius** | 8pt |
| **Container border** | 1px, `inputBarBorder` resting / `inputBarBorderFocus` on focus; **color change only**, no thickening |
| **Shadow** | None (explicitly disabled: `shadowColor=nil, shadowOpacity=0, shadowRadius=0`) |
| **Text vertical padding** | `textContainerInset.height = 9pt` (derived from `lineHeight + 2*9`) |
| **Container content height** | ~34pt (lineHeight ~16 + 2*9 padding) |
| **Minimum bar height** | ~48pt (34 container + 6 top margin + 8 bottom margin) |
| **Maximum bar height** | 200pt |
| **Caret color** | System default (no explicit tint) |
| **Scrollbar** | 3px `ThinScroller` subclass, always visible (`.legacy` style + `autohidesScrollers=false`) |
| **Focus animation** | 150ms ease-out, border color + prompt tint only |
| **Height animation** | 120ms ease-out via `NSAnimationContext` + `heightConstraint.animator().constant` |
| **Prompt position** | Anchored to `container.top + verticalPadding + lineHeight/2` (first-line center) |
| **Text scroll view insets** | Top/bottom flush with container (0pt); leading after prompt +8pt; trailing -10pt |

v2 rationale was "minimal redesign, drop elevation metaphor, flat 1px border with color-only focus". User feedback indicates this is still not minimal/modern enough.

---

## Preserve (absolute — must not regress)

- Q1 (#240): system default caret
- Q2 (#240): `chevron.right` small SF Symbol prompt glyph (may reduce size further in directions but NOT change to chevron.right.2 or '>' text)
- Q3 (#240): transparent base + 1px hairline top separator decision
- Q4 (#240): 3px thin scrollbar always visible
- Q5 (#240): 1px top separator (hairline)
- Cursor vertical centering root fix (#240): `textContainerInset.height` must remain derived from `NSLayoutManager.defaultLineHeight(for:)` — no hardcoded pixel values
- Font plumbing hot-reload + fallback chain (Q-font-1..5): uses `GlyphAtlas.shared.resolvedFontFamily`
- IME marked-text alignment (#240 + earlier): no changes to `textContainerInset` width; height must stay computed from line metrics
- All #240 keyboard functionality: history ↑/↓, Ctrl+C/D/L, Shift+Enter multiline, Tab autocomplete, ESC dropdown toggle, Cmd+V paste, / command dropdown
- Popover focus 3-layer defense: `DropdownTableView.acceptsFirstResponder = false` + inline `configurePopoverWindowAfterShow()` + `anchorWindow.makeKey()` post-show
- #246 English default locale (placeholder falls back to `"Type a command..."` not 한글)

---

## Direction A — Spacing & breathing room refinement

**Thesis:** v2 feels cramped, not visually unbalanced. Fix by expanding vertical padding, side margins, and glyph proportions WITHOUT adding or removing any visual elements.

### What changes from v2

| Attribute | v2 | A |
|---|---|---|
| `verticalPadding` | 9pt | **12pt** |
| `containerContentHeight` | ~34pt | ~40pt (lineHeight ~16 + 2*12) |
| `minBarHeight` | ~48pt | **~54pt** (40 + 6 top + 8 bottom) |
| Prompt glyph size | 11pt semibold | **12pt regular** (slightly larger, lighter weight) |
| Prompt leading inset | 12pt | **16pt** |
| Prompt-to-text gap | 8pt | **10pt** |
| Text trailing inset | 10pt | **14pt** |
| Container horizontal margin (to bar edges) | 8pt | **10pt** |
| Container corner radius | 8pt | 8pt (unchanged) |
| Border, separator, colors | unchanged | unchanged |

### Visual description (verbal)

A slightly taller bar (54pt vs 48pt) with more air around the text. The prompt glyph sits a bit further from the left edge (16pt) and is marginally larger but visually lighter (regular weight instead of semibold). Text starts 26pt in (vs 20pt). The container is 2pt narrower on each side. All other elements — transparency, border, hairline separator — remain untouched.

### Estimated LOC delta

**+0 / -0** logic; **~10 lines** of constant/constraint value changes in `OrchestratorInputBar.swift setup()`. Pure refactoring of layout constants.

### Trade-offs vs v2

| Factor | v2 | A | Δ |
|---|---|---|---|
| Vertical real estate consumed | 48pt | 54pt | **+6pt** (terminal area shrinks by 6pt) |
| Visual density | High (cramped) | Moderate | **+** (user perceives as less rushed) |
| Visual element count | 4 (separator, border, prompt, text) | 4 | **=** |
| Complexity | Low | Low | **=** |
| Modern feel | Minimal | Minimal | **=** (no new modern cues) |

### Risk to #240 functionality

**Low.** Only layout constants change. `minBarHeight` computed via same formula, just with different `verticalPadding`. `recomputeHeight` math still correct. All keyboard handlers untouched. IME marked-text: changes to `textContainerInset.height` stay in the same formula, so preedit underline still aligns with text baseline.

### Which user feedback does it address?

Addresses **"simple"** indirectly (less cramped = easier to read = feels simpler to interact with). Does NOT address **"modern"** — no new modern styling cues.

---

## Direction B — Subtle modern accents

**Thesis:** v2 is structurally correct but lacks the small visual affordances that modern Mac/iOS UIs use to feel refined. Add subtle modern touches without adding visual noise: a thin focus underline, refined placeholder, smoother animation curves, inset shadow hint on focus.

### What changes from v2

| Attribute | v2 | B |
|---|---|---|
| Border visible state | 1px border always, color changes on focus | **0px border unfocused** (transparent container only), **1px bottom-only underline** that appears on focus in `inputBarBorderFocus` color |
| Focus indicator | Border color change | **Bottom underline** (2px tall, inset 4pt from each side, indigo accent) animates in from 0 width to full width on focus, 220ms cubic ease-out |
| Placeholder style | Italic, `inputBarPlaceholder` color | **Regular (non-italic)**, `inputBarPlaceholder @ α=0.55` (lower opacity than v2) |
| Prompt glyph | `chevron.right` 11pt semibold | `chevron.right` 11pt **regular** (lighter weight, less visual weight) |
| Prompt tint | `inputBarPrompt` / `inputBarPromptFocus` | `inputBarPrompt @ α=0.65` / `inputBarPromptFocus` on focus (muted resting, full color focus) |
| Top separator | 1px hairline `@α=0.4` | 1px hairline `@α=0.25` (even more subtle) |
| Inset shadow on focus | None | **Subtle 1pt inset shadow** at top edge of container (`NSColor.black @ α=0.08`) — simulates depth |
| Height animation | 120ms ease-out | 180ms cubic-bezier(0.25, 0.1, 0.25, 1) |
| Focus animation | 150ms ease-out | 220ms cubic-bezier(0.25, 0.1, 0.25, 1) |
| Caret | System default | System default (unchanged — Q1 preserved) |
| Corner radius | 8pt | 8pt (unchanged) |
| Container bg | Transparent | Transparent |
| `verticalPadding`, `minBarHeight` | Same as v2 | Same as v2 |

### Visual description

At rest, the bar looks like v2 but with an EVEN MORE subtle top separator (you almost can't see it) and a lighter, non-italic placeholder. The prompt glyph is muted. No border. When the user clicks in:

1. Border fades in as a 2px-tall underline at the bottom of the container, spanning from 4pt-inset left to 4pt-inset right, in indigo accent color. Animates from 0 width (centered) outward to full width over 220ms.
2. Prompt glyph tint shifts from muted to full accent.
3. A barely-visible 1pt inset shadow appears at the top of the container — adds a hint of depth without feeling heavy.
4. Placeholder text fades out as user types (standard NSTextView behavior).

### Estimated LOC delta

**+25 / -15** logic; replaces `applyContainerStyle` with an underline-based focus indicator. Adds a new `focusUnderlineView: NSView` subview with its own animated width constraint. Keeps `containerView` as the layout root.

### Trade-offs vs v2

| Factor | v2 | B | Δ |
|---|---|---|---|
| Vertical real estate | 48pt | 48pt | **=** |
| Visual element count | 4 | 5 (adds focus underline) | **+1** |
| Complexity | Low | Medium | **+** (new constraint animations) |
| Modern feel | Minimal | **Refined** | **++** (Material Design / iOS text field idiom) |
| Simple feel | Minimal | Slightly less (one more element) | **-** (subjective) |
| Focus affordance | Subtle border color | **Clearer** (underline grows) | **++** |

### Risk to #240 functionality

**Medium.** The new underline view needs to be added as a sibling of `containerView` or as a subview positioned at the bottom edge. Must NOT interfere with `scrollView` or `textView` hit testing. The animated width constraint runs in parallel with the existing focus color animation. IME marked-text position is NOT affected since the text container inset and scroll view are unchanged.

**Specific risks:**
- New `focusUnderlineView` must have `autoresizingMask` or constraints that keep it anchored to `containerView.bottomAnchor` regardless of dynamic height changes
- Animation timing must not stall when the bar height also animates simultaneously (multi-line expansion)
- Inset shadow implementation via `CALayer.shadow*` is for DROP shadows, not inset shadows — inset requires either a CAShapeLayer mask trick or a separate overlay view

### Which user feedback does it address?

Addresses **"modern"** strongly (underline focus + refined placeholder + smoother curves = contemporary Apple/Material idiom). Partially addresses **"simple"** (reduces border prominence, lighter prompt) but adds 1 new visual element.

---

## Direction C — Minimalist ultra-clean (RECOMMENDED)

**Thesis:** v2 still has structural "chrome" (1px border, visible corner radius, separator hairline). v3 should strip MORE, leaving only the essential signals: the text, the caret, and a single visual cue for focus state.

### What changes from v2

| Attribute | v2 | C |
|---|---|---|
| Container border | 1px, color change on focus | **REMOVED** — no border ever |
| Container corner radius | 8pt | **0pt** — flat rectangle (no rounding) |
| Top separator | 1px hairline `@α=0.4` | 1px hairline `@α=0.5` — **kept**, slightly more visible since it's now the ONLY visual divider |
| Container background | Transparent | Transparent |
| Prompt glyph | `chevron.right` 11pt semibold | **`chevron.right` 10pt regular**, `inputBarPrompt @ α=0.4` resting / full `inputBarPromptFocus` on focus |
| Focus indicator | Border color change | **Prompt glyph color change only** (muted → accent). Optional: glyph scales 1.0 → 1.1 via `CATransform3DMakeScale` on focus. |
| Placeholder style | Italic | **Regular (non-italic)**, `inputBarPlaceholder @ α=0.5` |
| `verticalPadding` | 9pt | **10pt** (slight breathing room bump) |
| `minBarHeight` | ~48pt | ~50pt |
| Shadow | None | None |
| Focus animation | 150ms ease-out | **200ms ease-out** (slightly slower for elegance) |
| Caret | System default | System default (unchanged) |

### Visual description

At rest: you see a slightly-more-visible hairline at the top of the bar area, and below it, placeholder text "Type a command..." in a very muted gray, with an even smaller and more muted chevron glyph to its left. **No border, no corner radius, no rectangle outline.** The bar is essentially just a strip of text-input-ready space, defined only by its position (bottom of window) and the hairline above it.

When focused: the prompt glyph subtly brightens from muted to full accent color (optional slight scale-up). The caret blinks. Nothing else changes visually. Typing fills in the text; the placeholder fades.

This is the MOST MINIMAL option — every element that could be removed has been removed, leaving only text and a single focus signal.

### Estimated LOC delta

**+5 / -25** — net reduction. `applyContainerStyle` simplifies drastically (no border, no cornerRadius, no masksToBounds). `updateFocusState` only tints the prompt glyph.

### Trade-offs vs v2

| Factor | v2 | C | Δ |
|---|---|---|---|
| Vertical real estate | 48pt | 50pt | +2pt |
| Visual element count | 4 | **2** (hairline + prompt+text content) | **-2** |
| Complexity | Low | **Lowest** | **--** |
| Modern feel | Minimal | **Ultra-minimal** | **++** (matches iOS 17+ / macOS Sonoma "less chrome" trend) |
| Simple feel | Minimal | **Maximum simplicity** | **++** |
| Affordance (is this a text field?) | Clear (border outlines it) | **Reduced** — relies on context (bottom of window + placeholder text + caret) | **-** |
| Discoverability for new users | Good | Lower (no explicit box) | **-** |

### Risk to #240 functionality

**Low.** Purely visual simplification. No layout math changes (`textContainerInset.height` formula untouched). No new views. Keyboard handling unchanged. IME unaffected. The prompt glyph scale-up animation (if included) is a CATransform3D on the NSImageView's layer — doesn't affect layout.

**Specific risks:**
- **Affordance risk**: without a visible border, users unfamiliar with the app may not realize the bar is an input field. Mitigation: placeholder text ("Type a command...") serves as the primary affordance.
- **Focus feedback risk**: only the prompt glyph color changes on focus — if user is looking at the text area, they may not notice. Mitigation: the caret blinks, which IS a focus indicator. Optional: add subtle bottom hairline color change on focus as a secondary signal.

### Which user feedback does it address?

Addresses **BOTH "simple" AND "modern"** — this is the strongest match for the verbatim user request. Simplification is extreme (2 visual elements vs v2's 4); modernity comes from matching the current Apple design trend of removing chrome (iOS 17 Dynamic Island, macOS Sonoma simplified window controls, SwiftUI default styles).

---

## Direction D — Modern glass / vibrancy blur

**Thesis:** Use `NSVisualEffectView` to create a native macOS glass/vibrancy effect on the bar. The terminal content behind the bar is visible through a subtle blur, making the input area feel like a floating pane of frosted glass.

### What changes from v2

| Attribute | v2 | D |
|---|---|---|
| Container root view | `containerView: NSView` | **`containerView: NSVisualEffectView`** |
| NSVisualEffectView material | N/A | **`.hudWindow`** or **`.sidebar`** (test both) |
| NSVisualEffectView blending mode | N/A | **`.behindWindow`** — samples terminal render output below |
| NSVisualEffectView state | N/A | **`.active`** always (not tied to window key state) |
| Container background | Transparent | Managed by `NSVisualEffectView` |
| Container border | 1px flat | **0px** — rely on blur boundary |
| Container corner radius | 8pt | **12pt** (slightly more rounded for glass feel) |
| `masksToBounds` | true | true (required for rounded corners on NSVisualEffectView) |
| Top separator | 1px hairline | **Kept** as subtle divider above the glass |
| Prompt glyph | `chevron.right` 11pt semibold | `chevron.right` 11pt semibold, `inputBarPrompt` color, **vibrant tinted** via `NSVisualEffectView` inheritance |
| Placeholder style | Italic | Italic, preserved |
| Caret | System default | System default |
| Focus animation | 150ms border color | **200ms NSVisualEffectView material swap** — toggle between `.sidebar` (resting) and `.hudWindow` (focused) for a subtle brightness shift |

### Visual description

A floating glass panel at the bottom of the window. The terminal content behind it is visibly blurred — you can see dim shapes of text and colors through the frosted glass effect. The placeholder text appears to "float" on the glass. When focused, the glass brightens slightly (material swap) and the prompt glyph tints to accent. Rounded corners (12pt) give it a "hovering pane" feel.

This is the most macOS-native-feeling option — it uses Apple's first-party vibrancy system, matches Control Center / Notification Center aesthetic.

### Estimated LOC delta

**+15 / -10** — replace `NSView` with `NSVisualEffectView`, configure material/blending/state, adjust constraints since `NSVisualEffectView` has slightly different layout behavior than plain `NSView`.

### Trade-offs vs v2

| Factor | v2 | D | Δ |
|---|---|---|---|
| Vertical real estate | 48pt | 48pt | = |
| Visual element count | 4 | 4 (separator, glass pane, prompt, text) | = |
| Complexity | Low | Medium-High | **+** |
| Modern feel | Minimal | **Native macOS glass** | **+++** |
| Simple feel | Minimal | Moderate (glass is visually busier than flat transparent) | **-** |
| Performance cost | ~0 | **Non-zero** — NSVisualEffectView GPU blur sampling | **-** |
| Metal renderer interaction | Clean (no layer compositing) | **Untested** — blur samples from the Metal-rendered terminal below | **risk** |

### Risk to #240 functionality

**Medium-High** due to Metal renderer interaction.

- **HARD RULE 17 check**: `NSVisualEffectView` is AppKit first-party, no external dep ✓
- **Metal compositing risk**: the terminal below uses a `CAMetalLayer` for GPU rendering. `NSVisualEffectView` with `.behindWindow` blending mode samples the window backing store below the view. When the terminal is rendered via Metal directly to a `CAMetalLayer`, the sampled pixels may be outdated or garbled depending on frame timing and display link synchronization. This can produce visual artifacts (stale frames, flicker, wrong colors).
- **Performance cost**: NSVisualEffectView blur is GPU-accelerated but adds per-frame cost. On a 120Hz display with dynamic terminal content, the blur sampling runs at the compositor frame rate. Tested cost on other aterm UI elements: ~0.3-0.8ms per frame, acceptable but non-zero.
- **Focus state via material swap**: NSVisualEffectView material changes trigger a full re-composition. 200ms material swap may visibly "pulse" at the boundary.
- **IME marked-text**: preedit rendering is done by NSTextView inside the scroll view, which is a subview of the NSVisualEffectView. Should work normally since the IME layer is above the blur.

**Verdict:** feasible but risky. Recommend prototyping in a throwaway branch before committing to v3.

### Which user feedback does it address?

Addresses **"modern"** very strongly (native macOS glass is the most contemporary Apple aesthetic). Does NOT address **"simple"** — glass is visually richer than flat transparent, not simpler.

---

## Recommended direction: **C (Minimalist ultra-clean)**

**Reasoning:**

1. The user's exact words were **"더 심플하고 더 모던하게"** ("simpler AND more modern"). Direction C is the ONLY option that scores high on BOTH axes simultaneously:
   - A: simpler (via breathing room) but NOT more modern
   - B: more modern (via accents) but adds 1 element so slightly LESS simple
   - C: **both more simple (2 elements vs 4) AND more modern** (matches Apple's "less chrome" trend)
   - D: more modern (native glass) but NOT simpler (visually busier)

2. v2 was already positioned as "minimal redesign". The user's rejection of v2 implies "your minimalism wasn't minimal enough". The logical next step is MORE minimalism, not lateral moves into accents or glass.

3. C has the LOWEST risk to #240 functionality — it's purely subtractive. B and D add new rendering components with integration risks.

4. C has the LOWEST LOC delta (net negative ~20 lines). Easier to review, easier to revert if rejected.

5. The stated affordance risk (no visible border) is real but mitigable — placeholder text "Type a command..." + caret blink + position at bottom of window provide enough context for discoverability.

**Recommendation:** go with C. If user wants more modern flair on top of C, a second iteration can add subtle accents from B (e.g., the bottom-underline focus indicator) without re-introducing the border or corner radius.

Alternative: if user wants a "glass" feel, D is the second choice — but prototype first to confirm Metal compositing doesn't glitch.

---

## Files to modify

| # | File | Change scope |
|---|---|---|
| 1 | `macos/Sources/OrchestratorInputBar.swift` | `setup()` container styling (remove `cornerRadius`, `masksToBounds`, `border*`), `applyContainerStyle` simplification (prompt-tint only on focus), placeholder attributes (non-italic, lower alpha), `verticalPadding` constant tweak, optional prompt-glyph scale animation on focus |

**NOT touching:** `OrchestratorCommands.swift` (dropdown styling already matches; no changes needed), `AtermTheme.swift` (no new tokens required — C uses only existing `inputBarBorder`/`inputBarPrompt`/`inputBarPromptFocus`/`inputBarPlaceholder`), `AppDelegate.swift` (bar install unchanged), `SessionSidebarView.swift` (unrelated).

---

## Verification (no app run required)

1. **`swiftc -typecheck`** on 6-file closure (OrchestratorInputBar, OrchestratorCommands, OrchestratorHistory, AtermTheme, AtermLocalization, GlyphAtlas) — must pass exit 0
2. **Self-diff review**: confirm no keyboard handler changes (`handleEnter/Escape/Arrow/Tab`, `OrchestratorTextView.keyDown`), no IME path changes, no scrollView or textView configuration changes beyond padding constants
3. **Geometry math sanity check**: recompute `textContainerInset.height = verticalPadding`, confirm prompt glyph `centerY = container.top + verticalPadding + lineHeight/2` still holds, confirm first-baseline-center is pixel-identical for all lineHeight values
4. **Preservation checklist**: all #240 features ticked off against the prior REPORT
5. **Tester handoff** (after rebuild): visual verification of the following states — unfocused empty, focused empty (caret centered), single-line typed, multi-line expanded, `/command` dropdown open, IME 한글 composition (ㅎ→하→한), click-out dismissal, Cmd+V paste, Ctrl+C/D/L PTY transmission

---

## Risks (preservation of #240 features)

| Feature | Risk | Mitigation |
|---|---|---|
| Cursor vertical centering (#240 Q2 root fix) | **None** — `textContainerInset.height = verticalPadding` formula preserved | N/A |
| System default caret (#240 Q1) | **None** — no `insertionPointColor` set | N/A |
| `chevron.right` small SF Symbol (#240 Q2) | Size reduces 11pt → 10pt, weight `.semibold` → `.regular`, alpha reduces | Still `chevron.right` SF Symbol, just rendered smaller/lighter |
| Transparent base + hairline (#240 Q3+Q5) | Hairline alpha 0.4 → 0.5 (slightly more visible to compensate for lost border) | Minor value tweak; still subtle |
| 3px thin scrollbar always visible (#240 Q4) | **None** — ThinScroller and scrollView config unchanged | N/A |
| Dynamic height animation (120ms) | **None** — recomputeHeight unchanged | N/A |
| Focus animation (150 → 200ms) | Slightly slower | Subjective; 200ms still feels responsive |
| History ↑/↓ | **None** — no keyDown changes | N/A |
| Ctrl+C/D/L PTY transmission | **None** — no keyDown changes | N/A |
| Shift+Enter multiline | **None** | N/A |
| Tab autocomplete | **None** | N/A |
| ESC dropdown toggle | **None** | N/A |
| Cmd+V paste | **None** — NSTextView default responder chain preserved | N/A |
| `/command` dropdown | **None** — CommandDropdown unchanged | N/A |
| Popover focus 3-layer defense | **None** — OrchestratorCommands.swift untouched | N/A |
| Font plumbing hot-reload | **None** — GlyphAtlas.resolvedFontFamily still read by makeStyledFont | N/A |
| IME marked-text alignment | **Low** — `textContainerInset.height` formula unchanged, only the constant multiplier shifts from 9 → 10pt | Visual verification by tester with 한글 typing |
| #246 English default placeholder | **None** — `AtermLocalization.text(ko:en:)` still used | N/A |

---

## Open questions (for user decision)

1. **Q-v3-1** — Which direction? A / B / C / D / hybrid (C + specific accent from B)?
2. **Q-v3-2** (if C chosen) — Should the hairline top separator also disappear, or stay as the sole visual element? I recommend STAY — it's the only anchor that tells users "below this line is input area".
3. **Q-v3-3** (if C chosen) — Should the prompt glyph have a subtle scale-up animation on focus (1.0 → 1.08 → 1.0 bounce), or just color change? Scale adds 5 lines of CATransform3D code.
4. **Q-v3-4** (if C chosen) — `verticalPadding` 9 → 10pt is conservative. Should we go 9 → 11pt or 9 → 12pt for more breathing room, accepting 4-6pt more bar height?
5. **Q-v3-5** (if B chosen) — Underline focus indicator width animation: grow from center outward (both directions) or from left-to-right (reading order)? Center-outward feels more "techy"; left-to-right feels more "material design".
6. **Q-v3-6** (if D chosen) — Prototype first in a throwaway branch to verify Metal compositing? This adds 1-2 hours to implementation time.
7. **Q-v3-7** — Should the recommendation apply to the Settings sidebar text field and other input fields in the app for consistency? Out of scope for this fix but could be a follow-up design system update.
8. **Q-v3-8** — If user wants "even more minimal than C", there's a Direction E: remove the hairline separator entirely, relying on the sidebar bg contrast alone. I did NOT propose this as a main direction because the separator is explicitly preserved per Q3+Q5 constraint, but it's available if user wants to relax that constraint.

---

## NO CODE CHANGES MADE. Awaiting `[IMPLEMENT APPROVED]` + direction choice from orchestrator.
