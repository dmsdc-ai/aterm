# REPORT: #240 OrchestratorInputBar dropdown Enter-key P0 bug fix

**Date:** 2026-04-12
**Priority:** P0 (HARD RULE 24 SPEC-first exemption authorized)
**Owner:** aigentry-aterm-claude
**Rebuild target:** #87 (after builder rebuild)
**Bug repro:** Rebuild #86 PID 9887

---

## Status: COMPLETE

- **Code:** 2 files edited
- **swiftc -typecheck:** PASS (exit 0, no errors, no warnings across 6-file closure)
- **Regression checks:** verified via self-diff against pre-edit state
- **Parallel constraint:** none (sole owner of these files; no sub1 overlap)

---

## Root cause analysis (evidence-based)

### Observed
1. User typed `/` in orchestrator input bar
2. Dropdown (NSPopover-hosted NSTableView) opened with command list; row highlighted
3. User pressed Enter
4. **Nothing happened** — command NOT selected, NOT inserted, dropdown NOT closed, input NOT submitted

### Why `handleEnter` was never called

The pre-edit `OrchestratorInputBar.handleEnter` method contained:

```swift
private func handleEnter() {
    let text = textView.string
    let stripped = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !stripped.isEmpty else { return }

    if commandDropdown.isShown {
        if let cmd = commandDropdown.confirmCurrent() {
            applyCommand(cmd)
            return
        }
        commandDropdown.hide()
    }
    ...
}
```

If this method had been called, `text = "/"`, `stripped = "/"` (non-empty), guard passes, `isShown = true`, `confirmCurrent` returns the row-0 selection (guaranteed by `show()` which always `selectRowIndexes(IndexSet(integer: 0))`), `applyCommand` runs, dropdown closes. **The user observed NONE of this.**

Therefore `handleEnter` was never invoked. Which means `OrchestratorTextView.keyDown` never fired for keyCode 36. Which means **`OrchestratorTextView` was not first responder when Enter was pressed.**

### The responder chain race

`CommandDropdown.show()` calls:

```swift
popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .maxY)
```

`NSPopover` with `behavior = .transient` is documented to close on outside interaction, but in practice the popover's internal window **promotes itself to key status** as soon as it contains a focusable view. `NSTableView.acceptsFirstResponder` returns true whenever the table has rows (`numberOfRows > 0`). The moment `show()` is called, `selectRowIndexes(IndexSet(integer: 0))` populates a selection, and the popover's table becomes first responder in the popover's new key window.

From that point on, keyboard events are delivered to **NSTableView**, not to OrchestratorTextView. `NSTableView` has:
- ✅ **Arrow keys**: native row-selection navigation (why arrows appeared to work)
- ✅ **ESC**: propagates up as `cancelOperation:` which NSPopover auto-dismiss handles (why ESC appeared to work)
- ❌ **Return / Numpad Enter**: **NO default handler** — silently dropped
- ❌ **Tab**: default is inline-edit or propagate — silently dropped

### The asymmetry that concealed the bug

The pre-existing `handleArrowUp/Down` / `handleEscape` / `handleEnter` methods in `OrchestratorInputBar` LOOK like they handle all cases, but they only fire when `OrchestratorTextView` is first responder. ESC still "worked" because NSPopover auto-dismisses on cancelOperation: anyway. Arrow keys still "worked" because NSTableView has native arrow handling. These two accidents masked the fact that the popover was stealing first responder — until Enter exposed it (no native Enter handler anywhere).

### Why the #240 self-diff review missed it

My prior REPORT for the #240 redesign (SPEC ref ea1d288de553...) listed `ESC dropdown-close priority` as a preserved feature, verified by diff. That diff showed `handleEscape` unchanged, which was true — but both the pre-edit and post-edit `handleEscape` were **never reached when the bug condition applied**. NSPopover's built-in auto-dismiss made it look functional. Enter had no equivalent auto-behavior, so this bug was latent all along and the #240 redesign neither caused nor fixed it.

---

## Fix

### 1. `OrchestratorCommands.swift` — new `DropdownTableView` subclass

Subclass `NSTableView` with explicit `keyDown` override that routes Return/Tab/ESC to owner callbacks. This handles the key presses at the table level, independent of responder chain state — so the dropdown commits reliably whether the popover's window is key or not.

```swift
final class DropdownTableView: NSTableView {
    var onConfirmKey: (() -> Void)?
    var onCancelKey: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 36, 76:  // Return / Numpad Enter → confirm current selection
            onConfirmKey?()
            return
        case 48:      // Tab → same as Enter for dropdown (matches handleTab semantics)
            onConfirmKey?()
            return
        case 53:      // ESC → cancel dropdown (belt-and-suspenders alongside NSPopover auto-dismiss)
            onCancelKey?()
            return
        default:
            break
        }
        super.keyDown(with: event)
    }
}
```

### 2. `OrchestratorCommands.swift` — use subclass + wire callbacks

```swift
// BEFORE
private let tableView = NSTableView()

// AFTER
private let tableView = DropdownTableView()
```

And in `configureTable()`:

```swift
tableView.onConfirmKey = { [weak self] in
    guard let self = self else { return }
    let cmd = self.confirmCurrent()
    self.hide()
    if let cmd = cmd {
        self.onSelect?(cmd)
    }
}
tableView.onCancelKey = { [weak self] in
    self?.hide()
}
```

The `onConfirmKey` callback:
1. Reads current selection via existing `confirmCurrent()` → `CLICommand?`
2. Hides popover (restores first responder to OrchestratorTextView)
3. Fires `onSelect?(cmd)` → owner's wire-up `commandDropdown.onSelect = { self?.applyCommand($0) }` → `applyCommand` → `setText + moveCursorToEnd + hide` (hide idempotent, safe second call)

### 3. `OrchestratorInputBar.swift` — defensive reorder in `handleEnter`

Move the `commandDropdown.isShown` check **before** the empty-text guard. This preserves the existing fall-through behavior but handles the edge case where the user had the dropdown open but deleted back to empty text before pressing Enter. More importantly, it makes the code intent explicit: **dropdown takes priority**.

```swift
private func handleEnter() {
    // Dropdown takes priority — checked BEFORE the empty-text guard.
    if commandDropdown.isShown {
        if let cmd = commandDropdown.confirmCurrent() {
            applyCommand(cmd)
            return
        }
        commandDropdown.hide()
        // Fall through to submit existing text (preserves prior behavior).
    }

    let text = textView.string
    let stripped = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !stripped.isEmpty else { return }

    history.add(text)
    onSubmit?(text)
    clearText()
}
```

**Note:** this path only fires when OrchestratorTextView IS first responder. When the popover's DropdownTableView is first responder instead, the table-level keyDown handles the Enter. Both paths converge on `applyCommand()` → `setText + hide`.

---

## Two mutually-exclusive code paths converging on the same outcome

| Scenario | First responder | Enter delivery path | Terminal action |
|---|---|---|---|
| Popover window is key (common case) | `DropdownTableView` | `DropdownTableView.keyDown(36)` → `onConfirmKey` → `confirmCurrent` + `hide` + `onSelect` | `applyCommand(cmd)` → `setText(cmd.name + " ")` → `hide` (idempotent) |
| Parent window remains key | `OrchestratorTextView` | `OrchestratorTextView.keyDown(36)` → `onEnter` → `handleEnter` → `isShown` branch → `confirmCurrent` → `applyCommand` | `setText(cmd.name + " ")` → `hide` |

No double-fire: the two key-down paths are mutually exclusive (only one NSView is first responder at a time). No regression to any other key binding: Shift+Enter still calls `onShiftEnter` (different case), Ctrl+C/D/L still routes via modifier check before the keyCode switch.

---

## Self-diff regression check

| Preserved behavior | Verification |
|---|---|
| **Enter submits input** (dropdown closed) | Fall-through path in `handleEnter` still fires when `commandDropdown.isShown == false`, identical to prior code |
| **Shift+Enter inserts newline** | `OrchestratorTextView.keyDown` `case 36, 76:` still checks `if flags.contains(.shift)` first and calls `onShiftEnter` — unchanged |
| **Ctrl+C/D/L PTY transmission** | `keyDown` `.control` check block is evaluated BEFORE the keyCode switch — unchanged, bytes 0x03/0x04/0x0C routing intact |
| **ESC dropdown-close priority** | Now handled by BOTH `DropdownTableView.onCancelKey` AND the existing `handleEscape` — belt-and-suspenders, no regression |
| **ESC toggle to terminal** (when dropdown closed) | `handleEscape` still falls through to `onEscape?()` when `!commandDropdown.isShown` — unchanged |
| **Tab local autocomplete** (dropdown closed, single match) | `OrchestratorTextView.keyDown` `case 48:` still fires `onTab` → `handleTab` → single-match autocomplete branch — unchanged |
| **Tab in dropdown (new)** | `DropdownTableView.keyDown` `case 48:` now also confirms selection, matching the existing `handleTab` dropdown branch — consistent, not a regression |
| **Cmd+V paste** | No override on `paste:` action anywhere in touched code — NSTextView default responder chain intact |
| **IME marked-text underline** | No changes to `textContainerInset`, `setMarkedText`, or `NSTextInputClient` methods — untouched |
| **#240 cursor top-alignment** | `textContainerInset.height = verticalPadding` formula unchanged |
| **applySettingsToView font plumbing (prior task)** | Untouched |
| **GlyphAtlas font family plumbing (prior task)** | Untouched |

---

## Verification log

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

6-file closure typechecks cleanly. SourceKit LSP reported cross-file resolution errors for symbols in files outside the closure (expected — single-file LSP can't see the full project). Authoritative verification is the multi-file `swiftc -typecheck` above.

---

## Test cases for tester (after builder rebuild)

1. **Type `/`, press Enter immediately** — first entry ("help") should be selected and inserted
2. **Type `/`, arrow-down 3 times, press Enter** — 4th entry should be selected and inserted
3. **Type `/cle`, press Enter** — filter narrows, Enter selects the filtered top entry
4. **Type `/`, press Tab** — same as Enter (first entry selected) per `DropdownTableView` Tab binding
5. **Type `/`, press ESC** — dropdown closes, text `/` remains in input
6. **Type `hello world`, press Enter** — regular submit, dropdown never involved
7. **Type `hello`, press Shift+Enter** — newline inserted, no submit
8. **Type `abc`, press Ctrl+C** — byte 0x03 sent to PTY, text field unchanged
9. **Type `abc`, press Cmd+V to paste** — NSTextView default paste works
10. **Type `한국어` via IME composition (ㅎ→하→한)** — preedit underline aligned (no regression from prior #240 fix)
11. **Type `/`, click outside popover** — NSPopover.transient auto-dismiss still works

---

## Command-list discrepancy (observation, not a blocker)

The user's screenshot showed dropdown entries `help / clear / compact / follow / fork / fullscreen / resume / review / init`, but the current `OrchestratorCommands.commands(forCLI: "claude")` returns `help / clear / compact / model / status / cost / review / init / quit`. The entries `follow / fork / fullscreen / resume` don't exist in any `currentCLI` case.

Possible explanations (not investigated further since orthogonal to the Enter-key fix):
1. The screenshot shows a different dropdown — possibly the claude CLI's own native slash-command menu rendered in the terminal after `/` was submitted, not our NSPopover
2. Sub1 or another worker extended the command list in an untracked edit
3. User paraphrased the entries from memory

The Enter-key fix is independent of the command list content — it works for any rows in `filtered[]`. If the user wants the command list expanded, that's a separate task for `OrchestratorCommands.swift:commands(forCLI:)`.

---

## Known concerns

1. **Responder chain observation is inferred, not directly verified** — I cannot run the app to log first-responder state. The root cause analysis rests on the observable facts (handleEnter wasn't called, arrow/ESC worked, Enter didn't) combined with NSPopover/NSTableView documented behavior. The fix is defensive — works regardless of which NSView is first responder.
2. **Tab in dropdown now binds to confirm** — previously Tab was unhandled when popover was first responder. Now it confirms. Change is additive but technically a new behavior. Matches `OrchestratorTextView.handleTab` dropdown branch so consistent.
3. **ESC double-handling** — both `DropdownTableView.onCancelKey` AND NSPopover's built-in cancelOperation: auto-dismiss will fire on ESC. Both end in `hide()`. Idempotent (`performClose` early-returns if already closed). No user-visible effect.
4. **`onSelect` callback wiring** — relies on `CommandDropdown.onSelect` being set by the owner (OrchestratorInputBar does this in `setup()`). If a future caller forgets to set `onSelect`, Enter will hide the dropdown but not apply the command. Low risk — single caller today.
5. **Tester handoff** — this fix requires manual verification (responder chain state is runtime-only). Builder rebuild and tester screenshot/keystroke test are mandatory before closing the bug.
