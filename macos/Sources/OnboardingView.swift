import SwiftUI

struct OnboardingView: View {
    @Binding var isPresented: Bool
    @State private var cliStatus: CliStatus
    let onRefreshClis: () -> CliStatus
    let onComplete: (OnboardingResult) -> Void

    @State private var currentStep = 0
    @State private var selectedCLI: String
    @State private var cliArgs: String
    @State private var selectedShell: String = "zsh"
    @State private var tailscaleEnabled: Bool = false
    @State private var orchestratorCWD: String = ""

    private let totalSteps = 4

    init(isPresented: Binding<Bool>, cliStatus: CliStatus, onRefreshClis: @escaping () -> CliStatus, onComplete: @escaping (OnboardingResult) -> Void) {
        self._isPresented = isPresented
        self._cliStatus = State(initialValue: cliStatus)
        self.onRefreshClis = onRefreshClis
        self.onComplete = onComplete
        let initialCLI: String
        if cliStatus.claude {
            initialCLI = "claude"
        } else if cliStatus.codex {
            initialCLI = "codex"
        } else if cliStatus.gemini {
            initialCLI = "gemini"
        } else {
            initialCLI = "none"
        }
        _selectedCLI = State(initialValue: initialCLI)
        _cliArgs = State(initialValue: AtermSettings.shared.cliDefaults[initialCLI] ?? "")
    }

    var body: some View {
        VStack(spacing: 0) {
            // Progress dots
            HStack(spacing: 6) {
                ForEach(0..<totalSteps, id: \.self) { step in
                    Circle()
                        .fill(step == currentStep
                            ? Color(nsColor: AtermTheme.accent)
                            : Color(nsColor: AtermTheme.textMuted).opacity(0.3))
                        .frame(width: 8, height: 8)
                }
            }
            .padding(.top, 24)
            .padding(.bottom, 16)

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 24)

            // Step content
            ScrollView {
                Group {
                    switch currentStep {
                    case 0: stepWelcome
                    case 1: stepWhatIsOrchestrator
                    case 2: stepChooseCLI
                    case 3: stepGetStarted
                    default: EmptyView()
                    }
                }
                .padding(24)
            }

            Divider().overlay(Color(nsColor: AtermTheme.border)).padding(.horizontal, 24)

            // Navigation
            HStack {
                if currentStep > 0 {
                    Button(action: { withAnimation { currentStep -= 1 } }) {
                        Text("Back")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    }
                    .buttonStyle(.plain)
                }
                Spacer()
                Button(action: advance) {
                    Text(currentStep == totalSteps - 1 ? "Get Started" : "Next")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(Color(nsColor: AtermTheme.panelBackground))
                        .padding(.horizontal, 24)
                        .padding(.vertical, 8)
                        .background(Color(nsColor: AtermTheme.accent))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 16)
        }
        .frame(width: 480, height: 560)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }

    // MARK: - Step 1: Welcome

    private var stepWelcome: some View {
        VStack(spacing: 20) {
            Spacer().frame(height: 24)
            Text("aterm")
                .font(.system(size: 42, weight: .bold, design: .monospaced))
                .foregroundColor(Color(nsColor: AtermTheme.accentStrong))
            Text("AI Development Runtime")
                .font(.system(size: 16, weight: .medium))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
            Spacer().frame(height: 12)
            Text("aterm uses an Orchestrator to manage your AI workflow.")
                .font(.system(size: 14))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                .multilineTextAlignment(.center)
            Text("Let's set it up.")
                .font(.system(size: 14))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .multilineTextAlignment(.center)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Step 2: What is Orchestrator?

    private var stepWhatIsOrchestrator: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("What is the Orchestrator?")
                .font(.system(size: 20, weight: .bold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            tutorialCard(
                icon: "command.circle.fill",
                title: "Your AI command center",
                desc: "The Orchestrator is a persistent AI session that runs as long as aterm is open."
            )
            tutorialCard(
                icon: "arrow.triangle.branch",
                title: "Delegates tasks",
                desc: "It breaks down complex work into subtasks and assigns them to specialized sessions."
            )
            tutorialCard(
                icon: "list.clipboard.fill",
                title: "Tracks progress",
                desc: "Think of it as your AI project manager — it knows what's done and what's next."
            )

            Text("You can always add more workspaces for hands-on coding. The Orchestrator coordinates them all.")
                .font(.system(size: 12))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .padding(.top, 4)
        }
    }

    // MARK: - Step 3: Choose CLI

    private var stepChooseCLI: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Choose your Orchestrator CLI")
                    .font(.system(size: 20, weight: .bold))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                Spacer()
                Button(action: {
                    cliStatus = onRefreshClis()
                }) {
                    HStack(spacing: 4) {
                        Image(systemName: "arrow.clockwise")
                            .font(.system(size: 11))
                        Text("Refresh")
                            .font(.system(size: 11, weight: .medium))
                    }
                    .foregroundColor(Color(nsColor: AtermTheme.accent))
                }
                .buttonStyle(.plain)
            }

            Text("Which AI CLI should power your Orchestrator?")
                .font(.system(size: 13))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))

            VStack(spacing: 6) {
                cliOption("claude", label: "Claude Code", desc: "Anthropic — architecture, debugging, MCP", installed: cliStatus.claude)
                cliOption("codex", label: "Codex CLI", desc: "OpenAI — code generation, testing", installed: cliStatus.codex)
                cliOption("gemini", label: "Gemini CLI", desc: "Google — web search, documentation", installed: cliStatus.gemini)
                cliOption("none", label: "None", desc: "plain terminal", installed: true)
            }
            .onChange(of: selectedCLI) {
                cliArgs = AtermSettings.shared.cliDefaults[selectedCLI] ?? ""
            }

            if selectedCLI != "none" {
                VStack(alignment: .leading, spacing: 4) {
                    Text("CLI Arguments")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    TextField(
                        AtermSettings.defaultCliArgs[selectedCLI] ?? "",
                        text: $cliArgs
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12, design: .monospaced))
                }
                .padding(.top, 4)
            }
        }
    }

    // MARK: - Step 4: Get Started

    private var stepGetStarted: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("You're all set!")
                .font(.system(size: 20, weight: .bold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Image(systemName: "folder.fill")
                        .font(.system(size: 14))
                        .foregroundColor(Color(nsColor: AtermTheme.accent))
                        .frame(width: 20)
                    Text("Orchestrator Working Directory")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                }
                HStack(spacing: 8) {
                    Text(orchestratorCWD.isEmpty ? "~/.aigentry/orchestrator/" : orchestratorCWD)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Button("Change...") {
                        let panel = NSOpenPanel()
                        panel.canChooseDirectories = true
                        panel.canChooseFiles = false
                        panel.allowsMultipleSelection = false
                        if panel.runModal() == .OK, let url = panel.url {
                            orchestratorCWD = url.path
                        }
                    }
                    .controlSize(.small)
                }
            }
            infoRow(
                icon: "plus.circle.fill",
                text: "Add more workspaces later with the + button in the sidebar."
            )
            infoRow(
                icon: "gearshape.fill",
                text: "Settings (\u{2318},) lets you change the Orchestrator CLI anytime."
            )

            HStack(spacing: 16) {
                settingPill(title: "Shell", value: selectedShell, options: ["zsh", "bash", "fish"], selection: $selectedShell)
                Toggle(isOn: $tailscaleEnabled) {
                    Text("Tailscale")
                        .font(.system(size: 12))
                        .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                }
                .toggleStyle(.switch)
                .controlSize(.small)
            }
            .padding(.top, 8)

            Spacer()
        }
    }

    // MARK: - Helpers

    private func tutorialCard(icon: String, title: String, desc: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20))
                .foregroundColor(Color(nsColor: AtermTheme.accent))
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                Text(desc)
                    .font(.system(size: 12))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: AtermTheme.panelInsetBackground))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    private func infoRow(icon: String, text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 14))
                .foregroundColor(Color(nsColor: AtermTheme.accent))
                .frame(width: 20)
            Text(text)
                .font(.system(size: 13))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
        }
    }

    private func settingPill(title: String, value: String, options: [String], selection: Binding<String>) -> some View {
        HStack(spacing: 4) {
            Text(title)
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
            Picker("", selection: selection) {
                ForEach(options, id: \.self) { opt in
                    Text(opt).tag(opt)
                }
            }
            .labelsHidden()
            .controlSize(.small)
        }
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

    private func advance() {
        if currentStep < totalSteps - 1 {
            let nextStep = currentStep + 1
            // GAP 6: Re-detect CLIs when entering the CLI selection step
            if nextStep == 2 {
                cliStatus = onRefreshClis()
            }
            withAnimation { currentStep = nextStep }
        } else {
            complete()
        }
    }

    private func complete() {
        if selectedCLI != "none" {
            let args = cliArgs.trimmingCharacters(in: .whitespacesAndNewlines)
            if !args.isEmpty {
                AtermSettings.shared.cliDefaults[selectedCLI] = args
            }
            AtermSettings.shared.save()
        }

        let result = OnboardingResult(
            defaultCLI: selectedCLI,
            defaultShell: selectedShell,
            tailscaleEnabled: tailscaleEnabled,
            orchestratorCLI: selectedCLI,
            orchestratorCWD: orchestratorCWD
        )
        onComplete(result)
        isPresented = false
    }
}

struct OnboardingResult {
    let defaultCLI: String
    let defaultShell: String
    let tailscaleEnabled: Bool
    let orchestratorCLI: String
    let orchestratorCWD: String
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
    @State private var orchestratorCLI: String
    @State private var orchestratorCWD: String
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
        let orchestrator = currentConfig["orchestrator"] as? [String: Any]
        _orchestratorCLI = State(initialValue: orchestrator?["cli"] as? String ?? ai?["defaultCLI"] as? String ?? "none")
        _orchestratorCWD = State(initialValue: orchestrator?["cwd"] as? String ?? "")
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

                Section("Orchestrator") {
                    Picker("Orchestrator CLI", selection: $orchestratorCLI) {
                        Text("Claude").tag("claude")
                        Text("Codex").tag("codex")
                        Text("Gemini").tag("gemini")
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
                        orchestratorCLI: orchestratorCLI,
                        orchestratorCWD: orchestratorCWD
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
