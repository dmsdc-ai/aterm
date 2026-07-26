import AppKit
import SwiftUI

// MARK: - Settings Model

/// Availability of Bold / Italic / BoldItalic face variants for a font family.
/// Detected lazily per picker selection (Q-font-4 C2); memory-only, no persistence.
struct FontVariantAvailability {
    let hasBold: Bool
    let hasItalic: Bool
    let hasBoldItalic: Bool

    var allPresent: Bool { hasBold && hasItalic && hasBoldItalic }

    /// Missing variant labels in display order.
    func missing() -> [String] {
        var out: [String] = []
        if !hasBold { out.append("Bold") }
        if !hasItalic { out.append("Italic") }
        if !hasBoldItalic { out.append("Bold Italic") }
        return out
    }

    static let allPresent = FontVariantAvailability(hasBold: true, hasItalic: true, hasBoldItalic: true)
    static let noneKnown = FontVariantAvailability(hasBold: false, hasItalic: false, hasBoldItalic: false)
}

/// Persistent settings stored in ~/.aigentry/config/aterm.json
class AtermSettings: ObservableObject {
    static let shared = AtermSettings()

    // Appearance
    @Published var colorScheme: String = "Default"
    @Published var fontSize: Double = 18
    @Published var fontFamily: String = "System Default"
    @Published var lineHeight: Double = 1.0
    @Published var backgroundOpacity: Double = 100
    @Published var showStatusEmoji: Bool = false
    @Published var useAsciiIcons: Bool = false

    // Terminal
    @Published var defaultCLI: String = "none"
    @Published var defaultCWD: String = ""
    @Published var scrollbackLines: Int = 10000
    @Published var cursorStyle: String = "block"

    // Session
    @Published var autoRestoreSessions: Bool = true
    @Published var autoRestartDead: Bool = false
    @Published var maxRestartAttempts: Int = 3

    // Sidebar
    @Published var showTaskBoard: Bool = true

    // Shell
    @Published var shellDefault: String = "zsh"

    // Tailscale
    @Published var tailscaleConnectOnLaunch: Bool = false

    // CLI Defaults (per-CLI arguments)
    @Published var cliDefaults: [String: String] = AtermSettings.defaultCliArgs

    static let defaultCliArgs: [String: String] = [
        "claude": "--dangerously-skip-permissions --continue",
        "codex": "resume --dangerously-bypass-approvals-and-sandbox",
        "gemini": "resume -y",
    ]

    // Orchestrator
    @Published var orchestratorCLI: String = "claude"
    @Published var orchestratorArgs: String = ""
    @Published var orchestratorName: String = "orchestrator"
    @Published var orchestratorCWD: String = ""

    /// Resolved orchestrator CWD: user-set value or default (aigentryRoot/orchestrator)
    var effectiveOrchestratorCWD: String {
        orchestratorCWD.isEmpty ? aigentryRoot + "/orchestrator" : orchestratorCWD
    }

    /// Base data root: ATERM_DATA_ROOT env var if set, else ~/.aigentry
    static var dataRoot: String {
        if let root = ProcessInfo.processInfo.environment["ATERM_DATA_ROOT"], !root.isEmpty {
            return root
        }
        return NSHomeDirectory() + "/.aigentry"
    }

    // Data folder (aigentry root)
    @Published var aigentryRoot: String = AtermSettings.dataRoot

    static let colorSchemes = [
        "Default", "Dark", "Light", "Solarized Dark", "Solarized Light",
        "Monokai", "Dracula", "Nord", "Tokyo Night",
    ]

    static let cursorStyles = ["block", "underline", "bar"]

    static let shellPresets = ["zsh", "bash", "fish"]

    /// Monospaced font families (isFixedPitch filter)
    static var monospacedFontFamilies: [String] {
        NSFontManager.shared.availableFontFamilies.compactMap { family -> String? in
            guard let members = NSFontManager.shared.availableMembers(ofFontFamily: family),
                  let firstMember = members.first,
                  firstMember.count >= 1,
                  let postScript = firstMember[0] as? String,
                  let font = NSFont(name: postScript, size: 12),
                  font.isFixedPitch
            else { return nil }
            return family
        }.sorted()
    }

    /// All available font families (unfiltered)
    static var allFontFamilies: [String] {
        NSFontManager.shared.availableFontFamilies.sorted()
    }

    /// Lazily detect which of Bold / Italic / BoldItalic face variants a family ships.
    /// Called only on user picker selection change; ~3ms per call, no cache.
    static func detectFontVariants(family: String) -> FontVariantAvailability {
        if family == "System Default" { return .allPresent }
        guard let members = NSFontManager.shared.availableMembers(ofFontFamily: family),
              !members.isEmpty
        else { return .noneKnown }

        var hasBold = false
        var hasItalic = false
        var hasBoldItalic = false
        for member in members {
            // member = [postScriptName, faceName, weight (NSNumber), traits (NSNumber)]
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

    /// Map scheme name → C-FFI u8 value
    static func schemeIndex(_ name: String) -> UInt8 {
        switch name {
        case "Dark": return 0
        case "Light": return 1
        case "Solarized Dark": return 2
        case "Solarized Light": return 3
        case "Monokai": return 4
        case "Dracula": return 5
        case "Nord": return 6
        case "Tokyo Night": return 7
        case "Default": return 8
        default: return 8
        }
    }

    static func schemeName(_ index: UInt8) -> String {
        switch index {
        case 0: return "Dark"
        case 1: return "Light"
        case 2: return "Solarized Dark"
        case 3: return "Solarized Light"
        case 4: return "Monokai"
        case 5: return "Dracula"
        case 6: return "Nord"
        case 7: return "Tokyo Night"
        case 8: return "Default"
        default: return "Default"
        }
    }

    private static var bootstrapPath: String { dataRoot + "/config/aterm.json" }

    private var configPath: String {
        aigentryRoot + "/config/aterm.json"
    }

    /// Read aigentry_root from config file (~/.aigentry/config/aterm.json), default to ~/.aigentry
    private static func resolveAigentryRoot() -> String {
        let defaultRoot = dataRoot
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: bootstrapPath)),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let root = json["aigentry_root"] as? String
        else { return defaultRoot }
        // Expand ~ if present
        let expanded = root.hasPrefix("~/")
            ? NSHomeDirectory() + String(root.dropFirst(1))
            : root
        return expanded.isEmpty ? defaultRoot : expanded
    }

    /// Save aigentry_root pointer to bootstrap file
    private func saveBootstrap() {
        let dir = URL(fileURLWithPath: Self.bootstrapPath).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let json: [String: Any] = ["aigentry_root": aigentryRoot]
        if let data = try? JSONSerialization.data(withJSONObject: json, options: [.prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: Self.bootstrapPath))
        }
    }

    /// Migrate .aigentry contents to a new location
    func migrateAigentryRoot(to newRoot: String) -> Bool {
        let fm = FileManager.default
        let oldRoot = aigentryRoot
        guard oldRoot != newRoot else { return true }

        // Create destination
        do {
            try fm.createDirectory(atPath: newRoot, withIntermediateDirectories: true)
        } catch {
            NSLog("[aterm] migrate: failed to create destination %@: %@", newRoot, error.localizedDescription)
            return false
        }

        // Copy contents
        do {
            let contents = try fm.contentsOfDirectory(atPath: oldRoot)
            for item in contents {
                let src = oldRoot + "/" + item
                let dst = newRoot + "/" + item
                if fm.fileExists(atPath: dst) {
                    try fm.removeItem(atPath: dst)
                }
                try fm.copyItem(atPath: src, toPath: dst)
            }
        } catch {
            NSLog("[aterm] migrate: copy failed from %@ to %@: %@", oldRoot, newRoot, error.localizedDescription)
            return false
        }

        // Verify config exists at new location
        let newConfig = newRoot + "/config/aterm.json"
        guard fm.fileExists(atPath: newConfig) else {
            NSLog("[aterm] migrate: verification failed — config not found at %@", newConfig)
            return false
        }

        // Update state and save pointer
        aigentryRoot = newRoot
        saveBootstrap()

        // Remove old directory only after successful migration
        try? fm.removeItem(atPath: oldRoot)
        NSLog("[aterm] migrated .aigentry from %@ to %@", oldRoot, newRoot)
        return true
    }

    func load() {
        // Resolve aigentry root from bootstrap file first
        aigentryRoot = Self.resolveAigentryRoot()

        guard let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }

        let appearance = config["appearance"] as? [String: Any]
        let terminal = config["terminal"] as? [String: Any]
        let session = config["session"] as? [String: Any]
        let ai = config["ai"] as? [String: Any]

        if let s = appearance?["colorScheme"] as? String { colorScheme = s }
        let rawFS = appearance?["fontSize"]
        NSLog("[DIAG-LOAD] rawFontSize=%@ type=%@ asDouble=%@ asInt=%@",
              String(describing: rawFS),
              String(describing: type(of: rawFS)),
              String(describing: rawFS as? Double),
              String(describing: rawFS as? Int))
        if let s = appearance?["fontSize"] as? Double { fontSize = round(s) }
        else if let s = appearance?["fontSize"] as? Int { fontSize = Double(s) }
        NSLog("[DIAG-LOAD] configPath=%@ fontSize=%.1f", configPath, fontSize)
        if let s = appearance?["fontFamily"] as? String { fontFamily = s }
        if let s = appearance?["lineHeight"] as? Double { lineHeight = s }
        if let s = appearance?["backgroundOpacity"] as? Double { backgroundOpacity = s }
        if let s = appearance?["showStatusEmoji"] as? Bool { showStatusEmoji = s }
        if let s = appearance?["useAsciiIcons"] as? Bool { useAsciiIcons = s }
        if let s = ai?["defaultCLI"] as? String { defaultCLI = s }
        if let s = terminal?["defaultCWD"] as? String { defaultCWD = s }
        if let s = terminal?["scrollbackLines"] as? Int { scrollbackLines = s }
        if let s = terminal?["cursorStyle"] as? String { cursorStyle = s }

        if let s = session?["autoRestore"] as? Bool { autoRestoreSessions = s }
        if let s = session?["autoRestartDead"] as? Bool { autoRestartDead = s }
        if let s = session?["maxRestartAttempts"] as? Int { maxRestartAttempts = s }

        let sidebar = config["sidebar"] as? [String: Any]
        if let s = sidebar?["showTaskBoard"] as? Bool { showTaskBoard = s }

        let shell = config["shell"] as? [String: Any]
        if let s = shell?["default"] as? String, !s.isEmpty { shellDefault = s }

        let tailscale = config["tailscale"] as? [String: Any]
        if let s = tailscale?["connect_on_launch"] as? Bool { tailscaleConnectOnLaunch = s }

        if let cd = config["cli_defaults"] as? [String: String] {
            var merged = AtermSettings.defaultCliArgs
            for (k, v) in cd { merged[k] = v }
            cliDefaults = merged
        }

        let orchestrator = config["orchestrator"] as? [String: Any]
        if let s = orchestrator?["cli"] as? String { orchestratorCLI = s }
        else if let s = ai?["defaultCLI"] as? String, s != "none" { orchestratorCLI = s }
        if let s = orchestrator?["args"] as? String { orchestratorArgs = s }
        if let s = orchestrator?["name"] as? String { orchestratorName = s }
        if let s = orchestrator?["cwd"] as? String, !s.isEmpty { orchestratorCWD = s }
    }

    func save() {
        // Read existing config to preserve fields we don't manage
        var config: [String: Any] = [:]
        if let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
           let existing = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            config = existing
        }

        config["appearance"] = [
            "colorScheme": colorScheme,
            "fontSize": round(fontSize),
            "fontFamily": fontFamily,
            "lineHeight": lineHeight,
            "backgroundOpacity": backgroundOpacity,
            "showStatusEmoji": showStatusEmoji,
            "useAsciiIcons": useAsciiIcons,
        ] as [String: Any]

        var ai = config["ai"] as? [String: Any] ?? [:]
        ai["defaultCLI"] = defaultCLI
        config["ai"] = ai

        var terminal = config["terminal"] as? [String: Any] ?? [:]
        terminal["defaultCWD"] = defaultCWD
        terminal["scrollbackLines"] = scrollbackLines
        terminal["cursorStyle"] = cursorStyle
        config["terminal"] = terminal

        config["session"] = [
            "autoRestore": autoRestoreSessions,
            "autoRestartDead": autoRestartDead,
            "maxRestartAttempts": maxRestartAttempts,
        ] as [String: Any]

        config["sidebar"] = [
            "showTaskBoard": showTaskBoard,
        ] as [String: Any]

        var shell = config["shell"] as? [String: Any] ?? [:]
        shell["default"] = shellDefault
        config["shell"] = shell

        var tailscale = config["tailscale"] as? [String: Any] ?? [:]
        tailscale["connect_on_launch"] = tailscaleConnectOnLaunch
        config["tailscale"] = tailscale

        config["cli_defaults"] = cliDefaults

        config["orchestrator"] = [
            "cli": orchestratorCLI,
            "cwd": effectiveOrchestratorCWD,
            "args": orchestratorArgs,
            "name": orchestratorName,
        ] as [String: Any]

        do {
            let dir = URL(fileURLWithPath: configPath).deletingLastPathComponent()
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: config, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: URL(fileURLWithPath: configPath))
        } catch {
            NSLog("[aterm] settings save failed: %@", error.localizedDescription)
        }
    }
}

// MARK: - Settings Window

enum SettingsTab: CaseIterable {
    case appearance
    case terminal
    case session
    case orchestrator

    var title: String {
        switch self {
        case .appearance: return "Appearance"
        case .terminal: return "Terminal"
        case .session: return "Session"
        case .orchestrator: return "Orchestrator"
        }
    }

    var icon: String {
        switch self {
        case .appearance: return "paintbrush"
        case .terminal: return "terminal"
        case .session: return "arrow.clockwise"
        case .orchestrator: return "cpu"
        }
    }
}

struct SettingsView: View {
    @ObservedObject var settings: AtermSettings
    let onApply: () -> Void
    var onApplyOrchestrator: (() -> Void)? = nil
    var isOrchestratorRunning: Bool = false
    @State private var selectedTab: SettingsTab = .appearance
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            // Tab bar
            HStack(spacing: 0) {
                ForEach(SettingsTab.allCases, id: \.self) { tab in
                    Button(action: { selectedTab = tab }) {
                        VStack(spacing: 4) {
                            Image(systemName: tab.icon)
                                .font(.system(size: 16))
                            Text(tab.title)
                                .font(.system(size: 11))
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        .background(
                            selectedTab == tab
                                ? Color(nsColor: AtermTheme.accentSubtle)
                                : Color.clear
                        )
                        .foregroundColor(
                            selectedTab == tab
                                ? Color(nsColor: AtermTheme.accent)
                                : Color(nsColor: AtermTheme.textSecondary)
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 12)
            .padding(.top, 12)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 16)

            // Tab content
            ScrollView {
                switch selectedTab {
                case .appearance:
                    AppearanceSettingsView(settings: settings, onApply: onApply)
                case .terminal:
                    TerminalSettingsView(settings: settings)
                case .session:
                    SessionSettingsView(settings: settings)
                case .orchestrator:
                    OrchestratorSettingsView(
                        settings: settings,
                        isRunning: isOrchestratorRunning,
                        onApply: {
                            settings.save()
                            onApplyOrchestrator?()
                        }
                    )
                }
            }
            .frame(maxHeight: .infinity)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 16)

            // Bottom bar
            HStack {
                Button("Reset to Defaults") {
                    resetDefaults()
                }
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Spacer()
                Button("Done") {
                    settings.save()
                    onApply()
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 480, height: 520)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }

    private func resetDefaults() {
        settings.colorScheme = "Default"
        settings.fontSize = 18
        settings.fontFamily = "System Default"
        settings.lineHeight = 1.4
        settings.backgroundOpacity = 100
        settings.showStatusEmoji = false
        settings.useAsciiIcons = false
        settings.scrollbackLines = 10000
        settings.cursorStyle = "block"
        settings.shellDefault = "zsh"
        settings.autoRestoreSessions = true
        settings.autoRestartDead = false
        settings.maxRestartAttempts = 3
        settings.showTaskBoard = true
        settings.tailscaleConnectOnLaunch = false
        settings.orchestratorCLI = "claude"
        settings.orchestratorArgs = ""
        settings.orchestratorName = "orchestrator"
        settings.orchestratorCWD = ""
        settings.cliDefaults = AtermSettings.defaultCliArgs
        settings.save()
        onApply()
    }
}

// MARK: - Appearance Tab

struct AppearanceSettingsView: View {
    @ObservedObject var settings: AtermSettings
    let onApply: () -> Void
    @State private var showAllFonts: Bool = false
    @State private var variantAvailability: FontVariantAvailability = .allPresent

    private var fontFamilies: [String] {
        showAllFonts ? AtermSettings.allFontFamilies : AtermSettings.monospacedFontFamilies
    }

    private var variantBadgeMessage: String {
        let missing = variantAvailability.missing()
        if missing.isEmpty { return "" }
        let joined = missing.joined(separator: ", ")
        return "Missing: \(joined)"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Color Scheme")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            LazyVGrid(columns: [GridItem(.adaptive(minimum: 100), spacing: 8)], spacing: 8) {
                ForEach(AtermSettings.colorSchemes, id: \.self) { scheme in
                    SchemeButton(
                        name: scheme,
                        isSelected: settings.colorScheme == scheme,
                        action: {
                            settings.colorScheme = scheme
                            settings.save()
                            onApply()
                        }
                    )
                }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Font")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack {
                Text("Family")
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Picker("", selection: $settings.fontFamily) {
                    Text("System Default")
                        .tag("System Default")
                    Divider()
                    ForEach(fontFamilies, id: \.self) { family in
                        Text(family).tag(family)
                    }
                }
                .labelsHidden()
                .onChange(of: settings.fontFamily) { _ in
                    settings.save()
                    onApply()
                    variantAvailability = AtermSettings.detectFontVariants(family: settings.fontFamily)
                }
            }

            // Lazy variant badge (Q-font-4 C2): rendered only when selected family
            // is missing one or more of Bold / Italic / Bold Italic.
            if !variantAvailability.allPresent && settings.fontFamily != "System Default" {
                HStack(alignment: .center, spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.statusWarning))
                    Text(variantBadgeMessage)
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer()
                }
                .padding(.leading, 80)
            }

            HStack {
                Spacer()
                Toggle(
                    "Show all fonts",
                    isOn: $showAllFonts
                )
                .toggleStyle(.checkbox)
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))
            }

            HStack {
                Text("Size")
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.fontSize, in: 12...24, step: 1) {
                    EmptyView()
                }
                .onChange(of: settings.fontSize) { _ in
                    settings.save()
                    onApply()
                }
                Text("\(Int(settings.fontSize)) px")
                    .frame(width: 44, alignment: .trailing)
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    .monospacedDigit()
            }

            HStack {
                Text("Line Height")
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.lineHeight, in: 1.0...2.0, step: 0.1) {
                    EmptyView()
                }
                .onChange(of: settings.lineHeight) { _ in
                    settings.save()
                    onApply()
                }
                Text(String(format: "%.1f", settings.lineHeight))
                    .frame(width: 44, alignment: .trailing)
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    .monospacedDigit()
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            HStack {
                Text("Opacity")
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.backgroundOpacity, in: 50...100, step: 5) {
                    EmptyView()
                }
                .onChange(of: settings.backgroundOpacity) { _ in
                    settings.save()
                    onApply()
                }
                Text("\(Int(settings.backgroundOpacity))%")
                    .frame(width: 44, alignment: .trailing)
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    .monospacedDigit()
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Toggle("Show Status Emojis", isOn: $settings.showStatusEmoji)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.showStatusEmoji) { _ in settings.save() }
            
            Toggle("Use ASCII Icons", isOn: $settings.useAsciiIcons)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .disabled(!settings.showStatusEmoji)
                .opacity(settings.showStatusEmoji ? 1.0 : 0.5)
                .onChange(of: settings.useAsciiIcons) { _ in settings.save() }
        }
        .padding(20)
        .onAppear {
            variantAvailability = AtermSettings.detectFontVariants(family: settings.fontFamily)
        }
    }
}

struct SchemeButton: View {
    let name: String
    let isSelected: Bool
    let action: () -> Void

    private var previewColors: (bg: Color, fg: Color) {
        switch name {
        case "Dark": return (Color(nsColor: .atermHex(0x0D1117)), Color(nsColor: .atermHex(0xC9D1D9)))
        case "Light": return (Color(nsColor: .atermHex(0xFAF6F0)), Color(nsColor: .atermHex(0x24292F)))
        case "Solarized Dark": return (Color(nsColor: .atermHex(0x002B36)), Color(nsColor: .atermHex(0x839496)))
        case "Solarized Light": return (Color(nsColor: .atermHex(0xFDF6E3)), Color(nsColor: .atermHex(0x657B83)))
        case "Monokai": return (Color(nsColor: .atermHex(0x272822)), Color(nsColor: .atermHex(0xF8F8F2)))
        case "Dracula": return (Color(nsColor: .atermHex(0x282A36)), Color(nsColor: .atermHex(0xF8F8F2)))
        case "Nord": return (Color(nsColor: .atermHex(0x2E3440)), Color(nsColor: .atermHex(0xD8DEE9)))
        case "Tokyo Night": return (Color(nsColor: .atermHex(0x1A1B26)), Color(nsColor: .atermHex(0xA9B1D6)))
        default: return (Color.black, Color.white)
        }
    }

    var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                RoundedRectangle(cornerRadius: 6)
                    .fill(previewColors.bg)
                    .frame(height: 32)
                    .overlay(
                        Text("Aa")
                            .font(.system(size: 14, weight: .medium, design: .monospaced))
                            .foregroundColor(previewColors.fg)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(
                                isSelected
                                    ? Color(nsColor: AtermTheme.accent)
                                    : Color(nsColor: AtermTheme.border),
                                lineWidth: isSelected ? 2 : 1
                            )
                    )

                Text(name)
                    .font(.system(size: 10))
                    .foregroundColor(
                        isSelected
                            ? Color(nsColor: AtermTheme.accent)
                            : Color(nsColor: AtermTheme.textSecondary)
                    )
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Terminal Tab

struct TerminalSettingsView: View {
    @ObservedObject var settings: AtermSettings
    @State private var shellMode: String = "zsh"

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Default CLI")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $settings.defaultCLI) {
                Text("Claude").tag("claude")
                Text("Codex").tag("codex")
                Text("Gemini").tag("gemini")
                Text("Shell").tag("none")
            }
            .pickerStyle(.segmented)
            .onChange(of: settings.defaultCLI) { _ in settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Default CLI Arguments")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Text("Pre-filled when creating a new workspace")
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))

            ForEach(["claude", "codex", "gemini"], id: \.self) { cli in
                HStack {
                    Text(cli)
                        .font(.system(size: 12, weight: .medium, design: .monospaced))
                        .frame(width: 60, alignment: .leading)
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    TextField(
                        AtermSettings.defaultCliArgs[cli] ?? "",
                        text: Binding(
                            get: { settings.cliDefaults[cli] ?? "" },
                            set: { settings.cliDefaults[cli] = $0 }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 11, design: .monospaced))
                    .onChange(of: settings.cliDefaults[cli]) { _ in settings.save() }
                }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Default Working Directory")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack {
                Text(settings.defaultCWD.isEmpty ? "Home (~)" : settings.defaultCWD)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)

                Button("Choose...") {
                    let panel = NSOpenPanel()
                    panel.canChooseDirectories = true
                    panel.canChooseFiles = false
                    panel.allowsMultipleSelection = false
                    if panel.runModal() == .OK, let url = panel.url {
                        settings.defaultCWD = url.path
                        settings.save()
                    }
                }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            HStack {
                Text("Scrollback Lines")
                    .frame(width: 120, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                TextField("", value: $settings.scrollbackLines, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 100)
                    .onChange(of: settings.scrollbackLines) { _ in settings.save() }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Cursor Style")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $settings.cursorStyle) {
                Text("Block").tag("block")
                Text("Underline").tag("underline")
                Text("Bar").tag("bar")
            }
            .pickerStyle(.segmented)
            .onChange(of: settings.cursorStyle) { _ in settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Shell")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $shellMode) {
                Text("zsh").tag("zsh")
                Text("bash").tag("bash")
                Text("fish").tag("fish")
                Text("Other...").tag("other")
            }
            .pickerStyle(.segmented)
            .onAppear {
                shellMode = AtermSettings.shellPresets.contains(settings.shellDefault)
                    ? settings.shellDefault
                    : "other"
            }
            .onChange(of: shellMode) { _ in
                if shellMode == "other" {
                    if AtermSettings.shellPresets.contains(settings.shellDefault) {
                        settings.shellDefault = ""
                        settings.save()
                    }
                } else {
                    settings.shellDefault = shellMode
                    settings.save()
                }
            }

            if shellMode == "other" {
                TextField(
                    "Custom shell path (e.g. /bin/nu)",
                    text: $settings.shellDefault
                )
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 11, design: .monospaced))
                .onChange(of: settings.shellDefault) { _ in settings.save() }
            }
        }
        .padding(20)
    }
}

// MARK: - Session Tab

struct SessionSettingsView: View {
    @ObservedObject var settings: AtermSettings

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Session Management")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Toggle("Auto-restore sessions on launch", isOn: $settings.autoRestoreSessions)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.autoRestoreSessions) { _ in settings.save() }

            Toggle("Auto-restart dead sessions", isOn: $settings.autoRestartDead)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.autoRestartDead) { _ in settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            HStack {
                Text("Max Restart Attempts")
                    .frame(width: 160, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                TextField("", value: $settings.maxRestartAttempts, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 60)
                    .onChange(of: settings.maxRestartAttempts) { _ in settings.save() }
            }
            .opacity(settings.autoRestartDead ? 1.0 : 0.4)
            .disabled(!settings.autoRestartDead)

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Sidebar")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Toggle("Show Task Board", isOn: $settings.showTaskBoard)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.showTaskBoard) { _ in settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Integrations")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Toggle(
                "Connect Tailscale on launch",
                isOn: $settings.tailscaleConnectOnLaunch
            )
            .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
            .onChange(of: settings.tailscaleConnectOnLaunch) { _ in settings.save() }

            Text("Takes effect on next app launch")
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))
        }
        .padding(20)
    }
}

// MARK: - Orchestrator Tab

struct OrchestratorSettingsView: View {
    @ObservedObject var settings: AtermSettings
    let isRunning: Bool
    let onApply: () -> Void

    private var isTrusted: Bool {
        let cwd = settings.effectiveOrchestratorCWD
        let encoded = cwd.replacingOccurrences(of: "/", with: "-")
        let trustDir = NSHomeDirectory() + "/.claude/projects/" + encoded
        return FileManager.default.fileExists(atPath: trustDir)
    }

    private var orchestratorArgsPlaceholder: String {
        if settings.orchestratorCLI == "custom" {
            return "e.g. my-cli --flag"
        }
        return settings.cliDefaults[settings.orchestratorCLI]
            ?? AtermSettings.defaultCliArgs[settings.orchestratorCLI]
            ?? ""
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Status
            HStack(spacing: 8) {
                Circle()
                    .fill(isRunning ? Color.green : Color(nsColor: AtermTheme.textMuted))
                    .frame(width: 8, height: 8)
                Text(isRunning
                    ? "Running"
                    : "Stopped")
                    .font(.system(size: 12))
                    .foregroundColor(isRunning
                        ? Color.green
                        : Color(nsColor: AtermTheme.textMuted))
                Spacer()
                if settings.orchestratorCLI == "claude" {
                    HStack(spacing: 4) {
                        Image(systemName: isTrusted ? "checkmark.shield.fill" : "shield.slash")
                            .font(.system(size: 11))
                        Text(isTrusted
                            ? "Trusted"
                            : "Not Trusted")
                            .font(.system(size: 11))
                    }
                    .foregroundColor(isTrusted
                        ? Color.green
                        : Color(nsColor: AtermTheme.statusWarning))
                }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            // CLI picker
            Text("CLI")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $settings.orchestratorCLI) {
                Text("Claude").tag("claude")
                Text("Codex").tag("codex")
                Text("Gemini").tag("gemini")
                Text("Custom").tag("custom")
            }
            .pickerStyle(.segmented)

            Divider().overlay(Color(nsColor: AtermTheme.border))

            // Name
            Text("Workspace Name")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            TextField(
                "Name",
                text: $settings.orchestratorName
            )
            .textFieldStyle(.roundedBorder)

            Divider().overlay(Color(nsColor: AtermTheme.border))

            // Working directory
            Text("Working Directory")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack(spacing: 8) {
                Text(settings.effectiveOrchestratorCWD)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if settings.orchestratorCWD.isEmpty {
                    Text("Default")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundColor(Color(nsColor: AtermTheme.accent))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(nsColor: AtermTheme.accentSubtle))
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }

                Button("Choose...") {
                    let panel = NSOpenPanel()
                    panel.canChooseDirectories = true
                    panel.canChooseFiles = false
                    panel.allowsMultipleSelection = false
                    panel.prompt = "Select"
                    if panel.runModal() == .OK, let url = panel.url {
                        settings.orchestratorCWD = url.path
                    }
                }

                if !settings.orchestratorCWD.isEmpty {
                    Button("Reset") {
                        settings.orchestratorCWD = ""
                    }
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            // Data folder
            Text("Data Folder")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack(spacing: 8) {
                Text(settings.aigentryRoot)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if settings.aigentryRoot == NSHomeDirectory() + "/.aigentry" {
                    Text("Default")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundColor(Color(nsColor: AtermTheme.accent))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(nsColor: AtermTheme.accentSubtle))
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }

                Button("Change...") {
                    let panel = NSOpenPanel()
                    panel.canChooseDirectories = true
                    panel.canChooseFiles = false
                    panel.allowsMultipleSelection = false
                    panel.prompt = "Select"
                    if panel.runModal() == .OK, let url = panel.url {
                        let newRoot = url.path + "/.aigentry"
                        if settings.migrateAigentryRoot(to: newRoot) {
                            settings.save()
                            onApply()
                        }
                    }
                }
            }

            Text("Where orchestrator, settings, and session data are stored.")
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))

            // Args (visible for all CLIs — override mode)
            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text("Args (optional override)")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            TextField(
                orchestratorArgsPlaceholder,
                text: $settings.orchestratorArgs
            )
            .textFieldStyle(.roundedBorder)
            .font(.system(size: 11, design: .monospaced))
            .onChange(of: settings.orchestratorArgs) { _ in settings.save() }

            Text("Leave empty to use the default above. Any value fully overrides it.")
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))

            Divider().overlay(Color(nsColor: AtermTheme.border))

            // Apply button
            HStack {
                Spacer()
                Button(action: onApply) {
                    Text(isRunning
                        ? "Apply & Restart"
                        : "Apply & Start")
                        .frame(minWidth: 120)
                }
                .controlSize(.large)
            }

            Text("Starts or restarts the orchestrator workspace with the new configuration.")
                .font(.system(size: 10))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))
        }
        .padding(20)
    }
}
