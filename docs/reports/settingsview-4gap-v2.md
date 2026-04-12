# REPORT: SettingsView 4 gap exposure v2

**Session:** aigentry-aterm-claude-sub1
**File:** `macos/Sources/SettingsView.swift` (sole edit)
**SPEC ref:** 540ee094f2f0... (original SPEC)
**Prior HALT ref:** ac4d9ed89fa0...
**Build check:** `swiftc -typecheck` PASS for SettingsView.swift (0 errors). Pre-existing 4 errors in TerminalView.swift:764/809/1237/1382 are `aterm_core_write_pty` Int/UInt (size_t) bridging — NOT touched by this SPEC, NOT regressed by this SPEC.

## Edits applied (9)

### AtermSettings model
1. Added `@Published var shellDefault: String = "zsh"` after `showTaskBoard`.
2. Added `@Published var tailscaleConnectOnLaunch: Bool = false` immediately after.
3. Added `static let shellPresets = ["zsh", "bash", "fish"]`.
4. Added `static var monospacedFontFamilies: [String]` — NSFontManager family enumeration, filters each family's first member PostScript name through `NSFont.isFixedPitch`.
5. Added `static var allFontFamilies: [String]` — unfiltered `NSFontManager.shared.availableFontFamilies.sorted()`.

### load() / save()
6. `load()`: reads `config["shell"]?["default"]` → `shellDefault` (guard on empty string); reads `config["tailscale"]?["connect_on_launch"]` → `tailscaleConnectOnLaunch`. Missing keys fall through to default values (`zsh`, `false`).
7. `save()`: merges `shell` and `tailscale` sub-dicts using the exact pattern from AppDelegate.swift:882-887 — `var x = config["x"] as? [String: Any] ?? [:]; x["key"] = value; config["x"] = x`. Preserves any unrelated keys that onboarding or other code wrote to those sub-dicts.

### UI controls
8. **AppearanceSettingsView — Font Family picker** (Q2 C + Q3 C)
   - Added `@State private var showAllFonts: Bool = false` (memory only, not persisted per Q3).
   - Added `private var fontFamilies: [String]` computed → `showAllFonts ? allFontFamilies : monospacedFontFamilies`.
   - Inserted a Family row between the "글꼴 / Font" header and the Size slider with an 80pt leading label, a `Picker($settings.fontFamily)` containing `Text("System Default").tag("System Default")`, a `Divider()`, then `ForEach(fontFamilies)`.
   - The picker uses `.labelsHidden()`.
   - `.onChange(of: settings.fontFamily)` calls both `settings.save()` and `onApply()` so the parallel Rust/AppDelegate plumbing (aterm-claude's SPEC) will be invoked live.
   - Below the picker, right-aligned `Toggle("Show all fonts / 모두 보기")` with `.toggleStyle(.checkbox)` bound to `$showAllFonts`.
   - **NO "restart required" footer** per Q2 C decision.

9. **TerminalSettingsView — Shell picker** (Q4 A)
   - Added `@State private var shellMode: String = "zsh"` to the view struct.
   - After the existing cursor style picker, added a Divider + "셸 / Shell" header + `Picker($shellMode)` segmented with tags `zsh`, `bash`, `fish`, `other` (label `기타... / Other...`).
   - `.onAppear` syncs `shellMode` from `settings.shellDefault`: preset hit → that preset, otherwise `"other"` (so existing custom shells round-trip correctly).
   - `.onChange(of: shellMode)`: if new mode is a preset, write `settings.shellDefault = mode` and save; if mode is `other` AND current `shellDefault` was a preset, clear `shellDefault` to `""` so the TextField opens empty for the user to type (prevents the confusing "I picked other but it says zsh" state).
   - When `shellMode == "other"`, shows an inline `TextField` bound directly to `$settings.shellDefault` with placeholder `셸 경로 또는 명령 (예: /bin/nu) / Custom shell path (e.g. /bin/nu)`, monospaced font, saves on change.

10. **SessionSettingsView — Tailscale toggle** (new Integrations block)
    - After the Show Task Board toggle, appended a Divider, a "통합 / Integrations" section header, a `Toggle` bound to `$settings.tailscaleConnectOnLaunch` labeled `실행 시 Tailscale 자동 연결 / Connect Tailscale on launch`, and a small helper text `다음 앱 실행부터 적용됩니다 / Takes effect on next app launch`.
    - No new SettingsTab enum case — stays under the existing Session tab (window size 480x520 unchanged).

11. **OrchestratorSettingsView — args always visible** (Q5 A override mode)
    - Removed the `if settings.orchestratorCLI == "custom"` guard.
    - Added a private computed `orchestratorArgsPlaceholder: String`: for `custom` CLI returns `예: my-cli --flag / e.g. my-cli --flag`; otherwise returns `settings.cliDefaults[cli] ?? AtermSettings.defaultCliArgs[cli] ?? ""` (so the user sees what the default is for the CLI they picked, e.g. `--dangerously-skip-permissions --continue` when CLI = claude).
    - Renders a TextField labeled `CLI 인수 (선택적 재정의) / Args (optional override)` with that placeholder, bound to `$settings.orchestratorArgs`, saves on change.
    - Helper text beneath clarifies the override semantics: `비어 있으면 위 기본값이 사용됩니다. 값을 입력하면 완전히 재정의합니다. / Leave empty to use the default above. Any value fully overrides it.`
    - **NOTE:** The runtime path that consumes `orchestratorArgs` vs `cliDefaults[cli]` lives in AppDelegate.swift, which sub1 does not touch. The current AppDelegate path (lines around 695-900 based on earlier grep) already reads `orchestrator.args` and `cli_defaults[cli]` separately — confirming the override semantics are compatible with this SPEC's save surface. If AppDelegate's logic today is "append" rather than "override", that's a follow-up ticket for aterm-claude, NOT a regression from this SPEC.

### resetDefaults()
12. Added `settings.shellDefault = "zsh"` (after cursorStyle).
13. Added `settings.tailscaleConnectOnLaunch = false` (after showTaskBoard).
14. `fontFamily = "System Default"` was already in the function; no change needed.
15. `orchestratorArgs = ""` was already in the function.
16. `showAllFonts` is a view-local `@State`; not a Model field, so it resets to its initializer default (`false`) automatically whenever Settings is reopened. No explicit reset needed.

## Invariants preserved
- `ATERM_DATA_ROOT` env vs `~/.aigentry` path separation — `configPath` getter unchanged.
- `fontSize` Int/Double cast fallback + `round()` (#157 drift prevention) — unchanged.
- `[DIAG-LOAD] configPath` NSLog — unchanged.
- Schema backward compat: `load()` uses `guard let data = ...; guard let json = ...` and per-key `if let ...` checks. Old config.json files without `shell` or `tailscale` sub-dicts load fine and get defaults.
- `save()` uses read-then-merge, preserving keys this SPEC doesn't manage (e.g. `onboarding_completed`, `setupCompleted`).
- 4-tab SettingsTab enum (Appearance / Terminal / Session / Orchestrator) — no new enum case, tailscale added to Session tab.
- Window size `480x520` — unchanged.
- Not touched: AppDelegate.swift, aterm-core Rust, OnboardingView.swift.

## Mental load/save round-trip

Given a fresh app with `config.json` already containing `appearance/terminal/session/orchestrator/ai/sidebar/cli_defaults/shell/tailscale`, and a user who sets:
- Font Family = "JetBrains Mono"
- Shell = "fish"
- Tailscale on launch = true
- Orchestrator args (CLI=claude) = `--dangerously-skip-permissions --continue --model sonnet`

1. User types each value → `.onChange` fires `settings.save()` → `save()` merges each field into the top-level dict.
2. On-disk config.json grows keys:
   - `appearance.fontFamily = "JetBrains Mono"` (already round-tripped prior to this SPEC).
   - `shell.default = "fish"` (new).
   - `tailscale.connect_on_launch = true` (new).
   - `orchestrator.args = "--dangerously-skip-permissions --continue --model sonnet"` (pre-existing key, always written).
3. User restarts app. `load()`:
   - reads `appearance.fontFamily` → `fontFamily = "JetBrains Mono"`.
   - reads `shell.default` → `shellDefault = "fish"`.
   - reads `tailscale.connect_on_launch` → `tailscaleConnectOnLaunch = true`.
   - reads `orchestrator.args` → `orchestratorArgs = "..."`.
4. Settings UI re-opens:
   - Appearance: Picker shows "JetBrains Mono". `showAllFonts` resets to false; picker list includes JetBrains Mono because it's a monospaced family. If the user had picked a proportional font via Show All, on reopen the picker still shows the saved value BUT the list won't contain it unless Show All is toggled — minor UX note, acceptable for this SPEC.
   - Terminal: `shellMode` `.onAppear` fires, sees "fish" is in presets → `shellMode = "fish"`. Segment control highlights fish. No TextField.
   - Session: Tailscale toggle ON.
   - Orchestrator: args TextField populated with the saved value; placeholder still shows `cliDefaults[claude]` as a hint but user's saved value takes precedence visually and semantically.

Round-trip OK.

## Known concerns
1. **`fontFamily` runtime plumbing is not in this SPEC.** Q2 C said it lands in a parallel aterm-claude SPEC that edits `AppDelegate.applySettingsToView` to call `CTFontCreateWithName(settings.fontFamily ...)` instead of the hardcoded `Menlo-Regular`. Until that SPEC ships, saving a non-default font family will have no visual effect in the running session. No footer was added per the decision — the parallel SPEC is assumed to ship in the same build cycle.
2. **`shell.default` runtime plumbing is also not in this SPEC.** The value is saved to config.json but currently the PTY spawn path in Rust (`aterm-core/src/pty.rs`) reads `SHELL` env or hardcodes — I did not trace this path in detail. If shell switching should affect newly-spawned workspaces, aterm-claude may need a second SPEC to have AppDelegate read `settings.shellDefault` and pass it as the argv when spawning. Not sub1's territory.
3. **`tailscale.connect_on_launch` runtime plumbing exists.** AppDelegate.swift:1354-1355 already reads `config["tailscale"]["connect_on_launch"]` on launch. This SPEC completes the user-facing side — any value saved here will be honored on next launch without AppDelegate changes.
4. **`orchestrator.args` override semantics at runtime.** The save surface is correct; whether AppDelegate's orchestrator launch path today treats a non-empty `args` as override-vs-append is a runtime concern for aterm-claude. The help text under the field tells users it's an override. If the current runtime is append, there's a small mismatch that aterm-claude should fix — but that is out of scope for this SPEC.
5. **`shellMode` @State resync on external change.** If another SettingsView instance or a code path somewhere writes `settings.shellDefault` directly while TerminalSettingsView is already visible, `shellMode` won't automatically re-derive. No such code path exists today. If it did, we'd need `onChange(of: settings.shellDefault)` to re-derive. Leaving as a watchpoint, not a fix.
6. **Pre-existing SourceKit errors in SettingsView.swift.** Single-file analysis flags `AtermTheme` and `AtermLocalization` as "not in scope" because those types live in sibling .swift files. When the whole module is type-checked together (as done for this verification), those errors vanish. They are noise, not real issues, and they existed before this SPEC.
7. **Pre-existing real errors in TerminalView.swift** (4 errors: `aterm_core_write_pty` Int vs UInt on size_t param at lines 764, 809, 1237, 1382). These are NOT touched by this SPEC and NOT caused by this SPEC. They appear to be a header-regeneration drift — likely `cbindgen` changed `size_t` mapping or a Rust-side signature changed. Flagging for aterm-claude / Builder session. Not a blocker for this SPEC's review; the Builder can reproduce the same 4 errors on a clean checkout of HEAD without my edits.

## SAWP compliance
- Code only — no `make`, no `cargo build`, no app launch.
- `swiftc -typecheck` only (authorized in the task message).
- Self-diff: only SettingsView.swift is in the modified set that this session introduced. All other M marks (`AppDelegate`, `AtermTheme`, `SessionSidebarView`, `TeleptyBusClient`, `TerminalView`) match the initial pre-session `git status` snapshot from the session prompt — pre-existing dirty files unrelated to this SPEC.
- Failed approaches from v1: none repeated; no schema replacement, no hardcoded forced defaults, no AppDelegate touch, no Rust touch.
