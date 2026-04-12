# REPORT: #246 i18n — AtermLocalization English default

**Session:** aigentry-aterm-claude-sub1
**File:** `macos/Sources/AtermLocalization.swift` (sole edit)
**SPEC ref:** f4a71c2204f3... (audit report, i18n-audit-246.md)
**Decision:** B — English default + Korean preserved + i18n infrastructure intact
**Build check:** `swiftc -typecheck` PASS — zero errors introduced. Same 4 pre-existing `TerminalView.swift` size_t errors at 764/809/1237/1382 unchanged and not regressed.

## Patch

One Edit call in `AtermLocalization.swift`. Before (lines 3-16):

```swift
enum AtermLocalization {
    static var languageCode: String {
        let preferred = Locale.preferredLanguages.first?.lowercased() ?? ""
        return preferred.hasPrefix("ko") ? "ko" : "en"
    }

    static var isKorean: Bool {
        languageCode == "ko"
    }

    static func text(ko: String, en: String) -> String {
        isKorean ? ko : en
    }
}
```

After:

```swift
enum AtermLocalization {
    /// Task #246: forced English default. Korean string literals at call sites
    /// are preserved for future multi-lang revival — re-wire this getter to
    /// read OS locale or a user preference key when that returns.
    static var languageCode: String { "en" }

    static var isKorean: Bool { false }

    static func text(ko: String, en: String) -> String { en }
}
```

### Net diff

| Metric | Value |
|---|---|
| Lines removed | 9 (the OS-locale-detection getter body, isKorean computed body, text() ternary body — reduced to single-expression getters) |
| Lines added | 7 (3-line comment + 3 single-expression getters + blank line kept) |
| Net LOC delta | −2 lines |
| Behavior change | `languageCode` always returns `"en"`; `isKorean` always returns `false`; `text(ko:en:)` unconditionally returns its `en:` argument |
| API surface | `languageCode`, `isKorean`, `text(ko:en:)` signatures all preserved |
| Korean string literals at call sites | unchanged — 69 `ko:` arguments still compile and still live in source |

## Invariants preserved

- **Decision B compliance.** English is now the forced default. Korean string literals in 4 files (SettingsView, SessionSidebarView, OrchestratorInputBar, AppDelegate) are still in source, still reach the compiler, still pass through `text(ko:en:)`. The `en:` argument wins unconditionally.
- **API signatures unchanged.** Every existing `AtermLocalization.text(ko: ..., en: ...)` call site continues to compile without edit. Every reader of `AtermLocalization.languageCode` or `AtermLocalization.isKorean` continues to compile (grep confirmed zero external callers at audit time, but the getters remain as guardrails).
- **Korean strings NOT deleted.** All 69 Korean literals audited remain verbatim in their source files. No file other than `AtermLocalization.swift` was touched.
- **i18n infrastructure intact.** The `AtermLocalization` enum, its three members, and the contract that "pass two languages and get one back" are unchanged. Reviving multi-lang in the future is a 3-line revert of this patch plus whatever config/OS wiring the future design chooses.
- **`isKorean` / `languageCode` getters kept per Q3 default.** They now return constants (`false` / `"en"`) but still exist as computed properties. Any future caller that grabs them will get the new semantics without a compile break.
- **No unrelated edits.** The Foundation import at line 1 is untouched. The enum declaration is untouched except for the three member bodies.

## Files not touched (invariants)

- `SettingsView.swift` — 64 Hangul sites, 100% already wrapped, zero edit (my owned file, intentionally untouched per the audit conclusion).
- `SessionSidebarView.swift` — 3 Hangul sites, aterm-claude territory, untouched.
- `OrchestratorInputBar.swift` — 1 Hangul site, aterm-claude territory, also in active P0 regression-fix work per the task message — explicitly avoided, untouched.
- `AppDelegate.swift` — 1 Hangul site, aterm-claude territory, untouched.
- `TerminalView.swift` — 1 Hangul site (code comment at line 522, developer's choice per Decision B), untouched.
- `OnboardingView.swift` — 43 hardcoded English elements with zero Korean counterparts, flagged as out-of-scope follow-up (see §Follow-up below).
- `aterm-core/*.rs` — Rust side, out of scope for #246 per Q4.

## SAWP compliance

- Code only — no `make`, no `cargo build`, no app launch.
- `swiftc -typecheck` only, full-module run with `-sdk $(xcrun --sdk macosx --show-sdk-path) -target arm64-apple-macos13.0 -I aterm-core -import-objc-header aterm-core/aterm_core.h`.
- Zero compile errors introduced. The full error breakdown:
  - `SettingsView.swift`: 0 errors
  - `AtermLocalization.swift`: 0 errors
  - `TerminalView.swift`: 4 pre-existing `aterm_core_write_pty` Int/UInt size_t bridging errors at lines 764, 809, 1237, 1382 — unchanged and not regressed by this patch.
- Self-diff: the `git status --short macos/Sources/` modified set is unchanged in shape; no new files added. All session-introduced diff is inside `AtermLocalization.swift`.
- No conflict with aterm-claude's P0 regression work on OrchestratorInputBar — that file is not touched.
- 3-attempt stuck counter: not invoked — first attempt compiled clean.

## Mental round-trip verification

1. **App launches on a Korean macOS** (`Locale.preferredLanguages.first == "ko-KR"`). Before the patch: `languageCode` read preferred locale → `"ko"` → `isKorean` was `true` → every `text(ko: A, en: B)` returned `A` → UI displayed Korean. After the patch: `languageCode` returns `"en"` regardless of `Locale` → `isKorean` is `false` → `text(ko: A, en: B)` returns `B` → UI displays English. Confirmed by code reading.
2. **App launches on an English macOS.** Before: same `en` result. After: same `en` result. No behavior change for English users.
3. **Settings window.** Tab bar reads `AtermLocalization.text(ko: "모양", en: "Appearance")` → returns `"Appearance"` → tab title renders in English. Same for all 64 Settings sites.
4. **Session sidebar task board.** Line 910 reads `AtermLocalization.text(ko: "태스크 (\(active) 진행 중 / \(total) 전체)", en: "TASKS (\(active) active / \(total) total)")` → string interpolation happens in both arms at compile time (normal Swift), the `en:` argument is selected at runtime → header renders `TASKS (3 active / 10 total)` etc. No interpolation bug.
5. **Orchestrator input placeholder.** `textView.placeholderText = text(ko: "명령어 입력...", en: "Type a command...")` → returns `"Type a command..."` → NSTextView shows the English placeholder.
6. **Preferences window title.** `prefsWindow.title = text(ko: "설정", en: "Settings")` → returns `"Settings"` → the macOS window title bar reads `Settings`.
7. **Reverting this patch in the future.** A single Edit restoring the 3 original getter bodies (or a new implementation that reads OS locale / a config key) flips everything back. No other file needs touching.

Round-trip OK across all call sites.

## Follow-up task #247 candidate — FLAGGED

`macos/Sources/OnboardingView.swift` has **43 hardcoded English UI strings** (`Text(...)`, `Button(...)`, placeholder strings) with **zero `AtermLocalization.text` wrapping and zero Korean counterparts**. Under Decision B (English forced default), every one of those elements displays correctly and no user-visible regression exists in #246.

However, the file is **not prepared for future multi-lang revival**. If/when Korean or any other locale is re-enabled, OnboardingView will remain stuck in English while the rest of the app responds to the language setting. Closing that gap requires:

- Wrapping all 43 UI strings in `AtermLocalization.text(ko: "...", en: "...")` calls.
- Authoring the Korean translations for each string (first-class translation work, not mechanical).
- Estimated effort: ~90 LOC of mechanical wrapping + ~43 translation strings. Owner: whoever is comfortable with both SwiftUI and Korean UX copy.

**Recommendation for orchestrator:** register this as task #247, dependent on a future decision to revive multi-lang. No blocker for #246 — this flag is purely prospective.

## Known concerns

1. **Korean literals will drift over time.** With the English path being the only one exercised in UI testing, Korean strings may go stale (misspellings, outdated wording, new features shipping with only `en:` populated). This is the explicit tradeoff of Decision B — accepted and documented.
2. **`isKorean` / `languageCode` callsite revival.** The audit confirmed zero current external callers, but I did not add a compile-time assertion to prevent future callers from grabbing the constant values and treating them as runtime-variable. Low risk; mitigation is grep-on-future-PR.
3. **`Locale` import / `Foundation` import.** The `Foundation` import at line 1 of AtermLocalization.swift is still present even though the new body no longer references `Locale`. Kept to avoid a cascading-import churn if some other part of the enum (or future additions) needs Foundation. Zero compile impact.
4. **Comment accuracy over time.** The inline comment references "Task #246" as the context — if the decision is ever reverted, that comment becomes stale. Alternative is a bare code block without the comment. I chose to keep the comment because Decision B is non-obvious to a reader reverting blindly, and the 3-line explanation makes the intent durable.
5. **No runtime flag for quick revert.** A future maintainer cannot flip to Korean at runtime without recompiling. Designing such a flag is explicitly out of scope for #246 v1 per Decision B.
6. **Pre-existing TerminalView.swift size_t errors.** Still unresolved and still flagged for aterm-claude / Builder. Not touched, not caused, not regressed.
