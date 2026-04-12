# TASK #246 — aterm English locale transition AUDIT

**Session:** aigentry-aterm-claude-sub1 (audit only — no code changes)
**Scope:** macos/Sources/*.swift — user-facing Korean strings that need to display English by default
**User decision:** B — English default + Korean preserved in AtermLocalization + i18n infrastructure kept
**Audit mode:** read-only

---

## TL;DR

**Every Korean string in aterm Swift source is already wrapped in `AtermLocalization.text(ko:en:)` with a valid English counterpart.** Zero hardcoded user-facing Korean exists. The only real work is **one three-line change to `AtermLocalization.swift`** to flip the default language from OS-locale detection to forced English.

- Total Hangul occurrences across the Swift tree: **70**
- In user-facing strings: **69** (1 is a code comment in TerminalView.swift:522, excluded)
- Already wrapped in `AtermLocalization.text(ko:en:)`: **69 / 69 (100%)**
- Hardcoded Korean requiring new L10n wrapping: **0**
- Files with at least one Korean-containing user-facing string: **4** (SettingsView, SessionSidebarView, OrchestratorInputBar, AppDelegate)
- Files with zero user-facing strings that need audit action: **all other 10 Swift files**

**Implementation for Decision B is a ~3-line edit to AtermLocalization.swift.** No per-file refactoring is needed for the existing Korean inventory. A separate (out-of-scope) consideration: `OnboardingView.swift` has 43 hardcoded English UI elements with zero Korean counterparts — they display correctly under Decision B, but they are not yet prepared for future Korean translation.

---

## 1. AtermLocalization.swift — current state

File: `macos/Sources/AtermLocalization.swift` (16 lines total)

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

### Current behavior
- Reads `Locale.preferredLanguages.first` (the first entry of the user's macOS language preference list).
- If that string starts with `ko` (e.g. `"ko-KR"`, `"ko"`, `"ko-Hant"`), returns `"ko"`.
- Otherwise returns `"en"`.
- `text(ko:en:)` picks the Korean string when `isKorean` is true, otherwise English.

### What Decision B requires
- Force `"en"` as the default regardless of `Locale.preferredLanguages`.
- Preserve the `text(ko:en:)` signature so existing call sites continue to compile.
- Preserve the Korean string literals at call sites.
- Preserve the `isKorean` getter (may be read elsewhere — worth checking, see §4.2).

### Proposed Decision-B shape (one of several equivalent patches)
```swift
enum AtermLocalization {
    /// Fixed to "en" as of task #246. Future multi-lang revives OS/user selection here.
    static var languageCode: String { "en" }

    static var isKorean: Bool { false }

    static func text(ko: String, en: String) -> String { en }
}
```

This is a **3-line net change** inside `AtermLocalization.swift`. Everything else in the codebase keeps compiling unchanged.

### Usage of `isKorean` elsewhere
I grepped for `isKorean` outside of AtermLocalization.swift and found **0 references**. Safe to stub to `false`.

### Usage of `languageCode` elsewhere
I grepped for `languageCode` outside of AtermLocalization.swift and found **0 references**. Safe to stub to `"en"`.

---

## 2. File-by-file inventory

Legend for the **Via L10n?** column: `yes` = the Korean string is inside an `AtermLocalization.text(ko:..., en:...)` call that already has the English counterpart; `no` = hardcoded Korean that would need new L10n wrapping.

### SettingsView.swift — 64 Hangul occurrences, 100% via L10n

Owned by **sub1**. This audit confirms every wrap has a valid `en:` sibling (multi-line calls verified by reading their 2-3 line vicinity).

| Line | Korean | English suggestion (already present) | Context | Via L10n? |
|---|---|---|---|---|
| 388 | 모양 | Appearance | SettingsTab title | yes |
| 389 | 터미널 | Terminal | SettingsTab title | yes |
| 390 | 세션 | Session | SettingsTab title | yes |
| 391 | 오케스트레이터 | Orchestrator | SettingsTab title | yes |
| 473 | 기본값으로 재설정 | Reset to Defaults | Bottom-bar button | yes |
| 478 | 완료 | Done | Bottom-bar button | yes |
| 534 | 없음: \(joined) | Missing: \(joined) | Variant badge message | yes |
| 541 | 색상 테마 | Color Scheme | Appearance section header | yes |
| 561 | 글꼴 | Font | Appearance section header | yes |
| 566 | 서체 | Family | Font Family row label | yes |
| 570 | 시스템 기본 | System Default | Font Family picker sentinel | yes |
| 604 | 모든 글꼴 표시 | Show all fonts | Appearance toggle | yes |
| 613 | 크기 | Size | Font size slider label | yes |
| 630 | 줄 높이 | Line Height | Slider label | yes |
| 649 | 불투명도 | Opacity | Slider label | yes |
| 667 | 상태 이모지 표시 | Show Status Emojis | Appearance toggle | yes |
| 671 | ASCII 아이콘 사용 | Use ASCII Icons | Appearance toggle | yes |
| 748 | 기본 CLI | Default CLI | Terminal section header | yes |
| 756 | 셸 | Shell | Default CLI picker "none" label | yes |
| 763 | CLI 기본 인수 | Default CLI Arguments | Terminal section header | yes |
| 768 | 새 워크스페이스 생성 시 자동 입력되는 CLI 인수 | Pre-filled when creating a new workspace | Subtitle | yes |
| 795 | 기본 작업 디렉터리 | Default Working Directory | Terminal section header | yes |
| 800 | 홈 (~) | Home (~) | Default CWD fallback label | yes |
| 806 | 선택... | Choose... | CWD picker button | yes |
| 821 | 스크롤백 줄 수 | Scrollback Lines | Row label | yes |
| 832 | 커서 스타일 | Cursor Style | Section header | yes |
| 837 | 블록 | Block | Cursor style tag | yes |
| 838 | 밑줄 | Underline | Cursor style tag | yes |
| 839 | 막대 | Bar | Cursor style tag | yes |
| 846 | 셸 | Shell | Shell picker section header | yes |
| 854 | 기타... | Other... | Shell segment tag | yes |
| 877 | 셸 경로 또는 명령 (예: /bin/nu) | Custom shell path (e.g. /bin/nu) | Shell TextField placeholder | yes |
| 898 | 세션 관리 | Session Management | Section header | yes |
| 902 | 실행 시 세션 자동 복원 | Auto-restore sessions on launch | Toggle | yes |
| 906 | 죽은 세션 자동 재시작 | Auto-restart dead sessions | Toggle | yes |
| 913 | 최대 재시작 횟수 | Max Restart Attempts | Row label | yes |
| 926 | 사이드바 | Sidebar | Section header | yes |
| 930 | 태스크 보드 표시 | Show Task Board | Toggle | yes |
| 936 | 통합 | Integrations | Section header | yes |
| 942 | 실행 시 Tailscale 자동 연결 | Connect Tailscale on launch | Toggle | yes |
| 951 | 다음 앱 실행부터 적용됩니다 | Takes effect on next app launch | Subtitle | yes |
| 977 | 예: my-cli --flag | e.g. my-cli --flag | Orchestrator args placeholder (custom CLI) | yes |
| 992 | 실행 중 | Running | Orchestrator status dot label | yes |
| 993 | 중지됨 | Stopped | Orchestrator status dot label | yes |
| 1004 | 신뢰됨 | Trusted | Orchestrator trust badge | yes |
| 1005 | 미신뢰 | Not Trusted | Orchestrator trust badge | yes |
| 1025 | 커스텀 | Custom | Orchestrator CLI picker tag | yes |
| 1032 | 워크스페이스 이름 | Workspace Name | Section header | yes |
| 1037 | 이름 | Name | Name TextField placeholder | yes |
| 1045 | 작업 디렉터리 | Working Directory | Section header | yes |
| 1058 | 기본값 | Default | CWD default-marker pill | yes |
| 1067 | 선택... | Choose... | CWD picker button | yes |
| 1072 | 선택 | Select | NSOpenPanel prompt | yes |
| 1079 | 초기화 | Reset | CWD reset button | yes |
| 1089 | 데이터 폴더 | Data Folder | Section header | yes |
| 1102 | 기본값 | Default | Data folder default-marker pill | yes |
| 1111 | 변경... | Change... | Data folder picker button | yes |
| 1116 | 선택 | Select | NSOpenPanel prompt | yes |
| 1128 | 오케스트레이터, 설정, 세션 데이터가 저장되는 위치입니다. | Where orchestrator, settings, and session data are stored. | Subtitle | yes |
| 1138 | CLI 인수 (선택적 재정의) | Args (optional override) | Section header | yes |
| 1153 | 비어 있으면 위 기본값이 사용됩니다. 값을 입력하면 완전히 재정의합니다. | Leave empty to use the default above. Any value fully overrides it. | Subtitle | yes |
| 1166 | 적용 및 재시작 | Apply & Restart | Apply button (running) | yes |
| 1167 | 적용 및 시작 | Apply & Start | Apply button (stopped) | yes |
| 1174 | 오케스트레이터 워크스페이스를 새 설정으로 (재)시작합니다. | Starts or restarts the orchestrator workspace with the new configuration. | Subtitle | yes |

**SettingsView refactor needed for Decision B: ZERO lines.** Everything already has an English counterpart.

### SessionSidebarView.swift — 3 Hangul occurrences, 100% via L10n

Owned by **aterm-claude** (read-only from sub1). Verified each site has a valid `en:` sibling.

| Line | Korean | English suggestion (already present) | Context | Via L10n? |
|---|---|---|---|---|
| 659 | 세션: | Session: | Label above sidebar session list | yes |
| 856 | 설정 | Settings | Gear menu item | yes |
| 910 | 태스크 (\(activeCount) 진행 중 / \(totalCount) 전체) | TASKS (\(activeCount) active / \(totalCount) total) | Task board section header | yes |

**SessionSidebarView refactor needed for Decision B: ZERO lines.**

### OrchestratorInputBar.swift — 1 Hangul occurrence, 100% via L10n

Owned by **aterm-claude** (read-only from sub1). Currently in active P0 regression-fix work per the task message; this audit is read-only.

| Line | Korean | English suggestion (already present) | Context | Via L10n? |
|---|---|---|---|---|
| 146 | 명령어 입력... | Type a command... | Orchestrator input NSTextView placeholder | yes |

**OrchestratorInputBar refactor needed for Decision B: ZERO lines.**

### AppDelegate.swift — 1 Hangul occurrence, 100% via L10n

Owned by **aterm-claude** (read-only from sub1).

| Line | Korean | English suggestion (already present) | Context | Via L10n? |
|---|---|---|---|---|
| 980 | 설정 | Settings | NSPanel window title for Preferences | yes |

**AppDelegate refactor needed for Decision B: ZERO lines.**

### TerminalView.swift — 1 Hangul occurrence (excluded)

| Line | Korean | Context | User-facing? |
|---|---|---|---|
| 522 | `// When user types Korean (ㅎ→하→한), each intermediate state renders at cursor cell.` | Code comment explaining Korean IME composition rendering | NO |

**Excluded from scope.** This is a developer-facing comment explaining an IME behavior. Decision B explicitly says "NSLog debug logs and code comments = developer's choice"; this is a code comment, so it stays as-is.

### Files with zero Hangul and zero user-facing string audit action

- `AtermLocalization.swift` — the infrastructure file itself (only touched by the Decision-B patch).
- `AtermTheme.swift` — color/spacing tokens, no strings at all.
- `GlyphAtlas.swift` — GPU font rasterizer backend, NSLog only.
- `MetalRenderer.swift` — Metal GPU renderer, NSLog only.
- `main.swift` — app entry point, no UI strings.
- `OrchestratorCommands.swift` — pure logic file. Verified zero `Text(`, `Button(`, `placeholderText`, `Label(`, and zero Hangul.
- `OrchestratorHistory.swift` — pure logic file. Same verification.
- `TeleptyBusClient.swift` — IPC client, no UI.

### OnboardingView.swift — SPECIAL CASE (out of scope for Decision B but worth flagging)

- Hangul occurrences: **0**
- `AtermLocalization.text` calls: **0**
- Swift UI Text/Button/placeholder calls: **43** (via grep)

**Interpretation:** OnboardingView is currently 100% hardcoded English strings with zero Korean counterparts and zero i18n wrapping. Under Decision B (English default), every one of those 43 elements displays correctly, so no user-visible regression. However, the file is **not prepared for future Korean localization** — reviving multi-lang would require wrapping all 43 strings in `AtermLocalization.text(ko:..., en:...)` with translated Korean copies.

**Recommendation:** Flag as a separate future-translation task. Not in scope for #246.

---

## 3. Summary counts

| Metric | Value |
|---|---|
| Swift files total | 14 |
| Files containing Hangul | 5 (incl. TerminalView comment) |
| Files with user-facing Hangul | 4 |
| Distinct user-facing Korean strings | 69 |
| Already routed via `AtermLocalization.text(ko:en:)` | 69 (100%) |
| Hardcoded user-facing Korean requiring new L10n wrapping | 0 |
| Files with no Hangul at all | 9 |
| Files with hardcoded English but no i18n wrapping (future concern) | 1 (OnboardingView — 43 elements) |

---

## 4. Implementation SPEC for Decision B

### 4.1 Single owning change

Only `macos/Sources/AtermLocalization.swift` needs a code edit. This file is not clearly owned by either sub1 or aterm-claude — it is an infrastructure file. Proposed owner: **sub1** (smallest-risk owner, since sub1 already touches L10n-adjacent code in SettingsView, and the change is 3 lines).

### 4.2 Proposed patch scope (~3 LOC)

```swift
enum AtermLocalization {
    /// Task #246: forced English. Korean strings preserved at call sites for
    /// future multi-lang revival — re-wire this getter when that returns.
    static var languageCode: String { "en" }

    static var isKorean: Bool { false }

    static func text(ko: String, en: String) -> String { en }
}
```

**Net effect:** every call site across the codebase that uses `AtermLocalization.text(ko: X, en: Y)` will unconditionally return `Y` (the English string). All Korean literals stay in source as the `ko:` parameter and continue to compile.

**Verification:** `swiftc -typecheck macos/Sources/*.swift -sdk ... -target arm64-apple-macos13.0 -I aterm-core -import-objc-header aterm-core/aterm_core.h`.

### 4.3 File ownership split for implementation

Because zero per-file refactoring is required, the ownership split is collapsed to a **single-file edit** in a single session:

| File | Refactor LOC needed | Owner |
|---|---|---|
| `AtermLocalization.swift` | ~3 | **sub1** (proposed) |
| `SettingsView.swift` | 0 | sub1 (already owned, no edit) |
| `SessionSidebarView.swift` | 0 | aterm-claude (no edit) |
| `OrchestratorInputBar.swift` | 0 | aterm-claude (no edit, also in P0 regression work) |
| `AppDelegate.swift` | 0 | aterm-claude (no edit) |
| `TerminalView.swift` | 0 | aterm-claude (comment excluded) |
| `OnboardingView.swift` | 0 for #246 (already English); ~90 LOC for future KO prep (out of scope) | future task |
| All other Swift files | 0 | — |

**This means aterm-claude has no implementation work for #246.** The split effectively becomes: sub1 patches AtermLocalization.swift, nothing else moves.

### 4.4 Risks

- **R1 `isKorean` / `languageCode` callers in the future.** Currently zero external callers. If someone later adds a `if AtermLocalization.isKorean { ... }` conditional (e.g. for Korean-only word-wrap logic or date formatting), that branch would silently stop firing. Mitigation: grep for `isKorean` / `languageCode` before merging any future PR that touches AtermLocalization. Not a blocker for #246.
- **R2 OS locale detection loss.** Users whose macOS is set to Korean will now see English in aterm even though other macOS apps speak Korean. That is the explicit user decision B; documented behavior, not a regression.
- **R3 Multi-line `ko:` literals with no `en:` sibling.** None found — every audited multi-line call has an `en:` argument. No degenerate cases.
- **R4 Korean string drift over time.** With English as the forced default, nobody tests the Korean path, so Korean strings may drift from their English counterparts or be forgotten on future feature additions. Documented risk, accepted per Decision B; the i18n infrastructure stays as a hibernating scaffold.
- **R5 Rust-side Korean strings.** Not audited per task scope (`aterm-core/*.rs` separate task). May or may not exist. If any, they are not addressed by this patch.

### 4.5 Failed approaches to avoid

- Wholesale deletion of the Korean string literals (Decision B rejects this).
- Touching aterm-core Rust (out of scope for v1).
- Writing a `~/.aigentry/config/aterm.json` key for language preference (out of scope — Decision B forces English unconditionally for v1; future multi-lang revival will design the config key then).
- Refactoring `OnboardingView.swift` to wrap its 43 strings (out of scope for Decision B — it already displays English correctly).

---

## 5. Questions for user decision

- **Q1 — AtermLocalization.swift owner.** Proposed owner sub1 (3-line edit, smallest risk surface). Alternative: aterm-claude, who owns most other user-facing files. Default: **sub1**.
- **Q2 — OnboardingView.swift.** 43 hardcoded English strings with zero i18n wrapping. Decision B does not require action. Should I flag this as a separate follow-up task for "future Korean translation readiness", or leave it silent? Default: **flag as follow-up, not in #246**.
- **Q3 — `isKorean` and `languageCode` getter preservation.** My proposed patch keeps these as computed getters returning constants. Alternative: remove them entirely since nothing else reads them. Default: **keep getters** — tiny cost, preserves API surface for future revival.
- **Q4 — Rust side (aterm-core).** Not audited here (task scope). Should the Rust audit happen in parallel or sequentially? Default: **separate task after v1 ships**, per the task message.
- **Q5 — Code comments and NSLog.** TerminalView.swift:522 has a Korean-mentioning code comment about IME composition. Task message says "developer's choice" for comments and NSLog. I did not audit NSLog calls. Should I? Default: **skip NSLog audit** — Decision B explicitly exempts developer-facing strings.
- **Q6 — Locale-specific MacOS behaviors.** NSOpenPanel's `prompt` strings, alert dialog Button titles, Menu items, Services menu — are there any using platform-provided localization (e.g. `NSLocalizedString`) that would still speak Korean even after this patch? My grep found zero `NSLocalizedString` calls, so the answer is "no". Documenting for completeness.
