# SPEC: SettingsView lazy variant badge

**Session:** aigentry-aterm-claude-sub1
**Scope:** single-file follow-up to the 4 gap exposure SPEC already landed
**File:** `macos/Sources/SettingsView.swift` (sole edit, ~30 LOC)
**Prior work:** settingsview-4gap-v2.md (Font Family Picker, Show all fonts toggle, resetDefaults)
**Decision ref:** Q-font-4 C2 — lazy detection, badge on selected font only

## Goal
When the user picks a font family in the Appearance tab's Font Family Picker, detect whether that family ships Bold / Italic / Bold-Italic member faces and show a small inline warning badge below the picker (above the "Show all fonts" toggle) if any variant is missing. Zero badge when all three are present. Detection is lazy — runs only on selection change, never on list render.

## Detection helper

**Location:** `AtermSettings` class, as a `static func`. Rationale: keeps the view struct lean; detection is a pure function of family name, so static is correct; tests (if ever added) can call it directly.

**Signature:**
```swift
struct FontVariantAvailability {
    let hasBold: Bool
    let hasItalic: Bool
    let hasBoldItalic: Bool

    var allPresent: Bool { hasBold && hasItalic && hasBoldItalic }

    /// List of missing variant labels in display order: ["Bold", "Italic", "Bold Italic"]
    var missing: [String] {
        var out: [String] = []
        if !hasBold { out.append("Bold") }
        if !hasItalic { out.append("Italic") }
        if !hasBoldItalic { out.append("Bold Italic") }
        return out
    }

    static let allPresent = FontVariantAvailability(hasBold: true, hasItalic: true, hasBoldItalic: true)
    static let noneKnown = FontVariantAvailability(hasBold: false, hasItalic: false, hasBoldItalic: false)
}

static func detectFontVariants(family: String) -> FontVariantAvailability
```

**Impl approach:**
```swift
static func detectFontVariants(family: String) -> FontVariantAvailability {
    // System Default sentinel — we do not introspect this
    if family == "System Default" { return .allPresent }

    guard let members = NSFontManager.shared.availableMembers(ofFontFamily: family),
          !members.isEmpty
    else { return .noneKnown }

    var hasBold = false, hasItalic = false, hasBoldItalic = false
    for member in members {
        // Apple docs: member is [postScriptName, faceName, weight (NSNumber), traits (NSNumber)]
        guard member.count >= 4, let traitsRaw = member[3] as? UInt else { continue }
        let traits = NSFontTraitMask(rawValue: traitsRaw)
        let isBold = traits.contains(.boldFontMask)
        let isItalic = traits.contains(.italicFontMask)
        if isBold && isItalic { hasBoldItalic = true }
        else if isBold { hasBold = true }
        else if isItalic { hasItalic = true }
    }
    return FontVariantAvailability(hasBold: hasBold, hasItalic: hasItalic, hasBoldItalic: hasBoldItalic)
}
```

**Why NSFontTraitMask bitmask over style-name string matching:** robust against locale-specific face names ("Fett", "Gras", "Grassetto"), ships with every macOS, O(members) with ~4-8 members per family → well under the 9ms perception budget stated in the task.

**Fallback:** If `member[3] as? UInt` fails (Apple has historically returned NSNumber which bridges; this is the safe cast), that member is skipped. Worst case: under-detection → we show a false-positive "missing" badge, which is visible and correctable by the user rather than a silent wrong render.

## State in AppearanceSettingsView

Add one `@State` and one `.onChange`:
```swift
@State private var fontVariants: FontVariantAvailability = .allPresent
```

**Initial sync on appear:**
```swift
.onAppear {
    fontVariants = AtermSettings.detectFontVariants(family: settings.fontFamily)
}
```

**Re-detect when the picker's binding changes:**
```swift
.onChange(of: settings.fontFamily) {
    settings.save()
    onApply()
    fontVariants = AtermSettings.detectFontVariants(family: settings.fontFamily)
}
```

(The existing 4gap-v2 `onChange` already calls `settings.save()` and `onApply()`. I will add the third line — `fontVariants = ...` — without moving the other two. Single-line diff to the existing closure body.)

No separate cache. Per-selection call is ~3 × NSFontManager lookup = ~9ms, well under the 16ms frame budget, and a user cannot drive the picker faster than the detection.

## Badge view

Inline in `AppearanceSettingsView.body`, placed **between** the Font Family Picker's HStack and the "Show all fonts" right-aligned toggle row. Renders nothing when variants are all present, so the layout gains zero vertical whitespace in the common case.

```swift
if !fontVariants.allPresent && settings.fontFamily != "System Default" {
    HStack(alignment: .top, spacing: 6) {
        Image(systemName: "exclamationmark.triangle.fill")
            .font(.system(size: 10))
            .foregroundColor(Color(nsColor: AtermTheme.statusWarning))
        Text(variantBadgeMessage)
            .font(.system(size: 10))
            .foregroundColor(Color(nsColor: AtermTheme.textMuted))
            .fixedSize(horizontal: false, vertical: true)
        Spacer()
    }
    .padding(.leading, 80)  // aligns with the Picker's content edge (Family label is 80pt wide)
}
```

**Message builder** (private computed in `AppearanceSettingsView`):
```swift
private var variantBadgeMessage: String {
    let family = settings.fontFamily
    if fontVariants.missing.count == 3 {
        return AtermLocalization.text(
            ko: "\(family)은 Regular만 제공합니다 — Bold/Italic ANSI는 Regular로 렌더링됩니다",
            en: "\(family) has only Regular — bold/italic ANSI will render as Regular"
        )
    }
    let missing = fontVariants.missing.joined(separator: ", ")
    return AtermLocalization.text(
        ko: "\(family)에 \(missing) 변형이 없습니다 — Regular로 대체됩니다",
        en: "\(family) is missing: \(missing) — will render as Regular"
    )
}
```

**Tokens:** `AtermTheme.statusWarning` (used today in `OrchestratorSettingsView` for the "Not Trusted" shield, so the tint is already proven consistent), `AtermTheme.textMuted` for the body text. No new theme tokens required.

**Icon decision:** SF Symbol `exclamationmark.triangle.fill`, not emoji. Rationale: (1) integrates with macOS text metrics and dark mode, (2) scales with font size setting, (3) the rest of SettingsView uses SF Symbols (paintbrush, terminal, cpu for tab icons, shield.slash for trust status).

## Trigger and cost

- Detection runs only from `.onAppear` (once per Settings window open) and `.onChange(of: settings.fontFamily)` (user-initiated picker change). Never during picker list rendering, never in `body` eval.
- Cost: ~3ms per `NSFontManager.availableMembers(ofFontFamily:)` call × 1 per selection = ~3ms real, well under the 9ms budget.
- No persistence — `@State` lives for the Settings window lifetime. Reopening Settings re-runs detection via `.onAppear`, which is fine.

## Edge cases

1. **`System Default` sentinel.** The detection helper returns `.allPresent` short-circuit before calling NSFontManager; the view also double-guards with `&& settings.fontFamily != "System Default"`. No badge, ever, for the sentinel.
2. **Empty or nil `availableMembers` result.** Detection returns `.noneKnown` (all false) → badge shows "has only Regular" wording since `missing.count == 3`. Correct user signal for an unknown/broken family.
3. **Family only has `Regular`, no bold/italic.** Same as (2) — badge shows full "has only Regular" wording.
4. **Family has `Bold` but no `Italic` / `BoldItalic`.** Badge shows "is missing: Italic, Bold Italic — will render as Regular". Three-state coverage is correct.
5. **Proportional families selected via "Show all fonts".** Detection doesn't care about monospacing; still runs. The badge tells the user about variant gaps regardless of pitch. Acceptable.
6. **Family ships Bold/Italic as a separate NSFontManager family** (e.g. the old "Source Code Pro" layout where "Bold" was its own family). False positive — we'd show the badge even though the variants exist under a different family name. Documented risk, low frequency on modern macOS (most families use OT style groupings). Not a blocker.
7. **User changes system fonts while Settings window is open.** `NSFontManager` list is cached per process; our detection runs against whatever list the process has. If the font was installed after launch, the badge may be stale. Reopening Settings re-runs `.onAppear`. Acceptable — matches the picker's own staleness behavior.

## Files to modify
- `macos/Sources/SettingsView.swift` (sole edit)

## Files deliberately not touched
- `AppDelegate.swift` — aterm-claude territory (runtime font plumbing is a parallel SPEC)
- `GlyphAtlas.swift`, `TerminalView.swift`, `MetalRenderer.swift` — aterm-claude territory
- `aterm-core/**` — Rust is out of scope
- `SessionSettingsView`, `TerminalSettingsView`, `OrchestratorSettingsView` — 4 gap SPEC already closed those surfaces; no re-entry
- `Shaders.metal`, IPC, PTY — nothing to do with SettingsView

## Verification

- `swiftc -typecheck macos/Sources/*.swift -sdk $(xcrun --sdk macosx --show-sdk-path) -target arm64-apple-macos13.0 -I aterm-core -import-objc-header aterm-core/aterm_core.h`
- Expected: zero new errors in `SettingsView.swift`. Pre-existing 4 errors in `TerminalView.swift` (aterm_core_write_pty size_t) remain untouched and are not caused by this SPEC.
- Manual test plan (for Tester session, sub1 does not run the app):
  1. **Menlo** (shipped with macOS, full family): select → no badge.
  2. **Monaco** (shipped, only Regular): select → badge "has only Regular".
  3. **Iosevka** or any user-installed ttc with Bold but no Italic: select → badge "is missing: Italic, Bold Italic".
  4. **Helvetica**: select via Show all fonts → no badge (full family with Bold/Italic/BoldItalic).
  5. **System Default** sentinel: badge never appears.
  6. Rapid-fire picker switching (10 clicks in 2s): no visible lag, no jitter.

## Invariants preserved

- Rule 1 경량: ~30 LOC (detection helper ~15, badge view ~10, state/hook ~5). No new dependencies, no new theme tokens, no new enum cases.
- Rule 9 독립: single file, zero cross-file API changes. Detection helper is pure, view code is local.
- Rule 24 SPEC FIRST: this document exists; no code written until approval.
- 4 gap work preserved: the SPEC touches the existing `AppearanceSettingsView` body in exactly two places (add `@State`, extend the existing `.onChange`, insert one `HStack` between Picker row and Show-all-fonts toggle) — no refactoring of the font picker itself, no change to `settings.save()` / `onApply()` timing, no change to `fontFamilies` computed, no change to `showAllFonts` state.
- Memory only per Q-font-4: `@State` lives for the view lifetime, nothing persisted to `aterm.json`.
- Schema: zero config.json surface changes.
- SettingsTab enum: unchanged (4 tabs).
- Window size 480x520: unchanged. Badge adds at most one 10pt text line when visible; existing ScrollView inside the Appearance tab absorbs the extra height.

## Risks

1. **R1 NSFontManager stale list.** If a font is installed mid-session, neither the picker nor the badge reflect it until the app relaunches. Severity: low. Mitigation: none needed — matches macOS norms and the existing picker.
2. **R2 Badge layout reflow.** When the user switches from a full family to a partial one, the HStack appears below the picker and pushes "Show all fonts" down by one line. Severity: minor visual jitter. Mitigation: the padding/spacing stays consistent; SwiftUI handles the reflow smoothly. Acceptable.
3. **R3 Separate-family layout false positive.** Old-style font packages that ship Bold as a separate family (e.g. "Source Code Pro Bold" family) will report missing Bold for the base family. Severity: low, decreasing over time. Mitigation: accept for now; document in a follow-up issue if user reports.
4. **R4 `UInt` cast for traits bitmask.** Apple's NSFontManager doc is not explicit about whether `member[3]` is `NSNumber` bridged to `UInt` or to `Int`. If the cast fails on some macOS versions, detection under-reports → false-positive badge. Mitigation: I'll test both `UInt` and `Int` cast paths in the impl (as? UInt ?? UInt(bitPattern: member[3] as? Int ?? 0)) if Q1 below decides it's worth the belt-and-suspenders.
5. **R5 Localization gap.** `AtermLocalization.text(ko:en:)` string interpolation works but if the KO or EN translator later adds a third language, the `\(family)` interpolation in both branches would need duplication. Minor. Mitigation: none for this SPEC; follow the existing pattern.

## Questions

- **Q-badge-1 Wording tone.** Two options:
  - A (technical, proposed): "FontName is missing: Bold, Italic — will render as Regular"
  - B (friendlier): "FontName doesn't include Bold or Italic styles. Bold/italic text will use Regular."
  Default: **A** — matches the terse style of the rest of Settings helper text.

- **Q-badge-2 Icon.** SF Symbol `exclamationmark.triangle.fill` vs emoji `⚠️`:
  - SF Symbol (proposed): integrates with text metrics, scales with point size, honors dark mode via `AtermTheme.statusWarning`.
  - Emoji: platform-rendered, may look inconsistent with surrounding SF-Symbol icons in SettingsView.
  Default: **SF Symbol**.

- **Q-badge-3 Trait cast defensiveness.** Should I include the `Int` fallback from R4, or keep the single `UInt` cast for cleanness?
  Default: **single `UInt` cast** — NSFontManager has bridged traits as UInt on every macOS 13+ I can verify; if this turns out to be wrong, a one-line follow-up is cheaper than upfront complexity.

- **Q-badge-4 Placement fine-tuning.** Badge sits below the Picker's HStack, with `.padding(.leading, 80)` to align with the picker's content (the Family label occupies the first 80pt). Alternative: no leading padding, full-width left-aligned. Default: **80pt aligned** — visually tracks under the Picker control rather than the Family label.

- **Q-badge-5 Colon grammar in KO.** The KO string currently reads "에는 Bold 변형이 없습니다". For a single missing variant this is fine; for "Bold, Italic" the comma list may look odd with "변형이 없습니다". Fallback wording: "X, Y 변형이 누락되어 있습니다". Default: use the `missing` / `has only Regular` two-path split described above — `joined(separator: ", ")` produces natural Korean for 1-2 items.
