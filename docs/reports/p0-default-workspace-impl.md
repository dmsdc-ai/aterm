# REPORT: P0 default workspace fix implementation

**Date:** 2026-04-12
**Priority:** P0
**Owner:** aigentry-aterm-claude
**SPEC ref:** `a0619e111924eb7f9fbfcf6e866aee01366c688d10783ae17c5276fa01aedf19`
**Approval:** `[IMPLEMENT APPROVED]` with Q1/Q2/Q3 decisions (always orchestrator, defer Bug B, Bug B pre-existing)

---

## Status: COMPLETE

- **Code:** 1 file edited (`macos/Sources/AppDelegate.swift`)
- **LOC delta:** +8 / -1 (added `shouldSelectIndex` computation + comment, changed 1 line in `createWorkspace` call)
- **swiftc -typecheck:** AppDelegate.swift **PASS** (zero errors in AppDelegate); full-project fails due to pre-existing SettingsView.swift macOS 14+ API issue (sub1 territory, unrelated)

---

## Change

**File:** `macos/Sources/AppDelegate.swift`

**Before (line 703):**
```swift
createWorkspace(
    name: se.name,
    command: command,
    ...
    shouldSelect: i == entries.count - 1,  // always picks LAST entry
    ...
)
```

**After (+7 lines before loop, 1-line change inside loop):**
```swift
// Determine which entry to auto-select at launch (P0 fix):
//   1. First isSystem entry (orchestrator) — takes priority per direct user
//      quote "default가 orchestrator여야 함". Orchestrator is expected to be
//      first in workspaceOrder (per insert(at: 0) at createWorkspace:1502)
//      but may be any index in the restore entries array.
//   2. Fallback: last entry (preserves prior behavior when no orchestrator
//      exists, e.g. first-install or user removed orchestrator).
let shouldSelectIndex = entries.firstIndex(where: { $0.isSystem })
    ?? max(0, entries.count - 1)

for (i, se) in entries.enumerated() {
    ...
    createWorkspace(
        ...
        shouldSelect: i == shouldSelectIndex,
        ...
    )
}
```

---

## Behavior verification (mental trace)

### Scenario 1: sessions.json has orchestrator at index 0 + 8 non-system workspaces (tester's sandbox state)
- `entries.firstIndex(where: { $0.isSystem })` → `0`
- `shouldSelectIndex = 0`
- Loop i=0 → `shouldSelect: true` for orchestrator → `selectWorkspace(orchestratorUUID)` fires
- Loop i=1..8 → `shouldSelect: false` for all others
- `workspace.isSystem = true` → `showInputBar = true` → `orchestratorInputBar.isHidden = false` → bar visible
- Terminal shrinks to make room for bar

### Scenario 2: no orchestrator in sessions.json (user removed it, first-install after cleanup)
- `entries.firstIndex(where: { $0.isSystem })` → `nil`
- `max(0, entries.count - 1)` → last index
- Fallback: last entry selected (identical to prior behavior)
- Backward compat preserved

### Scenario 3: empty sessions.json
- `restoreWorkspaces` early-returns at `count == 0` check (line 637) before reaching the loop
- `createDefaultWorkspace` or `showOnboarding` takes over (AppDelegate.swift:241-246)
- No regression

### Scenario 4: multiple isSystem workspaces
- `firstIndex` returns the first one found (usually the most recently inserted per `workspaceOrder.insert(at: 0)` logic)
- First orchestrator wins default selection
- Acceptable for P0 — multi-orchestrator is an edge case

---

## Verification log

**AppDelegate-specific typecheck result:** PASS
- Grepped `error:` from swiftc output: all errors are in `macos/Sources/SettingsView.swift`
- Zero errors in `macos/Sources/AppDelegate.swift`
- My change (`shouldSelectIndex` computation + `i == shouldSelectIndex` comparison) is type-correct:
  - `entries.firstIndex(where: { $0.isSystem })` returns `Int?`
  - `max(0, entries.count - 1)` returns `Int`
  - `??` nil-coalescing gives `Int`
  - `i == shouldSelectIndex` compares two `Int` values
  - `Bool` result passed to `shouldSelect: Bool` parameter

**Full-project typecheck:** FAIL (pre-existing, not caused by P0 fix)
- `macos/Sources/SettingsView.swift:932,948,1150` — `.onChange(of:initial:_:)` is only available in macOS 14.0 or newer
- Sub1 used the newer SwiftUI API without `#available(macOS 14.0, *)` guard
- Deployment target is `arm64-apple-macos13` which requires the older `.onChange(of:_:)` signature
- This is in **sub1's SettingsView.swift** territory — not my file to fix
- **Blocks full-project build until sub1 resolves.** Builder will see the same errors; needs sub1 fix first OR deployment target bump to macOS 14.

---

## Self-diff check (confirm only AppDelegate touched)

```bash
# Expected: only 1 file modified
git status --short macos/Sources/
# Shows: M macos/Sources/AppDelegate.swift (plus other pre-existing modifications)
```

My P0 edit is surgically scoped to lines 680-707 area. No other files touched by this change. The `shouldSelectIndex` local variable is pure computation, no side effects.

---

## Preserved invariants

| Invariant | Status |
|---|---|
| #240 cursor alignment | ✅ unchanged |
| #246 English default | ✅ unchanged |
| Popover focus 3-layer defense | ✅ `OrchestratorCommands.swift` + `OrchestratorInputBar.swift` untouched |
| Settings contentShape (sub1) | ✅ `SettingsView.swift` untouched (separate pre-existing API issue there) |
| Font plumbing hot-reload | ✅ `GlyphAtlas.swift` + `TerminalView.swift` untouched |
| `workspaceOrder.insert(at: 0)` for isSystem | ✅ line 1502 unchanged — system workspaces still inserted first |
| `selectWorkspace(_ id: UUID)` contract | ✅ unchanged — still handles bar visibility, terminal constraint flip, PTY retry |
| `handleWorkspaceClick` → `selectWorkspace(named:)` user-click path | ✅ unchanged |
| `cliAvailable` check at line 694 | ✅ unchanged — Bug B behavior preserved (pre-existing, deferred) |
| `createDefaultWorkspace` / `showOnboarding` fallback paths | ✅ unchanged |

---

## Bug B (claude shell fallback) — confirmed deferred

Per Q3 approval: Bug B is **pre-existing** (predates #87), NOT a regression of recent work. Fix A is deployed first; user re-tests Rebuild #89:
- If orchestrator is auto-selected AND shows claude CLI → Bug B doesn't exist, task complete
- If orchestrator is auto-selected BUT shows `~` shell prompt → Bug B is real, diagnose separately with runtime evidence (shell PATH, `which claude` output, launch source)

---

## Known concerns

1. **SettingsView.swift pre-existing compile errors block builder.** Sub1's `.onChange(of:_:)` calls use the macOS 14+ API but deployment target is macOS 13. Options for resolution (not in scope for this P0 fix):
   - Sub1 adds `#available(macOS 14.0, *)` guards
   - Sub1 uses the older `.onChange(of: value) { newValue in ... }` closure signature with parameter
   - Project-wide deployment target bump to macOS 14 (breaking for macOS 13 users)
2. **Builder must resolve #1 before P0 fix can be runtime-verified.** My fix is type-correct; it just can't pass full-project typecheck while sub1 issue is unresolved.
3. **Bug B still hypothesized.** Tester verification of Rebuild #89 will confirm or deny. If confirmed, needs separate fix with runtime evidence.
4. **Multi-orchestrator edge case:** if user has TWO isSystem workspaces (rare), `firstIndex` picks the first one in the restore array. Acceptable for P0 — multi-orchestrator is not in the documented user workflow.

---

## Files list

```
macos/Sources/AppDelegate.swift  [M]  +8/-1 LOC — shouldSelectIndex computation + comparison change
```

No other files touched. No Rust changes. No tests run. No app run.
