import SwiftUI

struct OnboardingView: View {
    @Binding var isPresented: Bool
    let cliStatus: CliStatus
    let onComplete: (OnboardingResult) -> Void

    @State private var selectedCLI: String
    @State private var selectedShell: String = "zsh"
    @State private var tailscaleEnabled: Bool = false

    init(isPresented: Binding<Bool>, cliStatus: CliStatus, onComplete: @escaping (OnboardingResult) -> Void) {
        self._isPresented = isPresented
        self.cliStatus = cliStatus
        self.onComplete = onComplete
        if cliStatus.claude {
            _selectedCLI = State(initialValue: "claude")
        } else if cliStatus.codex {
            _selectedCLI = State(initialValue: "codex")
        } else if cliStatus.gemini {
            _selectedCLI = State(initialValue: "gemini")
        } else {
            _selectedCLI = State(initialValue: "none")
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 8) {
                Text("aterm")
                    .font(.system(size: 36, weight: .bold, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.accentStrong))
                Text("AI Development Runtime")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
            }
            .padding(.top, 32)
            .padding(.bottom, 24)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 24)

            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    settingSection(title: "Choose your AI assistant", step: 1) {
                        VStack(spacing: 6) {
                            cliOption("claude", label: "Claude Code", desc: "Anthropic", installed: cliStatus.claude)
                            cliOption("codex", label: "Codex CLI", desc: "OpenAI", installed: cliStatus.codex)
                            cliOption("gemini", label: "Gemini CLI", desc: "Google", installed: cliStatus.gemini)
                            cliOption("none", label: "None", desc: "plain terminal", installed: true)
                        }
                    }

                    settingSection(title: "Project folder", step: 2) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("After this step, aterm asks for the first workspace folder.")
                                .font(.system(size: 13, weight: .medium))
                                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                            Text("Skip the picker to keep the home directory behavior.")
                                .font(.system(size: 11))
                                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                        }
                    }

                    settingSection(title: "Default shell", step: 3) {
                        HStack(spacing: 8) {
                            ForEach(["zsh", "bash", "fish"], id: \.self) { shell in
                                Button(action: { selectedShell = shell }) {
                                    Text(shell)
                                        .font(.system(size: 13, weight: .medium))
                                        .foregroundColor(
                                            selectedShell == shell
                                                ? Color(nsColor: AtermTheme.panelBackground)
                                                : Color(nsColor: AtermTheme.textPrimary)
                                        )
                                        .padding(.horizontal, 16)
                                        .padding(.vertical, 8)
                                        .background(
                                            selectedShell == shell
                                                ? Color(nsColor: AtermTheme.accent)
                                                : Color(nsColor: AtermTheme.panelInsetBackground)
                                        )
                                        .clipShape(RoundedRectangle(cornerRadius: 8))
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }

                    settingSection(title: "Connect to other machines?", step: 4) {
                        Toggle(isOn: $tailscaleEnabled) {
                            Text("Tailscale")
                                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                        }
                        .toggleStyle(.switch)
                    }
                }
                .padding(24)
            }

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 24)

            HStack {
                Spacer()
                Button(action: complete) {
                    Text("Choose Project Folder")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(Color(nsColor: AtermTheme.panelBackground))
                        .padding(.horizontal, 24)
                        .padding(.vertical, 8)
                        .background(Color(nsColor: AtermTheme.accent))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                Spacer()
            }
            .padding(.vertical, 16)
        }
        .frame(width: 440, height: 720)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }

    private func cliOption(_ value: String, label: String, desc: String, installed: Bool) -> some View {
        let isSelected = selectedCLI == value
        return Button(action: {
            if installed { selectedCLI = value }
        }) {
            HStack(spacing: 10) {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundColor(
                        isSelected
                            ? Color(nsColor: AtermTheme.accent)
                            : Color(nsColor: AtermTheme.textMuted)
                    )
                    .font(.system(size: 16))
                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundColor(
                            installed
                                ? Color(nsColor: AtermTheme.textPrimary)
                                : Color(nsColor: AtermTheme.textMuted)
                        )
                    Text(desc)
                        .font(.system(size: 11))
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                }
                Spacer()
                if value != "none" {
                    Text(installed ? "installed" : "not found")
                        .font(.system(size: 10))
                        .foregroundColor(
                            installed
                                ? Color(nsColor: AtermTheme.statusSuccess)
                                : Color(nsColor: AtermTheme.textMuted)
                        )
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(
                isSelected
                    ? Color(nsColor: AtermTheme.selectedRowBackground)
                    : Color(nsColor: AtermTheme.secondaryRowBackground)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(
                        isSelected
                            ? Color(nsColor: AtermTheme.selectedRowStroke)
                            : Color(nsColor: AtermTheme.border),
                        lineWidth: 1
                    )
            )
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .disabled(!installed)
    }

    private func settingSection<Content: View>(title: String, step: Int, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text("\(step)")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundColor(Color(nsColor: AtermTheme.panelBackground))
                    .frame(width: 18, height: 18)
                    .background(Color(nsColor: AtermTheme.accent))
                    .clipShape(Circle())
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
            }
            content()
        }
    }

    private func complete() {
        let result = OnboardingResult(
            defaultCLI: selectedCLI,
            defaultShell: selectedShell,
            tailscaleEnabled: tailscaleEnabled,
            initialProjectDirectory: nil
        )
        onComplete(result)
        isPresented = false
    }
}

struct OnboardingResult {
    let defaultCLI: String
    let defaultShell: String
    let tailscaleEnabled: Bool
    let initialProjectDirectory: String?
}

struct CliStatus {
    let claude: Bool
    let codex: Bool
    let gemini: Bool
}

struct PreferencesView: View {
    let cliStatus: CliStatus
    let currentConfig: [String: Any]
    let onSave: (OnboardingResult) -> Void

    @State private var selectedCLI: String
    @State private var selectedShell: String
    @State private var tailscaleEnabled: Bool
    @Environment(\.dismiss) private var dismiss

    init(cliStatus: CliStatus, currentConfig: [String: Any], onSave: @escaping (OnboardingResult) -> Void) {
        self.cliStatus = cliStatus
        self.currentConfig = currentConfig
        self.onSave = onSave

        let ai = currentConfig["ai"] as? [String: Any]
        let shell = currentConfig["shell"] as? [String: Any]
        let tailscale = currentConfig["tailscale"] as? [String: Any]

        _selectedCLI = State(initialValue: ai?["defaultCLI"] as? String ?? "none")
        _selectedShell = State(initialValue: shell?["default"] as? String ?? "zsh")
        _tailscaleEnabled = State(initialValue: tailscale?["connect_on_launch"] as? Bool ?? false)
    }

    var body: some View {
        VStack(spacing: 0) {
            Text("Preferences")
                .font(.system(size: 16, weight: .bold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                .padding(.top, 20)
                .padding(.bottom, 16)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 20)

            Form {
                Section("AI Assistant") {
                    Picker("Default CLI", selection: $selectedCLI) {
                        Text("Claude").tag("claude").disabled(!cliStatus.claude)
                        Text("Codex").tag("codex").disabled(!cliStatus.codex)
                        Text("Gemini").tag("gemini").disabled(!cliStatus.gemini)
                        Text("None").tag("none")
                    }
                }

                Section("Terminal") {
                    Picker("Default Shell", selection: $selectedShell) {
                        Text("zsh").tag("zsh")
                        Text("bash").tag("bash")
                        Text("fish").tag("fish")
                    }
                }

                Section("Network") {
                    Toggle("Tailscale mesh networking", isOn: $tailscaleEnabled)
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 20)

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Save") {
                    onSave(OnboardingResult(
                        defaultCLI: selectedCLI,
                        defaultShell: selectedShell,
                        tailscaleEnabled: tailscaleEnabled,
                        initialProjectDirectory: nil
                    ))
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 380, height: 400)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }
}
