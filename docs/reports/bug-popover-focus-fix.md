# REPORT: NSPopover focus theft fix (broader root cause)

**Date:** 2026-04-12
**Priority:** P0 continuation
**Owner:** aigentry-aterm-claude
**Rebuild target:** #88 (after builder rebuild)
**Bug repro:** Rebuild #86 PID 9887 — filter-as-you-type, Tab autocomplete broken
**Prior fix:** Enter-only fix (ref `b61d58f61fdc...`) — insufficient; only rescued Return
**Tester verification:** ref `6d418e022061...`
**User decision:** Option A (canBecomeKey=false approach, not child-view rewrite)

---

## Status: COMPLETE

- **Code:** 2 files edited (both already in the dropdown Enter fix — this is an extension)
- **swiftc -typecheck:** PASS (exit 0, 6-file closure)
- **Prior Enter fix preserved:** `DropdownTableView.keyDown` override retained as defensive belt-and-suspenders
- **Parallel constraint:** none

---

## Broader root cause (what the Enter-only fix missed)

My prior fix (`b61d58f61fdc...`) correctly identified that `NSPopover` with `.transient` behavior steals first responder from `OrchestratorTextView` by promoting the popover's own window to key. My solution was to handle Return at the `NSTableView` subclass level, so Enter worked regardless of which view was first responder.

**What I missed:** the same focus theft also breaks EVERY OTHER keyboard interaction:
- **Character keys** (filter-as-you-type) — typing `/cle` sends `c`, `l`, `e` to `NSTableView.keyDown`, which ignores arbitrary characters → text view never sees them → dropdown never refilters
- **Tab autocomplete** — Tab goes to `NSTableView` which has no autocomplete logic → silently dropped
- **Arrow keys** — worked by coincidence via `NSTableView.keyDown` native row-navigation (not via `OrchestratorInputBar.handleArrowUp/Down`) → user couldn't tell the difference

Tester's Rebuild #86 PID 9887 verification surfaced the broader failure: typing `/` opens the dropdown, but typing `cle` after it does nothing visible in the input bar or the filter.

**The real fix needs to prevent the focus theft entirely, not just patch around individual key cases.**

---

## Why NSTableView becomes first responder

`NSTableView.acceptsFirstResponder` returns `true` when `numberOfRows > 0`. As soon as `CommandDropdown.show()` populates `filtered[]` and calls `tableView.reloadData()` + `selectRowIndexes(IndexSet(integer: 0))`, the table has a row count > 0 and accepts first responder. AppKit then:

1. Creates `NSPopover`'s internal window (`_NSPopoverWindow`, an `NSPanel` subclass)
2. Calls `makeFirstResponder(tableView)` on that window — succeeds because the table accepts
3. Calls `makeKey()` on the popover window — becomes key because the window has a valid first responder
4. Keyboard events now flow to `tableView.keyDown`

The ONLY key events that still worked on `OrchestratorTextView` before this fix were ones where the popover had not yet finished this promotion dance. By the time user types characters after `/`, the popover is fully key.

---

## Fix (two-layered defense, Option A)

### Layer 1 — refuse first responder at the table level

```swift
final class DropdownTableView: NSTableView {
    /// Refuse first responder — PRIMARY focus-theft fix.
    override var acceptsFirstResponder: Bool { false }

    // ... keyDown override retained as defensive belt-and-suspenders ...
}
```

With `acceptsFirstResponder = false`, AppKit cannot make the table first responder. The popover window has no first responder candidate inside its content (assuming the container NSView also doesn't accept first responder — which it doesn't by default for a plain NSView). The popover window either:
- Stays non-key (if `becomesKeyOnlyIfNeeded = true`)
- Becomes key with nil first responder (and keyboard events fall through the responder chain)

Either way, `OrchestratorTextView` in the parent window is no longer displaced.

### Layer 2 — primary defense: `popoverDidShow` restores parent window as key

```swift
private weak var anchorWindow: NSWindow?  // captured in show()

private func configurePopover() {
    popover.behavior = .transient
    popover.animates = false
    NotificationCenter.default.addObserver(
        self,
        selector: #selector(popoverDidShow(_:)),
        name: NSPopover.didShowNotification,
        object: popover
    )
}

@objc private func popoverDidShow(_ note: Notification) {
    if let popoverWindow = popover.contentViewController?.view.window {
        // NSPopover's internal window is typically an NSPanel subclass.
        // becomesKeyOnlyIfNeeded is declared on NSPanel (not NSWindow).
        if let panel = popoverWindow as? NSPanel {
            panel.becomesKeyOnlyIfNeeded = true
        }
    }
    // PRIMARY: explicitly restore key status to the parent (main) window.
    // Even if the popover window was briefly promoted to key by AppKit
    // during show(), this forces focus back so OrchestratorTextView
    // continues receiving keyboard events.
    anchorWindow?.makeKey()
}

func show(anchor: NSView, prefix: String) {
    ...
    anchorWindow = anchor.window  // captured BEFORE popover.show()
    ...
    popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .maxY)
}
```

**Three-step defense:**
1. `DropdownTableView.acceptsFirstResponder = false` — AppKit can't promote the table to first responder
2. Cast popover window to `NSPanel` and set `becomesKeyOnlyIfNeeded = true` — if cast succeeds, the panel only becomes key when a contained view needs keyboard input, which nothing does
3. `anchorWindow?.makeKey()` — explicit restore of parent as key window, overriding any transient promotion AppKit may have done during the show transition

### Defensive hide on OrchestratorTextView unfocus

```swift
textView.onFocusChange = { [weak self] focused in
    self?.updateFocusState(focused)
    if !focused {
        self?.commandDropdown.hide()
    }
}
```

The popover is now non-key, which means `.transient` auto-dismiss (which hinges on focus-change detection by the popover) may not fire reliably. Adding an explicit `hide()` on text-view unfocus prevents stale dropdowns from lingering when the user tabs or clicks away from the input bar.

---

## Post-fix keyboard flow (mental verification, not runtime)

| Action | Previous (broken) | After fix |
|---|---|---|
| User types `/` | Reaches text view → `textDidChange` → `maybeShowCommandDropdown` → `popover.show()` → popover steals focus | Same up to `popover.show()`, but `popoverDidShow` immediately restores parent key → text view stays focused |
| User types `c` after `/` | Key event goes to `NSTableView.keyDown` → default handler drops character → text view never sees it → filter doesn't update | Key event goes to `OrchestratorTextView.keyDown` → default NSTextView handling inserts `c` → `textDidChange` fires → `maybeShowCommandDropdown("/c")` → `CommandDropdown.show(prefix:"/c")` → `filter()` narrows to commands starting with `/c` → table reloads |
| User types `le` | Same silent drop | Same refilter flow, narrows to `/clear` |
| User presses ↓ | NSTableView native arrow handling (worked by coincidence) | `OrchestratorTextView.keyDown` case 125 → `onArrowDown` → `handleArrowDown` → `commandDropdown.selectNext()` → `tableView.selectRowIndexes` (manual) |
| User presses ↑ | Same NSTableView native | `handleArrowUp` → `commandDropdown.selectPrevious()` |
| User presses Enter | Prior fix: `DropdownTableView.keyDown` case 36 → `onConfirmKey` → `confirmCurrent` + `hide` + `onSelect` | `OrchestratorTextView.keyDown` case 36 → `onEnter` → `handleEnter` → dropdown priority branch → `confirmCurrent` → `applyCommand` → `setText + hide`. Prior `DropdownTableView.keyDown` is defensive dead code. |
| User presses Tab | NSTableView default drops Tab | `handleTab` → dropdown-open branch → `applyCommand(currentSelection)` |
| User presses ESC | NSPopover cancelOperation: dismissal (by coincidence) | `handleEscape` → `commandDropdown.hide()` → dropdown closes |
| User clicks outside input bar | `.transient` auto-dismiss | Text view resigns first responder → `onFocusChange(false)` → explicit `commandDropdown.hide()` (defensive) |
| User clicks on a dropdown row | NSTableView target-action `tableClicked(_:)` | Same — mouse events don't require first responder status |

---

## Files modified

| # | File | LOC delta | Purpose |
|---|---|---|---|
| 1 | `macos/Sources/OrchestratorCommands.swift` | +42 | `DropdownTableView.acceptsFirstResponder = false`; `popoverDidShow` observer + `NSPanel` cast + parent `makeKey`; `anchorWindow` weak capture; `deinit` observer removal |
| 2 | `macos/Sources/OrchestratorInputBar.swift` | +6 | Defensive `commandDropdown.hide()` on text view unfocus |

**Net change from prior state (after Enter fix):** only the `DropdownTableView.acceptsFirstResponder` override, the `popoverDidShow` observer, and the `anchorWindow` weak ref are genuinely new. The rest (keyDown override, onConfirmKey wiring, deinit) was already in place from the prior Enter fix.

---

## swiftc -typecheck verification

```bash
swiftc -typecheck \
  -sdk $(xcrun --show-sdk-path --sdk macosx) \
  -target arm64-apple-macos13 \
  macos/Sources/OrchestratorCommands.swift \
  macos/Sources/OrchestratorInputBar.swift \
  macos/Sources/OrchestratorHistory.swift \
  macos/Sources/AtermTheme.swift \
  macos/Sources/AtermLocalization.swift \
  macos/Sources/GlyphAtlas.swift
# EXIT: 0  (no errors, no warnings)
```

**Note on first attempt failure:** my first edit used `popoverWindow.becomesKeyOnlyIfNeeded` directly, which fails because `becomesKeyOnlyIfNeeded` is declared on `NSPanel`, not `NSWindow`. Fixed by adding `if let panel = popoverWindow as? NSPanel { panel.becomesKeyOnlyIfNeeded = true }`. `NSPopover`'s internal window is typically `_NSPopoverWindow` (a private `NSPanel` subclass), so the cast succeeds at runtime. If it doesn't on some macOS version, the block is skipped silently and we rely on the primary defense (`anchorWindow?.makeKey()`).

**SourceKit LSP shows stale diagnostics** because it reads the file linearly and doesn't re-parse immediately — authoritative verification is `swiftc -typecheck` against the closed set of files, which passed.

---

## Regression checks (self-diff verified)

| Preserved behavior | Path |
|---|---|
| Enter submits input (dropdown closed) | `handleEnter` dropdown-priority branch short-circuits when `!isShown`; falls through to submit |
| Enter commits dropdown selection (NEW primary path) | `OrchestratorTextView.keyDown` case 36 → `onEnter` → `handleEnter` → `isShown` branch → `confirmCurrent` → `applyCommand` |
| Enter commits dropdown selection (defensive path) | `DropdownTableView.keyDown` case 36 → `onConfirmKey` — dead code after this fix but retained for macOS edge cases |
| Shift+Enter newline | `OrchestratorTextView.keyDown` case 36/76 with `.shift` → `onShiftEnter` — unchanged |
| Ctrl+C/D/L PTY bytes 0x03/0x04/0x0C | Modifier check before keyCode switch — unchanged |
| Tab autocomplete (single match) | `handleTab` single-match branch — unchanged; now actually reached because OrchestratorTextView stays first responder |
| Tab commits dropdown selection | `handleTab` dropdown-open branch — same semantics, now actually reached |
| ESC closes dropdown | `handleEscape` dropdown-priority branch — now actually reached |
| ESC toggles focus to terminal | `handleEscape` fall-through — unchanged |
| Cmd+V paste | NSTextView default responder chain — unchanged |
| IME marked-text (한글 composition) | `textContainerInset` and `NSTextInputClient` untouched |
| #240 cursor vertical centering | `verticalPadding`, `lineHeight`, `containerContentHeight` math untouched |
| Font plumbing (prior task) | Untouched |
| Filter-as-you-type (NEW — was broken) | Keyboard events flow to `OrchestratorTextView` → `textStorage` updates → `textDidChange` → `maybeShowCommandDropdown` → `filter(prefix:)` narrows list → `reloadData` |
| Click-to-select dropdown row | `tableView.action = #selector(tableClicked(_:))` — mouse events don't require first responder, works regardless |

---

## Known concerns

1. **Cast `as? NSPanel` may fail on future macOS** — NSPopover's internal window class is private (`_NSPopoverWindow`). If Apple changes it to a non-NSPanel subclass, the `becomesKeyOnlyIfNeeded` setting is silently skipped. The primary defense (`anchorWindow?.makeKey()`) still works in that case. If both fail, fallback is option B (child-view overlay rewrite).

2. **`popoverDidShow` timing** — `NSPopover.didShowNotification` fires after the popover is visible AND its window is configured. If the window becomes key before this notification fires (single-frame window), there may be a micro-flicker where the popover briefly holds focus. In practice, AppKit fires `didShowNotification` synchronously within the `popover.show()` call path, so the notification handler runs before any user key event can be processed. Not a practical issue.

3. **`.transient` auto-dismiss may behave oddly with non-key popover** — `.transient` closes on user interaction with UI outside the popover. With our parent-window makeKey defense, any user click in the main window will trigger `.transient` close. Typing characters (which go to OrchestratorTextView) may or may not count as "interaction outside the popover" — depends on AppKit internals. If `.transient` auto-closes on every keystroke, the dropdown would flicker off→on. Mitigation: defensive `hide()` on unfocus is already in place; if flicker occurs, switch to `.applicationDefined` behavior (we already manage hide manually via `maybeShowCommandDropdown`).

4. **Selection highlight cosmetic** — when the popover window is non-key, NSTableView may draw its selection with the "inactive" (gray) color instead of "active" (blue). This is cosmetic only. If user complains, force `tableView.selectionHighlightStyle = .sourceList` or override `isEmphasized`.

5. **`anchorWindow` weak ref** — captured in `show()`, used in `popoverDidShow`. If anchor's window is deallocated between show and didShow (impossible in practice), the restore is skipped and we rely on layer-1 (acceptsFirstResponder=false). Safe.

6. **Mouse click on dropdown row may dismiss prematurely** — clicking the row fires `tableClicked` → `hide` + `onSelect` → `applyCommand` → `setText + hide` (idempotent). Should be fine.

7. **Cannot run app for live verification** — responder-chain and key-window state are runtime concepts. All verification is static/mental + `swiftc -typecheck` + self-diff. Tester must exercise the manual test cases after builder rebuild #88.

---

## Tester test plan (post-rebuild)

Run Rebuild #88 on the orchestrator input bar. All tests must PASS:

1. **filter-as-you-type** — type `/`, then `cle`, observe dropdown narrows live to only `/clear` (this was the primary broken case)
2. **character input after dropdown open** — type `/`, dropdown opens, type `h`, `e`, `l`, `p` → filter narrows to `/help`
3. **Tab autocomplete single match** — type `/cle`, press Tab → text becomes `/clear ` (with trailing space), dropdown hides
4. **Enter commits dropdown selection** — type `/`, press Enter → first entry (`/help`) inserted, dropdown hides, cursor after inserted text
5. **Arrow key navigation** — type `/`, press ↓ twice, observe 3rd row highlighted, press Enter → 3rd command inserted
6. **ESC closes dropdown** — type `/`, press ESC → dropdown closes, text `/` remains in input
7. **Regular Enter submit** — type `hello world`, press Enter → text sent to PTY, text field cleared
8. **Shift+Enter newline** — type `foo`, press Shift+Enter, type `bar` → two-line input, no submit
9. **Ctrl+C while focused** — type `abc`, press Ctrl+C → 0x03 sent to PTY, text field unchanged
10. **Cmd+V paste** — paste into input bar → NSTextView default paste works
11. **IME 한글 composition** — type ㅎ→하→한 → preedit underline aligned (no regression from #240 cursor alignment fix)
12. **Click outside input bar** — dropdown dismisses, text view loses focus
13. **Click on dropdown row** — row selected, `applyCommand` fires, dropdown dismisses, focused restores to text view

**Failure modes to watch for:**
- Dropdown flickers open/closed on every keystroke → `.transient` is misbehaving with non-key popover → switch to `.applicationDefined`
- Selection highlight is gray instead of blue → cosmetic, report if intolerable
- Dropdown doesn't dismiss on outside click → defensive `hide()` on unfocus catches this; if it still lingers, add a `NSWindow.didResignKeyNotification` observer
