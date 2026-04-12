# REPORT: SettingsView lazy variant badge — implementation

**Session:** aigentry-aterm-claude-sub1
**File:** `macos/Sources/SettingsView.swift` (sole edit)
**SPEC ref:** 562cc3679caa... (settingsview-variant-badge-spec.md)
**Decision ref:** Q-font-4 C2 (lazy detection, badge on selected font only)
**Build check:** `swiftc -typecheck` PASS for SettingsView.swift (0 errors). Pre-existing 4 errors in TerminalView.swift:764/809/1237/1382 (`aterm_core_write_pty` Int/UInt size_t bridging) unchanged — NOT touched and NOT regressed by this SPEC.

## LOC budget
~55 LOC net added (struct + static helper + view @State + computed + badge HStack + onAppear + onChange one-line extension). This is above the SPEC's "~30 LOC" estimate because the `FontVariantAvailability` struct + static detection helper together are ~35 LOC on their own. The view-layer additions are ~20 LOC. No cross-file impact, no new types exposed beyond the intended `FontVariantAvailability` helper.

## Edits applied (6)

### 1. FontVariantAvailability struct (top-level, before `class AtermSettings`)
- `let hasBold: Bool`, `let hasItalic: Bool`, `let hasBoldItalic: Bool`
- `var allPresent: Bool { hasBold && hasItalic && hasBoldItalic }`
- `func missing() -> [String]` returning in display order: `Bold`, `Italic`, `Bold Italic` (any that are false)
- Two static sentinels: `static let allPresent = ...(true, true, true)` and `static let noneKnown = ...(false, false, false)`
- Placed after `// MARK: - Settings Model` and before the `class AtermSettings` comment so the struct is visible to both the class and the view structs without ordering gymnastics.

### 2. `AtermSettings.detectFontVariants(family:)` static method
- Placed directly after `allFontFamilies` static var.
- Short-circuits `System Default` → returns `.allPresent` with zero NSFontManager work.
- Calls `NSFontManager.shared.availableMembers(ofFontFamily: family)`. Nil or empty → returns `.noneKnown` (all false).
- Iterates each member. Guards `member.count >= 4` and `member[3] as? UInt` — the trait bitmask. Members that fail this cast are skipped (Q3 default: UInt-only, no Int fallback).
- Decodes the UInt into `NSFontTraitMask(rawValue:)` and uses `.contains(.boldFontMask)` / `.contains(.italicFontMask)` for locale-robust detection (works regardless of face-name string language).
- Bucket logic: `isBold && isItalic → hasBoldItalic = true`, `isBold only → hasBold = true`, `isItalic only → hasItalic = true`. Regular faces hit neither bucket.
- Returns a populated `FontVariantAvailability`.

### 3. `AppearanceSettingsView` new state + computed
- Added `@State private var variantAvailability: FontVariantAvailability = .allPresent` next to the existing `@State var showAllFonts`.
- Added `private var variantBadgeMessage: String` computed: reads `variantAvailability.missing()`, returns `""` if empty, otherwise joins with `", "` and wraps in `AtermLocalization.text(ko: "없음: \(joined)", en: "Missing: \(joined)")`. Q1 A terse format; Q5 two-path split handled naturally by the `, `-joined list (singular = `없음: Bold`, plural = `없음: Bold, Italic`).

### 4. Extended `.onChange(of: settings.fontFamily)` closure
- Added one line at the end of the existing closure: `variantAvailability = AtermSettings.detectFontVariants(family: settings.fontFamily)`.
- Preserved the existing `settings.save()` and `onApply()` calls and their order (save-before-onApply is the 4 gap invariant).

### 5. Badge HStack inserted between Family picker row and Show-all-fonts row
- Renders only when `!variantAvailability.allPresent && settings.fontFamily != "System Default"`. Double guard because `variantAvailability` starts at `.allPresent` but could be updated asynchronously before the family string resets — the second guard keeps the sentinel totally silent.
- Structure: `HStack(alignment: .center, spacing: 6)` containing:
  - `Image(systemName: "exclamationmark.triangle.fill")` at `.font(.system(size: 10))` in `AtermTheme.statusWarning` (Q2 default, same token already used by OrchestratorSettingsView's "Not Trusted" shield at line 776).
  - `Text(variantBadgeMessage)` at `.font(.system(size: 10))` in `AtermTheme.textMuted` with `.fixedSize(horizontal: false, vertical: true)` so the message wraps instead of clipping if the family name is long.
  - `Spacer()` to left-align the content.
- `.padding(.leading, 80)` (Q4 default) so the badge content starts at the same x-offset as the Picker's content edge (the Family label column is 80pt wide).
- The conditional `if` around the HStack means the layout occupies zero space when variants are complete, which is the desired clean-UI behavior.

### 6. `.onAppear` on the outer VStack
- Added `.onAppear { variantAvailability = AtermSettings.detectFontVariants(family: settings.fontFamily) }` after `.padding(20)` on the root VStack.
- Fires every time the user opens the Settings window and the Appearance tab is first visible. Each reopen re-runs detection — correct behavior for a memory-only @State (no cross-open cache).

## Invariants preserved
- **Rule 1 경량.** Implementation is surgical: one struct + one static func + three view edits (state, computed, HStack insertion) + one onAppear + one onChange line. No refactoring of existing picker or toggle code.
- **Rule 9 독립.** Single-file edit. Zero impact on AppDelegate, AtermTheme, GlyphAtlas, TerminalView, or any sibling file. Zero new cross-file API.
- **4 gap work intact.** The Font Family Picker, Show-all-fonts toggle, shell picker, tailscale toggle, and orchestrator.args unhiding all still build and function. I did not touch any of them — only extended the existing `onChange(of: settings.fontFamily)` closure with a single additional line and inserted the badge HStack between the picker row and the Show-all-fonts row.
- **AtermTheme tokens only.** Used `AtermTheme.statusWarning` (already used at line 776 in OrchestratorSettingsView) and `AtermTheme.textMuted` (used throughout). No new color definitions.
- **AtermLocalization bilingual.** KO and EN strings provided via the existing `AtermLocalization.text(ko:en:)` API.
- **Memory-only state** (Q-font-4 C2). `variantAvailability` is `@State`, lives for the Settings window lifetime. No persistence to config.json, no new schema key, no cache file.
- **No new `SettingsTab` enum case.** Badge lives inside the existing Appearance tab.
- **480x520 window size.** Unchanged. Badge is a single 10pt line when visible and absent otherwise; existing ScrollView inside the Appearance tab absorbs the extra vertical pixels.

## Failed approaches NOT used
- No eager detection of all font family variants (Q-font-4 C2 lazy decision respected).
- No AppDelegate.swift, GlyphAtlas.swift, TerminalView.swift, or aterm-core touches.
- No cache file persistence.
- No red alarming color — used `AtermTheme.statusWarning` (yellow/amber tint that aterm already uses for the Not-Trusted shield).
- No separate helper View struct for the badge — inlined per the SPEC's "small scope" preference.

## Mental round-trip

1. User opens Settings → Appearance tab. `.onAppear` fires on the outer VStack. `variantAvailability` is set to `detectFontVariants("Menlo")` = `.allPresent` (Menlo ships Bold/Italic/BoldItalic). Badge conditional is false → no badge renders.
2. User switches to "Monaco" via Picker. `.onChange` fires:
   - `settings.save()` — config.json updated.
   - `onApply()` — existing live-apply path runs.
   - `variantAvailability = detectFontVariants("Monaco")` — Monaco ships only Regular → `FontVariantAvailability(false, false, false)`.
   - SwiftUI re-renders. Badge conditional is now true and family is not `System Default` → badge HStack appears with SF Symbol + text `Missing: Bold, Italic, Bold Italic`.
3. User clicks "Show all fonts" toggle and picks "Helvetica" (proportional, full family with Bold/Italic/BoldItalic). `.onChange` fires same sequence → `detectFontVariants("Helvetica")` = `.allPresent`. Badge disappears.
4. User picks "System Default". `.onChange` fires → `detectFontVariants("System Default")` short-circuits to `.allPresent`. Badge stays hidden.
5. User closes Settings, reopens. `.onAppear` re-runs detection for whatever `settings.fontFamily` currently is — fresh detection, no stale state.

Round-trip OK.

## Known concerns

1. **Separate-family false positive (R3 from SPEC).** Families that ship Bold as a separate `-Bold`-suffixed family rather than as a face member of the same family will report missing Bold. Accepted risk; low frequency on modern macOS. Not a blocker.
2. **`UInt` trait cast defensiveness (R4 from SPEC).** Went with Q3 default (UInt only, no Int fallback). If a macOS version returns Int-bridged NSNumber instead of UInt-bridged, the cast silently skips the member and the badge under-reports. Mitigation is a one-line follow-up if it ever surfaces.
3. **Stale list on mid-session font install (R1).** NSFontManager list is process-cached. If a user installs a font after launch, neither the picker nor the badge reflects it until relaunch. Matches existing macOS norms.
4. **Badge layout jitter on switch.** The `if` conditional causes a vertical reflow when the badge appears/disappears. Minor visual jitter; SwiftUI handles it cleanly. Acceptable.
5. **Runtime font plumbing.** Orthogonal concern handled by the parallel aterm-claude SPEC on `AppDelegate.applySettingsToView` / `CTFontCreateWithName`. This SPEC's badge is purely informational and does not depend on runtime font switching being done.
6. **Pre-existing TerminalView.swift errors.** 4 `aterm_core_write_pty` Int/UInt size_t errors remain at lines 764, 809, 1237, 1382. NOT touched and NOT regressed by this SPEC. Still flagged for Builder / aterm-claude.
7. **Pre-existing SettingsView.swift single-file SourceKit errors.** The standalone-file analyzer flags `AtermTheme` and `AtermLocalization` as "not in scope" because those types live in sibling .swift files. When the whole module is type-checked together (as done for verification), those errors vanish. Noise, not real issues, pre-existing before this SPEC.

## SAWP compliance

- Code only — no `make`, no `cargo build`, no app launch.
- `swiftc -typecheck` only, run against the full `macos/Sources/*.swift` set with `-sdk $(xcrun --sdk macosx --show-sdk-path) -target arm64-apple-macos13.0 -I aterm-core -import-objc-header aterm-core/aterm_core.h`.
- Zero compile errors in SettingsView.swift.
- Self-diff: `git status --short macos/Sources/` shows the same 6 files in the modified set that were already modified at session start; no additional files added. All session-introduced diff is inside SettingsView.swift.
- 3-attempt stuck counter: not invoked — first attempt compiled clean.
