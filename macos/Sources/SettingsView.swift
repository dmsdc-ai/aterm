import AppKit
import SwiftUI

// MARK: - Settings Model

/// Persistent settings stored in ~/.aigentry/config/aterm.json
class AtermSettings: ObservableObject {
    static let shared = AtermSettings()

    // Appearance
    @Published var colorScheme: String = "Tokyo Night"
    @Published var fontSize: Double = 18
    @Published var fontFamily: String = "System Default"
    @Published var lineHeight: Double = 1.4
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

    static let colorSchemes = [
        "Dark", "Light", "Solarized Dark", "Solarized Light",
        "Monokai", "Dracula", "Nord", "Tokyo Night",
    ]

    static let cursorStyles = ["block", "underline", "bar"]

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
        default: return 0
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
        default: return "Dark"
        }
    }

    private static let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"

    func load() {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: Self.configPath)),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }

        let appearance = config["appearance"] as? [String: Any]
        let terminal = config["terminal"] as? [String: Any]
        let session = config["session"] as? [String: Any]
        let ai = config["ai"] as? [String: Any]

        if let s = appearance?["colorScheme"] as? String { colorScheme = s }
        if let s = appearance?["fontSize"] as? Double { fontSize = s }
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
    }

    func save() {
        // Read existing config to preserve fields we don't manage
        var config: [String: Any] = [:]
        if let data = try? Data(contentsOf: URL(fileURLWithPath: Self.configPath)),
           let existing = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            config = existing
        }

        config["appearance"] = [
            "colorScheme": colorScheme,
            "fontSize": fontSize,
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

        do {
            let dir = URL(fileURLWithPath: Self.configPath).deletingLastPathComponent()
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: config, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: URL(fileURLWithPath: Self.configPath))
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

    var title: String {
        switch self {
        case .appearance: return AtermLocalization.text(ko: "모양", en: "Appearance")
        case .terminal: return AtermLocalization.text(ko: "터미널", en: "Terminal")
        case .session: return AtermLocalization.text(ko: "세션", en: "Session")
        }
    }

    var icon: String {
        switch self {
        case .appearance: return "paintbrush"
        case .terminal: return "terminal"
        case .session: return "arrow.clockwise"
        }
    }
}

struct SettingsView: View {
    @ObservedObject var settings: AtermSettings
    let onApply: () -> Void
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
                }
            }
            .frame(maxHeight: .infinity)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 16)

            // Bottom bar
            HStack {
                Button(AtermLocalization.text(ko: "기본값으로 재설정", en: "Reset to Defaults")) {
                    resetDefaults()
                }
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Spacer()
                Button(AtermLocalization.text(ko: "완료", en: "Done")) {
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
        settings.colorScheme = "Dark"
        settings.fontSize = 18
        settings.fontFamily = "System Default"
        settings.lineHeight = 1.4
        settings.backgroundOpacity = 100
        settings.showStatusEmoji = false
        settings.useAsciiIcons = false
        settings.scrollbackLines = 10000
        settings.cursorStyle = "block"
        settings.autoRestoreSessions = true
        settings.autoRestartDead = false
        settings.maxRestartAttempts = 3
        settings.showTaskBoard = true
        settings.save()
        onApply()
    }
}

// MARK: - Appearance Tab

struct AppearanceSettingsView: View {
    @ObservedObject var settings: AtermSettings
    let onApply: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(AtermLocalization.text(ko: "색상 테마", en: "Color Scheme"))
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

            Text(AtermLocalization.text(ko: "글꼴", en: "Font"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack {
                Text(AtermLocalization.text(ko: "크기", en: "Size"))
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.fontSize, in: 12...24, step: 1) {
                    EmptyView()
                }
                .onChange(of: settings.fontSize) {
                    settings.save()
                    onApply()
                }
                Text("\(Int(settings.fontSize)) px")
                    .frame(width: 44, alignment: .trailing)
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    .monospacedDigit()
            }

            HStack {
                Text(AtermLocalization.text(ko: "줄 높이", en: "Line Height"))
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.lineHeight, in: 1.0...2.0, step: 0.1) {
                    EmptyView()
                }
                .onChange(of: settings.lineHeight) {
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
                Text(AtermLocalization.text(ko: "불투명도", en: "Opacity"))
                    .frame(width: 80, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                Slider(value: $settings.backgroundOpacity, in: 50...100, step: 5) {
                    EmptyView()
                }
                .onChange(of: settings.backgroundOpacity) {
                    settings.save()
                    onApply()
                }
                Text("\(Int(settings.backgroundOpacity))%")
                    .frame(width: 44, alignment: .trailing)
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    .monospacedDigit()
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Toggle(AtermLocalization.text(ko: "상태 이모지 표시", en: "Show Status Emojis"), isOn: $settings.showStatusEmoji)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.showStatusEmoji) { settings.save() }
            
            Toggle(AtermLocalization.text(ko: "ASCII 아이콘 사용", en: "Use ASCII Icons"), isOn: $settings.useAsciiIcons)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .disabled(!settings.showStatusEmoji)
                .opacity(settings.showStatusEmoji ? 1.0 : 0.5)
                .onChange(of: settings.useAsciiIcons) { settings.save() }
        }
        .padding(20)
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
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Terminal Tab

struct TerminalSettingsView: View {
    @ObservedObject var settings: AtermSettings

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(AtermLocalization.text(ko: "기본 CLI", en: "Default CLI"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $settings.defaultCLI) {
                Text("Claude").tag("claude")
                Text("Codex").tag("codex")
                Text("Gemini").tag("gemini")
                Text(AtermLocalization.text(ko: "셸", en: "Shell")).tag("none")
            }
            .pickerStyle(.segmented)
            .onChange(of: settings.defaultCLI) { settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text(AtermLocalization.text(ko: "기본 작업 디렉터리", en: "Default Working Directory"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            HStack {
                Text(settings.defaultCWD.isEmpty ? AtermLocalization.text(ko: "홈 (~)", en: "Home (~)") : settings.defaultCWD)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)

                Button(AtermLocalization.text(ko: "선택...", en: "Choose...")) {
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
                Text(AtermLocalization.text(ko: "스크롤백 줄 수", en: "Scrollback Lines"))
                    .frame(width: 120, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                TextField("", value: $settings.scrollbackLines, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 100)
                    .onChange(of: settings.scrollbackLines) { settings.save() }
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text(AtermLocalization.text(ko: "커서 스타일", en: "Cursor Style"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Picker("", selection: $settings.cursorStyle) {
                Text(AtermLocalization.text(ko: "블록", en: "Block")).tag("block")
                Text(AtermLocalization.text(ko: "밑줄", en: "Underline")).tag("underline")
                Text(AtermLocalization.text(ko: "막대", en: "Bar")).tag("bar")
            }
            .pickerStyle(.segmented)
            .onChange(of: settings.cursorStyle) { settings.save() }
        }
        .padding(20)
    }
}

// MARK: - Session Tab

struct SessionSettingsView: View {
    @ObservedObject var settings: AtermSettings

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(AtermLocalization.text(ko: "세션 관리", en: "Session Management"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Toggle(AtermLocalization.text(ko: "실행 시 세션 자동 복원", en: "Auto-restore sessions on launch"), isOn: $settings.autoRestoreSessions)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.autoRestoreSessions) { settings.save() }

            Toggle(AtermLocalization.text(ko: "죽은 세션 자동 재시작", en: "Auto-restart dead sessions"), isOn: $settings.autoRestartDead)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.autoRestartDead) { settings.save() }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            HStack {
                Text(AtermLocalization.text(ko: "최대 재시작 횟수", en: "Max Restart Attempts"))
                    .frame(width: 160, alignment: .leading)
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                TextField("", value: $settings.maxRestartAttempts, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 60)
                    .onChange(of: settings.maxRestartAttempts) { settings.save() }
            }
            .opacity(settings.autoRestartDead ? 1.0 : 0.4)
            .disabled(!settings.autoRestartDead)

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Text(AtermLocalization.text(ko: "사이드바", en: "Sidebar"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Toggle(AtermLocalization.text(ko: "태스크 보드 표시", en: "Show Task Board"), isOn: $settings.showTaskBoard)
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .onChange(of: settings.showTaskBoard) { settings.save() }
        }
        .padding(20)
    }
}
