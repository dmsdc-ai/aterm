# SPEC: P0 default workspace + showInputBar fix

**Status:** Draft — awaiting user approval
**Priority:** P0 (user-facing regression, Rule 24 exemption authorized)
**Owner:** aigentry-aterm-claude
**User report (direct quote):** "aterm이 켜지면 default가 오케스트레이터가 아니야" — when aterm launches, default is NOT orchestrator
**Evidence:** Rebuild #88 PID 26192 screenshot — orchestrator row highlighted (manually clicked) + terminal shows `~` shell prompt (not claude) + input bar not visible
**Codex cross-check:** ref aee65ca3aa9fe6675 identified `AppDelegate.swift:1711-1744` area as root cause
**Prior work disclosed:** my earlier popover focus / regression fix refactor was unrelated (harmless per codex); the real root cause was never touched by me

---

## Two bugs identified

### Bug A — Default selection on launch is NOT the orchestrator (PRIMARY — confirmed)

**File:** `macos/Sources/AppDelegate.swift:703`
**Current code:**
```swift
for (i, se) in entries.enumerated() {
    ...
    createWorkspace(
        name: se.name,
        command: command,
        ...
        shouldSelect: i == entries.count - 1,  // ← BUG: only LAST entry is selected
        isSystem: se.isSystem,
        skipSave: true
    )
}
```

**Root cause:** the `shouldSelect` flag is `true` only for the last iteration (`i == entries.count - 1`). Whichever workspace is LAST in sessions.json becomes the auto-selected workspace on launch, regardless of `isSystem` status.

**Evidence — sessions.json ordering (sandbox):**
```
Index 0: orchestrator       (isSystem: true)   ← should be default
Index 1: aigentry-bridge    (isSystem: false)
Index 2: aigentry-builder   (isSystem: false)
Index 3: aigentry-context   (isSystem: false)
Index 4: aigentry-context 2 (isSystem: false)
Index 5: aigentry-hooks     (isSystem: false)
Index 6: aigentry-amplify   (isSystem: false)
Index 7: aigentry-aterm     (isSystem: false)
Index 8: alacritty          (isSystem: false)  ← currently selected (LAST)
```

At launch:
- `restoreWorkspaces` iterates 9 entries
- `createWorkspace(shouldSelect: i == 8)` only fires true on the last iteration (`alacritty`)
- `selectWorkspace(alacrittyUUID)` is called → `workspace.isSystem = false` → `showInputBar = false` → bar stays hidden
- Terminal area shows alacritty's shell (zsh — since `alacritty` is command `zsh`)

**Why the P0 regression report traced this incorrectly earlier:** my prior investigation audited the correct code path (line 703) but dismissed Bug A as "known behavior" because I was looking for what *changed* between Rebuild #86 and #87. The bug was present in #86 too — the user just hadn't reported it until #88 when they checked explicitly. This is a **long-standing latent bug**, not a #87 regression.

### Bug B — Selection doesn't activate claude CLI (SECONDARY — needs confirmation)

**Possible cause:** even when user manually clicks the orchestrator row, the terminal area shows `~` shell prompt instead of claude CLI. Two hypotheses:

**B1 (most likely):** `cliAvailable(for: .claude)` returns `false` at restore time because the app's PATH doesn't include claude's install directory. This triggers silent fallback to `.zsh`:

```swift
// AppDelegate.swift:693-694
let requestedCommand = WorkspaceLaunchCommand(rawValue: se.command) ?? .zsh
let command = cliAvailable(for: requestedCommand) ? requestedCommand : .zsh  // ← silent fallback
```

`cliAvailable(.claude)` calls `which("claude")` which runs:
```swift
// AppDelegate.swift:1096-1099
let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
process.arguments = ["-l", "-c", "command -v \(command) >/dev/null 2>&1"]
```

This depends on the user's login shell profile containing claude's PATH. When aterm is launched from Finder (not a terminal), the shell's `.zprofile`/`.zshenv`/`.zshrc` may or may not have claude in PATH depending on install method.

If `cliAvailable(.claude)` returns false, the orchestrator workspace is restored with `command = .zsh` and the user sees a shell prompt forever.

**B2 (less likely):** `selectWorkspace` fully activates orchestrator but the claude CLI hasn't started yet. The retry logic at `AppDelegate.swift:1760-1763` handles `!workspace.terminalView.didSpawnShell`, but "didSpawnShell" refers to the PTY process, not the claude CLI specifically. If the PTY was spawned as zsh (per B1), retrying doesn't launch claude.

**B3 (least likely):** selectWorkspace works but there's a race between terminal view switch and input bar constraint activation.

**Scope of this fix:** prioritize Bug A (confirmed + simple fix). Address Bug B only if the user confirms the `~` shell prompt issue persists after Bug A is fixed. **Recommendation:** fix A first, ask user to re-test, then diagnose B with live runtime evidence.

---

## Codex-identified code range audit

**`AppDelegate.swift:1711-1744`** per codex:

| Line | Code | Relevant to |
|---|---|---|
| 1715-1717 | `workspaceID(named:)` — lookup by name | Path from sidebar click to UUID |
| 1719-1765 | `selectWorkspace(_ id: UUID)` | Where `showInputBar = workspace.isSystem` is set |
| 1736-1737 | `let showInputBar = workspace.isSystem` + `orchestratorInputBar.isHidden = !showInputBar` | Bar visibility toggle — IS correct, fires on every selectWorkspace call |
| 1738-1739 | `terminalContainerBottomToWindow.isActive = !showInputBar` + `terminalContainerBottomToInputBar.isActive = showInputBar` | Terminal area constraint flip |
| 1751-1755 | `if showInputBar { orchestratorInputBar.focus() } else { window.makeFirstResponder(workspace.terminalView) }` | Focus routing |
| 1760-1763 | `if !workspace.terminalView.didSpawnShell { retrySpawnIfNeeded() }` | Retry PTY spawn if initial spawn failed |
| 1767-1770 | `selectWorkspace(named:)` wrapper | Lookup + delegate |

**Analysis:** this code is **correct for its current contract** — when `selectWorkspace(id)` is called with an isSystem workspace, it correctly shows the bar. The bug is UPSTREAM: nothing calls `selectWorkspace` with the orchestrator UUID at launch time. The selection happens on the last-iteration workspace (`alacritty`) instead.

---

## Fix approach

### Fix A — Prefer orchestrator as default at launch

**Change location:** `macos/Sources/AppDelegate.swift:680-707` (`restoreWorkspaces` loop)

**Minimal fix:** compute the `shouldSelectIndex` BEFORE the loop, preferring the first `isSystem` entry if any exists, falling back to the last entry otherwise.

```swift
// Determine which entry to auto-select at launch:
//   1. First isSystem entry (orchestrator) — takes priority per user expectation
//   2. Fallback: last entry (preserves old behavior when no orchestrator exists)
let shouldSelectIndex: Int = {
    if let systemIndex = entries.firstIndex(where: { $0.isSystem }) {
        return systemIndex
    }
    return max(0, entries.count - 1)
}()

// Now create workspaces from collected data (skipSave until the end)
for (i, se) in entries.enumerated() {
    let effectiveCwd: String
    ...
    createWorkspace(
        name: se.name,
        command: command,
        customCommand: se.customCommand,
        cliArgs: restoredCliArgs,
        cwd: effectiveCwd,
        shouldSelect: i == shouldSelectIndex,  // ← prefer isSystem, fall back to last
        isSystem: se.isSystem,
        skipSave: true
    )
}
```

**Why this works:**
- `entries.firstIndex(where: { $0.isSystem })` returns the first orchestrator entry (usually index 0 since system workspaces are saved first per line 1502 `workspaceOrder.insert(workspaceID, at: 0)`)
- `shouldSelectIndex` is computed once before the loop — no per-iteration overhead
- Fallback to `entries.count - 1` preserves backward compat when no orchestrator exists (e.g., first-install, user removed orchestrator)
- `max(0, ...)` guards against empty entries (shouldn't happen but defensive)

**Behavior change:**
| Scenario | Before fix | After fix |
|---|---|---|
| orchestrator exists, isSystem=true | last entry selected (wrong) | **orchestrator selected** |
| no orchestrator exists | last entry selected | last entry selected (unchanged) |
| empty sessions.json | early return from `restoreWorkspaces` | early return (unchanged) |
| multiple isSystem workspaces | last entry selected | **first isSystem entry selected** |

### Fix B (deferred) — `cliAvailable` PATH detection

**Recommendation:** do NOT fix B in this task. Confirm after Fix A is deployed:
1. User tests Rebuild #89 with Fix A
2. If orchestrator is auto-selected AND shows claude CLI → Bug B doesn't exist
3. If orchestrator is auto-selected BUT shows shell `~` → Bug B is real, diagnose separately with runtime evidence (which PATH, which shell config, what `which claude` returns in aterm's env)

**Why defer:** Bug B has multiple possible causes (B1/B2/B3). Static analysis alone can't distinguish them. Attempting to fix without evidence risks shotgun patches. Fix A is a clean, single-line change with a certain root cause.

**If user insists on addressing B in this fix:** the simplest mitigation is to make `cliAvailable(.claude)` accept `false` but still spawn the bootstrap as `claude` anyway (attempt the launch, let the shell fail if PATH is wrong, show error in terminal). This removes the silent fallback. But this changes semantics for all CLIs, not just claude — risk of breaking codex/gemini restore flows.

---

## Files to modify

| # | File | Change | LOC delta |
|---|---|---|---|
| 1 | `macos/Sources/AppDelegate.swift` | Add `shouldSelectIndex` computation before restore loop; change `shouldSelect: i == entries.count - 1` → `shouldSelect: i == shouldSelectIndex` | +7 / -1 |

**NOT touched:** `aterm-core/src/**` (Swift-only), `SettingsView.swift` (sub1 territory, no settings UI change needed since default is auto-detected), `OrchestratorInputBar.swift` (already correct — the bar wiring works when `selectWorkspace` fires), `OrchestratorCommands.swift` (popover focus fix preserved), `SessionSidebarView.swift`, `TerminalView.swift`, `GlyphAtlas.swift`.

---

## Invariants preserved

| Invariant | Path | Status |
|---|---|---|
| #240 cursor alignment | `OrchestratorInputBar.verticalPadding` formula | ✅ untouched |
| #246 English default locale | `AtermLocalization.text(ko:en:)` | ✅ untouched |
| Popover focus fix | `DropdownTableView.acceptsFirstResponder=false` + inline `configurePopoverWindowAfterShow()` | ✅ untouched |
| Settings contentShape (sub1) | `SettingsView.swift` | ✅ not touching sub1 file |
| Font plumbing hot-reload | `GlyphAtlas.setFontFamily` + `resolvedFontFamily` + `applySettingsToView` cascade | ✅ untouched |
| Existing workspace selection preferences | None exist today — default was implicitly "last entry"; new default is "first isSystem, fall back to last entry" | ✅ backward compat when no isSystem exists |
| `selectWorkspace(_ id:)` contract | Unchanged — still handles bar visibility, terminal constraint flip, PTY retry | ✅ |
| `workspaceOrder.insert(at: 0)` for isSystem | `AppDelegate.swift:1502` — system workspaces still inserted first in order | ✅ unchanged |
| User-initiated click on sidebar row | `handleWorkspaceClick` → `selectWorkspace(named:)` → `selectWorkspace(id)` | ✅ unchanged |

---

## Verification (no app run)

1. **`swiftc -typecheck`** on single file change (plus dependencies) — must pass exit 0
2. **Mental trace — launch with sessions.json containing orchestrator at index 0:**
   - `restoreWorkspaces` called
   - Collects 9 entries into `entries: [RestoredEntry]`
   - Computes `shouldSelectIndex = entries.firstIndex(where: { $0.isSystem })` → returns `0` (orchestrator)
   - Loop iterates i=0..8, only i=0 has `shouldSelect: true`
   - `createWorkspace(orchestrator..., shouldSelect: true)` → `selectWorkspace(orchestratorUUID)` at line 1512
   - `selectWorkspace(UUID)` → `workspace.isSystem = true` → `showInputBar = true` → `orchestratorInputBar.isHidden = false` → bar becomes visible
   - `terminalContainerBottomToInputBar.isActive = true` → terminal area shrinks to leave room for bar
   - `orchestratorInputBar.setCLI("claude")` → dropdown commands source set
   - `orchestratorInputBar.focus()` → `makeFirstResponder(textView)` → bar has focus, cursor blinks
   - Subsequent workspaces (i=1..8) have `shouldSelect: false` → no selectWorkspace call → they're created but hidden → normal pre-spawn behavior
3. **Mental trace — launch with no orchestrator in sessions.json (e.g., fresh install or user removed orchestrator):**
   - `entries.firstIndex(where: { $0.isSystem })` returns `nil`
   - Fallback: `shouldSelectIndex = max(0, entries.count - 1)` = last entry
   - Behavior identical to current code — backward compat preserved
4. **Mental trace — launch with empty sessions.json:**
   - `count == 0` → early return at line 637
   - No workspaces created, no selection, onboarding / default workspace path kicks in at `applicationDidFinishLaunching:241-246`
   - No regression
5. **Self-diff check:** confirm no other file touched, confirm `shouldSelectIndex` computation is pure (no side effects), confirm type of `shouldSelect: Bool` unchanged in `createWorkspace` signature
6. **Tester handoff (after Rebuild #89):** verify orchestrator is default, click other workspaces and click back to orchestrator, kill+restart aterm and verify orchestrator is re-auto-selected. If Bug B is real (shell `~` instead of claude), report back with evidence for separate fix.

---

## Questions for user decision

### Q1 — Should default always be orchestrator, or user-configurable?

**Recommendation:** default ALWAYS prefers `isSystem` (orchestrator) workspace. This matches the documented user expectation per the direct quote.

**Alternative:** add a `AtermSettings.defaultWorkspaceName: String?` config field. If set, prefer that workspace on launch. If unset, fall back to "first isSystem". This adds UI surface in SettingsView (not in scope for this P0 fix).

**For P0:** implement the simple version (always prefer isSystem). Defer configurable default to a follow-up feature request.

### Q2 — If orchestrator PTY hasn't spawned claude, should aterm auto-launch or wait?

**Current behavior:** aterm spawns the PTY with the `bootstrapCommand` derived from `launchCommand` + `customCommand` + `cliArgs`. If `cliAvailable(.claude)` returned `false` at restore time, the command is silently swapped to `.zsh`. The user sees a shell prompt.

**Recommendation for P0 (Bug A only):** don't change auto-launch behavior. Fix A ensures orchestrator is selected; if the user reports Bug B after Fix A is deployed, we address it separately with runtime evidence.

**Alternative for Bug B:**
1. Option a — remove silent fallback; if `cliAvailable(.claude) == false`, log an error and spawn shell with a warning message
2. Option b — attempt claude spawn anyway (no `cliAvailable` check); let the shell fail if PATH is wrong; user sees error in terminal
3. Option c — prompt user with a modal: "claude CLI not found in PATH. Install claude or configure PATH?"

**For P0:** defer Bug B to separate task with user input on which option.

### Q3 — Is Bug B (PTY shell fallback) a regression or pre-existing?

**Analysis:** the `cliAvailable` check at line 694 has been in the code since the restore logic was written. It's not new to #87 or #88. If Bug B is real (user sees shell instead of claude), it's a PRE-EXISTING issue that's now surfaced because Bug A was blocking the user from even reaching orchestrator.

**Recommendation:** confirm Bug B status AFTER Fix A is deployed. Most likely pre-existing, not a regression.

---

## Risks

| # | Risk | Mitigation |
|---|---|---|
| 1 | User has multiple isSystem workspaces (e.g., legacy orchestrator + new one) — `firstIndex` picks first one, may not be the user's preferred default | Very unlikely — aterm only creates one orchestrator per install. If multiple exist, `workspaceOrder.insert(at: 0)` on line 1502 ensures the most-recently-inserted is first, which is usually the current orchestrator. Acceptable for P0. |
| 2 | User has NO orchestrator (uninstalled or first-install) — fix falls back to last entry (current behavior) | Backward compat preserved; no regression |
| 3 | User specifically wanted last-used to be default — new behavior overrides this | Mitigation: Q1's alternative (configurable default) addresses this in a follow-up. For P0, user's direct quote states orchestrator should be default. |
| 4 | Fix A applies to restore-time only. If user uses Cmd+W to close orchestrator during session then re-opens aterm, orchestrator is re-restored and re-selected. Expected behavior. | N/A |
| 5 | Fix A does NOT address Bug B (shell vs claude). User may still see `~` shell prompt after fix. | Documented in SPEC; tester confirms after Fix A; separate task if needed. |
| 6 | Race condition: `selectWorkspace` fires at line 1512 during workspace creation, before all other workspaces are created. The PTY spawn + attach logic may have ordering issues. | Pre-existing behavior — Fix A doesn't change WHICH workspace gets `shouldSelect`, only the index. Any existing race is unchanged. |
| 7 | If `entries` array contains orchestrator at multiple positions (e.g., malformed sessions.json with duplicate isSystem rows), `firstIndex` picks the first — acceptable |  N/A |

---

## Expected post-fix behavior (Rebuild #89)

1. **Fresh launch (aterm app opens from Dock/Finder):**
   - `applicationDidFinishLaunching` → `restoreWorkspaces`
   - All 9 workspaces from sessions.json are created
   - `shouldSelectIndex = 0` (orchestrator at index 0)
   - `selectWorkspace(orchestratorUUID)` fires → bar visible, terminal area shows orchestrator, focus on bar
2. **User clicks another sidebar row:**
   - `selectWorkspace(otherUUID)` → `showInputBar = false` → bar hidden → terminal area expands
3. **User clicks orchestrator row:**
   - `selectWorkspace(orchestratorUUID)` → `showInputBar = true` → bar visible again
4. **If user sees `~` shell prompt in orchestrator instead of claude CLI:**
   - Bug A is fixed (orchestrator IS selected and visible)
   - Bug B is real (claude CLI not launched) — report separately with runtime evidence: `env | grep PATH`, `which claude` from aterm's spawned shell, launch source (Finder/Terminal), shell config

---

## NO CODE CHANGES MADE. Awaiting `[IMPLEMENT APPROVED]` from orchestrator.
