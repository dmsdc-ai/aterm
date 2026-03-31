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
        // Auto-select first available CLI
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
            // Header
            VStack(spacing: 8) {
                Text("aterm")
                    .font(.system(size: 36, weight: .bold, design: .monospaced))
                    .foregroundColor(.cyan)
                Text("AI Development Runtime")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundColor(.gray)
            }
            .padding(.top, 32)
            .padding(.bottom, 24)

            Divider().padding(.horizontal, 24)

            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    // AI CLI Selection
                    settingSection(title: "Choose your AI assistant", step: 1) {
                        VStack(spacing: 6) {
                            cliOption("claude", label: "Claude Code", desc: "Anthropic", installed: cliStatus.claude)
                            cliOption("codex", label: "Codex CLI", desc: "OpenAI", installed: cliStatus.codex)
                            cliOption("gemini", label: "Gemini CLI", desc: "Google", installed: cliStatus.gemini)
                            cliOption("none", label: "None", desc: "plain terminal", installed: true)
                        }
                    }

                    // Shell Selection
                    settingSection(title: "Default shell", step: 2) {
                        HStack(spacing: 8) {
                            ForEach(["zsh", "bash", "fish"], id: \.self) { shell in
                                Button(action: { selectedShell = shell }) {
                                    Text(shell)
                                        .font(.system(size: 13, weight: .medium))
                                        .foregroundColor(.white)
                                        .padding(.horizontal, 16)
                                        .padding(.vertical, 8)
                                        .background(selectedShell == shell ? Color.cyan : Color.white.opacity(0.1))
                                        .cornerRadius(8)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }

                    // Tailscale
                    settingSection(title: "Connect to other machines?", step: 3) {
                        Toggle(isOn: $tailscaleEnabled) {
                            Text("Tailscale networking")
                                .foregroundColor(.white.opacity(0.85))
                        }
                        .toggleStyle(.switch)
                    }
                }
                .padding(24)
            }

            Divider().padding(.horizontal, 24)

            // Done button
            HStack {
                Spacer()
                Button(action: complete) {
                    Text("Get Started")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(.white)
                        .padding(.horizontal, 24)
                        .padding(.vertical, 8)
                        .background(Color.cyan)
                        .cornerRadius(8)
                }
                .buttonStyle(.plain)
                Spacer()
            }
            .padding(.vertical, 16)
        }
        .frame(width: 420, height: 600)
        .background(Color(nsColor: NSColor(white: 0.12, alpha: 1.0)))
    }

    private func cliOption(_ value: String, label: String, desc: String, installed: Bool) -> some View {
        let isSelected = selectedCLI == value
        return Button(action: {
            if installed { selectedCLI = value }
        }) {
            HStack(spacing: 10) {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundColor(isSelected ? .cyan : .gray)
                    .font(.system(size: 16))
                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundColor(installed ? .white : .white.opacity(0.4))
                    Text(desc)
                        .font(.system(size: 11))
                        .foregroundColor(.white.opacity(0.5))
                }
                Spacer()
                if value != "none" {
                    Text(installed ? "installed" : "not found")
                        .font(.system(size: 10))
                        .foregroundColor(installed ? .green : .white.opacity(0.3))
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(isSelected ? Color.white.opacity(0.08) : Color.clear)
            .cornerRadius(6)
        }
        .buttonStyle(.plain)
        .disabled(!installed)
    }

    private func settingSection<Content: View>(title: String, step: Int, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text("\(step)")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundColor(.black)
                    .frame(width: 18, height: 18)
                    .background(Color.cyan)
                    .clipShape(Circle())
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(.white)
            }
            content()
        }
    }

    private func complete() {
        let result = OnboardingResult(
            defaultCLI: selectedCLI,
            defaultShell: selectedShell,
            tailscaleEnabled: tailscaleEnabled
        )
        onComplete(result)
        isPresented = false
    }
}

struct OnboardingResult {
    let defaultCLI: String
    let defaultShell: String
    let tailscaleEnabled: Bool
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
                .foregroundColor(.white)
                .padding(.top, 20)
                .padding(.bottom, 16)

            Divider().padding(.horizontal, 20)

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

            Divider().padding(.horizontal, 20)

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Save") {
                    onSave(OnboardingResult(
                        defaultCLI: selectedCLI,
                        defaultShell: selectedShell,
                        tailscaleEnabled: tailscaleEnabled
                    ))
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 380, height: 400)
        .background(Color(nsColor: NSColor(white: 0.12, alpha: 1.0)))
    }
}
