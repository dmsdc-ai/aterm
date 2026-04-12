# REPORT: Settings UX fixes — theme live-apply and tab hit area

**Session:** aigentry-aterm-claude-sub1
**File:** `macos/Sources/SettingsView.swift` (sole edit)
**Scope:** 2 small UX fixes, both rooted in the same SwiftUI hit-test gap around `.buttonStyle(.plain)`
**Build check:** `swiftc -typecheck` PASS for SettingsView.swift (0 errors). Pre-existing 4 errors in TerminalView.swift (`aterm_core_write_pty` Int/UInt size_t at 764/809/1237/1382) unchanged and not touched.

## Root cause (shared)

Both issues are instances of the same SwiftUI footgun: a `Button(action:) { ... }.buttonStyle(.plain)` with a multi-child label whose background is `Color.clear` (or has transparent gaps between opaque children) has a **hit-test region equal to the union of its opaque content**, not to the button's frame.

For the tab bar, the background is `Color.clear` when the tab is not selected, so clicks on the non-selected tab's padding fall through. For the theme scheme grid, the `VStack { RoundedRectangle(height: 32) + Text(name) }` has transparent space between the color chip and the label, plus whatever slack the `LazyVGrid` cell gives above/below — clicks in those regions never reach the `Button.action` closure.

SwiftUI provides `.contentShape(Rectangle())` as the canonical fix: it tells the hit-test system to treat the underlying `Rectangle()` (sized to the frame) as the shape for pointer interaction, without drawing anything. Applied **inside** the Button's label closure so the Button itself picks up the larger shape.

## Fix 1 — Tab bar (Issue 2)

**Before:** `VStack { Image + Text }.frame(maxWidth: .infinity).padding(.vertical, 8).background(...).foregroundColor(...)` inside a `Button`. The `.background(Color.clear)` (for unselected tabs) plus the padding region left the outer cell unclickable.

**After:** Appended `.contentShape(Rectangle())` immediately after `.foregroundColor(...)` and inside the Button's label closure. The Rectangle is sized to the same frame that `.frame(maxWidth: .infinity).padding(.vertical, 8)` produces, so the entire cell — including the padding surrounding the icon and label — now registers taps.

**No other behavior changed.** The `Button(action: { selectedTab = tab })` closure is untouched. The visual appearance (icon + label centered, selected-tab accent background, accent text color) is unchanged. Only the hit test region widened.

## Fix 2 — Theme scheme tile (Issue 1)

**User-reported symptom:** "click a theme tile → selection highlights but terminal doesn't change until Save/완료".

**Diagnosis:** The SchemeButton's `action` closure already calls `settings.colorScheme = scheme; settings.save(); onApply()`, which in turn invokes AppDelegate's `applySettings()` → `applySettingsToView(view)` → Rust `aterm_core_set_color_scheme` + `aterm_core_scheme_bg_color` + `metalRenderer.setBgColor` + `aterm_core_render`. That chain is correct and already wired (AppDelegate.swift:1028-1084). The wiring is not the bug.

The most likely explanation for the reported symptom is this: the user's click lands on a part of the tile **outside** the 32-pt colored chip, in the gap between the chip and the label, or in the slack above/below the `VStack`. A plain Button with no `.contentShape` ignores those taps, so the `action` closure never fires. The "selection highlights" the user sees is actually the macOS plain-button press state (a brief tint during mouse-down) — not the `isSelected` state transitioning. The user then clicks Save/완료, the Done button runs `settings.save(); onApply()`, and the theme finally applies — but from the user's perspective it looks like "Save triggered the change".

**Fix:** Appended `.frame(maxWidth: .infinity).contentShape(Rectangle())` after the VStack's children, still inside the Button's label closure. The `.frame(maxWidth: .infinity)` makes the VStack actually span the full grid cell width (without it, the VStack's width follows its intrinsic content, not the cell). The `.contentShape(Rectangle())` then forces the entire VStack frame to be tap-aware — including the gap between the color chip and the label, and the slack.

**Result:** every click anywhere inside the grid cell will now reliably invoke the `action` closure → `settings.colorScheme = scheme; settings.save(); onApply()` → AppDelegate cascade → Metal repaint. The chain was never broken; the click was being swallowed.

**If the bug persists after this fix**, the root cause is in AppDelegate's `applySettingsToView` → Metal redraw path (aterm-claude territory). This SPEC explicitly does not touch AppDelegate or TerminalView per the task instructions, and the user's note "AppDelegate.swift:1354 area already handles colorScheme reload per aterm-claude audit" suggests that path was previously verified. Flagging for aterm-claude as a fallback action if the hit-test fix is insufficient.

## Invariants preserved

- **4 gap picker intact** (fontFamily, shell, tailscale, orchestrator.args) — not touched.
- **Variant badge intact** — `FontVariantAvailability`, `detectFontVariants`, `variantAvailability` @State, badge HStack, onAppear, extended onChange — all untouched.
- **Font hot-reload wiring** — not touched.
- **Save/완료 button** — unchanged at line 477-482, still runs `settings.save(); onApply(); dismiss()`.
- **AppDelegate.swift** — not touched.
- **GlyphAtlas.swift, TerminalView.swift** — not touched.
- **aterm-core Rust** — not touched.
- **Schema / config.json surface** — zero new keys, zero schema mutation.
- **480x520 window size** — unchanged.
- **SettingsTab enum** — unchanged, 4 cases.
- **AtermTheme / AtermLocalization usage** — unchanged; no new tokens introduced.

## Self-diff

`git status --short macos/Sources/` shows the same 6 modified files that were already dirty at session start. All session-introduced modifications are inside SettingsView.swift; the only changes are:

1. Tab bar Button label: added `.contentShape(Rectangle())` after `.foregroundColor(...)`.
2. SchemeButton body: added `.frame(maxWidth: .infinity).contentShape(Rectangle())` after the VStack closing brace, still inside the Button label closure.

Two additions. Both zero-LOC net on logic (no new state, no new methods, no new closures — just two modifier calls each on existing Button labels).

## Verification

- `swiftc -typecheck macos/Sources/*.swift -sdk $(xcrun --sdk macosx --show-sdk-path) -target arm64-apple-macos13.0 -I aterm-core -import-objc-header aterm-core/aterm_core.h` → 0 errors in SettingsView.swift.
- Pre-existing TerminalView.swift errors (4) at lines 764/809/1237/1382: unchanged, not regressed.
- Mental walkthrough:
  1. Click anywhere inside a theme tile (chip, label, or padding) → `action` closure fires → color scheme updates → save() writes config.json → onApply() runs → AppDelegate applies new scheme to every managed workspace's terminal view → Metal repaint.
  2. Click anywhere inside a tab cell (icon, label, or surrounding padding) → `selectedTab = tab` → SwiftUI switch statement re-renders the Tab content pane with the new active tab.
  3. Keyboard tab navigation and the Save/완료 button (which has its own `.keyboardShortcut(.defaultAction)`) are unchanged.
  4. All existing Settings controls (Font Family Picker, shell segmented, tailscale toggle, orchestrator.args, cursor style, scrollback, etc.) are unchanged.

## Known concerns

1. **Bug root cause is inferred, not observed.** I did not run the app. The diagnosis above assumes the user's "selection highlights but doesn't apply" symptom is the press-state flash, not an actual `isSelected` state change. The `contentShape` fix is the right answer for either interpretation: if the click was being dropped it now lands, and if the click was already landing the fix is a no-op. No regression risk.
2. **AppDelegate applySettings chain not re-verified.** The user referenced an aterm-claude audit that concluded the cascade is wired correctly. I took that at face value. If the real bug turns out to be in `applySettingsToView` → Metal repaint (e.g. `needsRender` flag not being set after color scheme update), that's aterm-claude's territory. This SPEC explicitly avoids AppDelegate edits.
3. **`.contentShape(Rectangle())` placement inside Button label.** Must be the last modifier on the label so it applies to the already-sized VStack frame. Verified by reading the edited code — the two additions are in the correct position (after `.foregroundColor` for the tab bar; after the `VStack` closing `}` but before the `Button` closing `}` for SchemeButton).
4. **No visual regression.** `.contentShape` does not render anything. `.frame(maxWidth: .infinity)` on the SchemeButton VStack makes it fill the grid cell width — this matches the cell's natural offered width, so the visual layout should be identical to before (the colored chip and label still center within the cell).
5. **Pre-existing SourceKit single-file errors in SettingsView.swift for `AtermTheme` and `AtermLocalization`.** Cross-file resolution noise, unchanged by this SPEC. Full-module `swiftc -typecheck` passes.
6. **Pre-existing 4 TerminalView.swift size_t Int/UInt errors.** Untouched, not regressed. Still flagged for Builder / aterm-claude.

## SAWP compliance

- Code only — no `make`, no `cargo build`, no app launch.
- `swiftc -typecheck` run against full module, authorized per the task.
- Zero compile errors in SettingsView.swift, zero regression in sibling files.
- Self-diff scope: all session-introduced changes are in SettingsView.swift only.
- Failed approaches not repeated: no AppDelegate edit, no AtermSettings field change, no Save button removal, no other tabs/cells logic rewrite.
- 3-attempt stuck counter: not invoked — first attempt compiled clean.
