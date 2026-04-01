import AppKit
import Darwin
import Foundation
import SwiftUI

private struct IpcWorkspaceConfig: Decodable {
    let name: String
    let cli: String
    let cwd: String
    let cols: UInt16?
    let rows: UInt16?
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var terminalView: TerminalView?
    var busClient: TeleptyBusClient!
    var splitView: NSSplitView!
    var terminalContainerView: NSView!

    private let workspaceSidebarModel = WorkspaceSidebarModel()
    private var processPollTimer: Timer?
    private var managedWorkspaces: [UUID: ManagedWorkspace] = [:]
    private var workspaceOrder: [UUID] = []
    private var excludedChildPIDs: Set<Int32> = []
    private var sessionSaveTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        ensureTeleptyDaemon()
        startTailscale()

        let rect = NSRect(x: 0, y: 0, width: 1280, height: 768)
        window = NSWindow(
            contentRect: rect,
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "aterm v3"
        window.center()
        window.minSize = NSSize(width: 640, height: 400)
        window.backgroundColor = AtermTheme.windowBackground

        // Create telepty bus client
        busClient = TeleptyBusClient()

        // Create SwiftUI sidebar
        let sidebarView = SessionSidebarView(
            busClient: busClient,
            workspaceModel: workspaceSidebarModel,
            onBeginWorkspaceCreation: { [weak self] request in
                self?.beginWorkspaceCreation(request)
            },
            onCreateWorkspaces: { [weak self] drafts in
                self?.createWorkspaces(from: drafts)
            },
            onSelectWorkspace: { [weak self] id in
                self?.selectWorkspace(id)
            },
            onRenameWorkspace: { [weak self] id, name in
                self?.renameWorkspace(id: id, to: name)
            },
            onCloseWorkspace: { [weak self] id in
                self?.closeWorkspace(id: id)
            },
            onAttachExternalSession: { [weak self] sessionID in
                self?.attachExternalSession(sessionID)
            },
            onOpenSettings: { [weak self] in
                self?.showPreferences()
            }
        )
        let sidebarHost = NSHostingView(rootView: sidebarView)
        sidebarHost.frame = NSRect(x: 0, y: 0, width: 240, height: rect.height)

        // Create terminal container
        let terminalRect = NSRect(x: 0, y: 0, width: rect.width - 240, height: rect.height)
        terminalContainerView = NSView(frame: terminalRect)
        terminalContainerView.autoresizingMask = [.width, .height]
        terminalContainerView.wantsLayer = true
        terminalContainerView.layer?.backgroundColor = AtermTheme.terminalBackground.cgColor

        // Create split view
        splitView = NSSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        splitView.frame = rect
        splitView.autoresizingMask = [.width, .height]

        splitView.addSubview(sidebarHost)
        splitView.addSubview(terminalContainerView)

        // Set sidebar constraints
        // Keep the sidebar near its initial width and let the terminal absorb
        // horizontal growth/shrink so PTY columns track the visible content.
        splitView.setHoldingPriority(.defaultHigh, forSubviewAt: 0)
        splitView.setHoldingPriority(.defaultLow, forSubviewAt: 1)
        sidebarHost.widthAnchor.constraint(greaterThanOrEqualToConstant: 180).isActive = true
        sidebarHost.widthAnchor.constraint(lessThanOrEqualToConstant: 400).isActive = true
        splitView.setPosition(240, ofDividerAt: 0)

        window.contentView = splitView
        window.makeKeyAndOrderFront(nil)

        setupPreferencesMenu()
        AtermSettings.shared.load()

        let restoredCount = restoreWorkspaces()
        if restoredCount == 0 {
            if needsOnboarding() {
                showOnboarding()
            } else {
                createDefaultWorkspace()
            }
        } else if needsOnboarding() {
            // REINSTALL: sessions restored but config needs setup
            showOnboarding()
        }
        registerHostCallbacks()
        startProcessPolling()
        sessionSaveTimer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            self?.saveWorkspaces()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        saveWorkspaces()
        // Deregister all workspaces from telepty daemon to prevent ghost sessions
        deregisterTeleptyWorkspaces()
        sessionSaveTimer?.invalidate()
        sessionSaveTimer = nil
        processPollTimer?.invalidate()
        processPollTimer = nil
        let env = ProcessInfo.processInfo.environment
        let tailscaleDisabledEnv = env["ATERM_TAILSCALE_ENABLED"].map { $0 == "0" || $0.lowercased() == "false" || $0.lowercased() == "no" } ?? false
        let tailscaleDisabledDefaults = UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil && !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled")
        if !tailscaleDisabledEnv && !tailscaleDisabledDefaults {
            aterm_tailscale_shutdown()
        }
    }

    private func deregisterTeleptyWorkspaces() {
        let port = 3848
        for (_, workspace) in managedWorkspaces {
            let name = workspace.name
            guard !name.isEmpty,
                  let url = URL(string: "http://localhost:\(port)/api/sessions/\(name)") else { continue }
            var request = URLRequest(url: url)
            request.httpMethod = "DELETE"
            request.timeoutInterval = 2
            // Fire synchronously — we're terminating, must complete before exit
            let semaphore = DispatchSemaphore(value: 0)
            URLSession.shared.dataTask(with: request) { _, _, _ in
                semaphore.signal()
            }.resume()
            _ = semaphore.wait(timeout: .now() + 2)
            NSLog("[aterm] deregistered workspace '%@' from telepty", name)
        }
    }

    // MARK: - IPC Host Callbacks

    private func registerHostCallbacks() {
        let ud = Unmanaged.passUnretained(self).toOpaque()
        var callbacks = AtermHostCallbacks()
        callbacks.userdata = ud
        callbacks.create_workspace_view = { userdata, idPtr, configPtr in
            guard let userdata, let idPtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let id = String(cString: idPtr)
            let configJson = configPtr.map { String(cString: $0) } ?? "{}"
            NSLog("[aterm-ipc] create_workspace_view: %@ config=%@", id, configJson)
            DispatchQueue.main.async {
                delegate.handleIpcCreateWorkspace(id: id, configJson: configJson)
            }
        }
        callbacks.close_workspace_view = { userdata, idPtr in
            guard let userdata, let idPtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let id = String(cString: idPtr)
            DispatchQueue.main.async {
                if let uuid = delegate.managedWorkspaces.first(where: { $0.value.name == id })?.key {
                    delegate.closeWorkspace(id: uuid)
                }
            }
        }
        callbacks.focus_workspace = { userdata, idPtr in
            guard let userdata, let idPtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let id = String(cString: idPtr)
            DispatchQueue.main.async {
                if let uuid = delegate.managedWorkspaces.first(where: { $0.value.name == id })?.key {
                    delegate.selectWorkspace(uuid)
                }
            }
        }
        callbacks.rename_workspace = { userdata, oldNamePtr, newNamePtr in
            guard let userdata, let oldNamePtr, let newNamePtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let oldName = String(cString: oldNamePtr)
            let newName = String(cString: newNamePtr)
            DispatchQueue.main.async {
                delegate.renameWorkspace(named: oldName, toExact: newName)
            }
        }
        callbacks.send_key = { userdata, workspacePtr, keyPtr in
            guard let userdata, let workspacePtr, let keyPtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let workspace = String(cString: workspacePtr)
            let key = String(cString: keyPtr)
            DispatchQueue.main.async {
                delegate.sendKey(toWorkspaceNamed: workspace, key: key)
            }
        }
        callbacks.attach_external_session = { userdata, sessionIDPtr in
            guard let userdata, let sessionIDPtr else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let sessionID = String(cString: sessionIDPtr)
            DispatchQueue.main.async {
                delegate.attachExternalSession(sessionID)
            }
        }
        callbacks.reload_settings = { userdata in
            guard let userdata else { return }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            DispatchQueue.main.async {
                delegate.reloadSettingsFromIPC()
            }
        }
        callbacks.list_workspaces = { userdata in
            guard let userdata else { return nil }
            let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
            let list = delegate.workspaceOrder.compactMap { id -> [String: String]? in
                guard let ws = delegate.managedWorkspaces[id] else { return nil }
                return [
                    "id": ws.name,
                    "name": ws.name,
                    "cli": ws.launchCommand.rawValue,
                    "cwd": ws.cwd,
                    "status": "running",
                ]
            }
            guard let data = try? JSONSerialization.data(withJSONObject: list),
                  let str = String(data: data, encoding: .utf8) else { return nil }
            return strdup(str)
        }
        callbacks.on_workspace_event = { _, eventPtr in
            guard let eventPtr else { return }
            let event = String(cString: eventPtr)
            NSLog("[aterm-ipc] workspace_event: %@", event)
        }
        callbacks.request_redraw = { _ in
            // No-op for now — individual TerminalViews handle their own redraw
        }
        aterm_set_host(callbacks)
        NSLog("[aterm-ipc] host callbacks registered")
    }

    private func handleIpcCreateWorkspace(id: String, configJson: String) {
        // Parse config and create workspace
        guard let data = configJson.data(using: .utf8),
              let config = try? JSONDecoder().decode(IpcWorkspaceConfig.self, from: data) else {
            NSLog("[aterm-ipc] failed to parse workspace config: %@", configJson)
            return
        }
        let command = WorkspaceLaunchCommand(rawValue: config.cli) ?? .zsh
        let cwd = config.cwd.isEmpty ? NSHomeDirectory() : config.cwd
        createWorkspace(name: config.name, command: command, customCommand: "", cwd: cwd, shouldSelect: true)
    }

    // MARK: - Workspace Persistence

    private func saveWorkspaces() {
        let entries: [[String: Any]] = workspaceOrder.compactMap { id in
            guard let ws = managedWorkspaces[id] else { return nil }
            return [
                "id": ws.name,
                "cwd": ws.cwd,
                "command": ws.launchCommand.rawValue,
                "args": [] as [String],
                "customCommand": ws.customCommand,
                "isSystem": ws.isSystem,
                "resumeCommand": ws.launchCommand.bootstrapCommand(customCommand: ws.customCommand) ?? "",
            ] as [String: Any]
        }
        let wrapper: [String: Any] = ["sessions": entries]
        do {
            let sessionsURL = URL(fileURLWithPath: NSHomeDirectory() + "/.aterm/sessions.json")
            let dir = sessionsURL.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: wrapper, options: [.prettyPrinted, .sortedKeys])
            // Atomic write: write to .tmp then rename/replace
            let tmpURL = sessionsURL.appendingPathExtension("tmp")
            try data.write(to: tmpURL)
            if FileManager.default.fileExists(atPath: sessionsURL.path) {
                _ = try FileManager.default.replaceItemAt(sessionsURL, withItemAt: tmpURL)
            } else {
                try FileManager.default.moveItem(at: tmpURL, to: sessionsURL)
            }
            NSLog("[aterm] saved %d workspaces to %@", entries.count, sessionsURL.path)
        } catch {
            NSLog("[aterm] save workspaces failed: %@", error.localizedDescription)
        }
    }

    private func hasPersistedSessions() -> Bool {
        let fm = FileManager.default
        return fm.fileExists(atPath: NSHomeDirectory() + "/.aterm/sessions.json")
            || fm.fileExists(atPath: NSHomeDirectory() + "/.aigentry/config/sessions.json")
    }

    @discardableResult
    private func restoreWorkspaces() -> Int {
        // Distinguish first install from reinstall/upgrade.
        // NEVER clear sessions just because onboarding is incomplete.
        if !hasPersistedSessions() {
            if needsOnboarding() {
                NSLog("[aterm] first install — no sessions to restore")
            }
            return 0
        }
        if needsOnboarding() {
            NSLog("[aterm] reinstall detected — restoring sessions despite incomplete onboarding")
        }

        let count = Int(aterm_session_count(nil))
        if count == 0 { return 0 }

        var restored = 0
        for i in 0..<count {
            let entry = aterm_session_get(nil, UInt32(i))
            defer { aterm_session_free(entry) }

            guard let idPtr = entry.id else {
                NSLog("[aterm] skipping session entry %d: nil id", i)
                continue
            }
            let name = String(cString: idPtr)
            let cwd = entry.cwd.map { String(cString: $0) } ?? NSHomeDirectory()
            let commandStr = entry.command.map { String(cString: $0) } ?? ""
            let custom = entry.custom_command.map { String(cString: $0) } ?? ""

            let effectiveCwd = FileManager.default.fileExists(atPath: cwd) ? cwd : NSHomeDirectory()
            let requestedCommand = WorkspaceLaunchCommand(rawValue: commandStr) ?? .zsh
            let command = cliAvailable(for: requestedCommand) ? requestedCommand : .zsh

            createWorkspace(
                name: name,
                command: command,
                customCommand: custom,
                cwd: effectiveCwd,
                shouldSelect: i == count - 1,
                isSystem: entry.is_system
            )
            restored += 1
        }
        if restored > 0 {
            NSLog("[aterm] restored %d workspaces", restored)
        }
        return restored
    }

    private func cliAvailable(for command: WorkspaceLaunchCommand) -> Bool {
        switch command {
        case .zsh:
            return true
        case .claude:
            return which("claude")
        case .codex:
            return which("codex")
        case .gemini:
            return which("gemini")
        case .custom:
            return true
        }
    }

    private func createWorkspaceFromOnboarding(_ result: OnboardingResult) {
        // Trust user's selection — don't check cliAvailable.
        // If CLI fails to spawn, PTY termination will trigger zsh fallback.
        let command: WorkspaceLaunchCommand
        switch result.defaultCLI {
        case "claude": command = .claude
        case "codex": command = .codex
        case "gemini": command = .gemini
        default: command = .zsh
        }

        let userSelectedCli = result.defaultCLI != "none"
        let name = userSelectedCli ? "orchestrator" : "main"
        let requestedDirectory = result.initialProjectDirectory?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let cwd: String
        if let requestedDirectory,
           !requestedDirectory.isEmpty,
           FileManager.default.fileExists(atPath: requestedDirectory) {
            cwd = requestedDirectory
        } else {
            cwd = NSHomeDirectory()
        }

        // Pre-create Claude Code trust directory so the trust prompt is skipped
        if command == .claude {
            let sanitized = cwd.replacingOccurrences(of: "/", with: "-")
            let trustDir = NSHomeDirectory() + "/.claude/projects/\(sanitized)"
            try? FileManager.default.createDirectory(atPath: trustDir, withIntermediateDirectories: true)
        }

        NSLog("[aterm] creating workspace from onboarding: name=%@, cli=%@, command=%@", name, result.defaultCLI, command.rawValue)
        createWorkspace(
            name: name,
            command: command,
            customCommand: "",
            cwd: cwd,
            shouldSelect: true,
            isSystem: userSelectedCli
        )
    }

    private func createDefaultWorkspace() {
        // Read defaultCLI from aterm.json (set by TUI wizard)
        let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"
        var defaultCLI = "none"
        var parsedConfig: [String: Any]?
        if let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
           let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            parsedConfig = config
            if let ai = config["ai"] as? [String: Any],
               let cli = ai["defaultCLI"] as? String {
                defaultCLI = cli
            }
        }

        // Map defaultCLI to WorkspaceLaunchCommand
        let requestedCommand: WorkspaceLaunchCommand
        switch defaultCLI {
        case "claude": requestedCommand = .claude
        case "codex": requestedCommand = .codex
        case "gemini": requestedCommand = .gemini
        default: requestedCommand = .zsh
        }

        // Verify CLI is installed, fallback to zsh if not
        let command: WorkspaceLaunchCommand
        if requestedCommand != .zsh && cliAvailable(for: requestedCommand) {
            command = requestedCommand
        } else if requestedCommand != .zsh {
            NSLog("[aterm] %@ not found, starting with zsh", defaultCLI)
            command = .zsh
        } else {
            command = .zsh
        }

        let workspace = parsedConfig?["workspace"] as? [String: Any]
        let configuredCwd = workspace?["defaultCwd"] as? String
        let cwd: String
        if let configuredCwd,
           !configuredCwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
           FileManager.default.fileExists(atPath: configuredCwd) {
            cwd = configuredCwd
        } else {
            cwd = NSHomeDirectory()
        }

        // Name is 'orchestrator' if user selected a CLI (even if binary not found)
        let userSelectedCli = defaultCLI != "none"
        let name = userSelectedCli ? "orchestrator" : "main"

        createWorkspace(
            name: name,
            command: command,
            customCommand: "",
            cwd: cwd,
            shouldSelect: true,
            isSystem: userSelectedCli
        )
    }

    // MARK: - Onboarding & Preferences

    private func detectClis() -> CliStatus {
        guard let jsonPtr = aterm_core_detect_clis() else {
            return CliStatus(claude: false, codex: false, gemini: false)
        }
        let json = String(cString: jsonPtr)
        aterm_core_free_string(jsonPtr)
        guard let data = json.data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Bool] else {
            return CliStatus(claude: false, codex: false, gemini: false)
        }
        return CliStatus(
            claude: dict["claude"] ?? false,
            codex: dict["codex"] ?? false,
            gemini: dict["gemini"] ?? false
        )
    }

    private func needsOnboarding() -> Bool {
        let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return true // No config = needs onboarding
        }
        if config["setupCompleted"] as? Bool != true { return true }
        // Field-by-field check for existing users
        let ai = config["ai"] as? [String: Any]
        if ai?["defaultCLI"] == nil { return true }
        return false
    }

    private func readConfig() -> [String: Any] {
        let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return [:]
        }
        return config
    }

    private func saveOnboardingResult(_ result: OnboardingResult) {
        let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"
        var config = readConfig()
        var ai = config["ai"] as? [String: Any] ?? [:]
        ai["defaultCLI"] = result.defaultCLI
        config["ai"] = ai
        var shell = config["shell"] as? [String: Any] ?? [:]
        shell["default"] = result.defaultShell
        config["shell"] = shell
        var tailscale = config["tailscale"] as? [String: Any] ?? [:]
        tailscale["connect_on_launch"] = result.tailscaleEnabled
        config["tailscale"] = tailscale
        if let initialProjectDirectory = result.initialProjectDirectory,
           !initialProjectDirectory.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            var workspace = config["workspace"] as? [String: Any] ?? [:]
            workspace["defaultCwd"] = initialProjectDirectory
            config["workspace"] = workspace
        }
        config["setupCompleted"] = true

        let dir = (configPath as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        if let data = try? JSONSerialization.data(withJSONObject: config, options: .prettyPrinted) {
            try? data.write(to: URL(fileURLWithPath: configPath))
        }
    }

    private func showOnboarding() {
        let cliStatus = detectClis()
        var showSheet = true

        let onboardingView = OnboardingView(
            isPresented: Binding(
                get: { showSheet },
                set: { [weak self] newValue in
                    showSheet = newValue
                    if !newValue {
                        self?.window.endSheet(self?.window.attachedSheet ?? NSPanel())
                    }
                }
            ),
            cliStatus: cliStatus,
            onComplete: { [weak self] result in
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
                    self?.promptForInitialProjectDirectory(using: result) { finalizedResult in
                        self?.saveOnboardingResult(finalizedResult)
                        if self?.managedWorkspaces.isEmpty ?? true {
                            self?.createWorkspaceFromOnboarding(finalizedResult)
                        }
                    }
                }
            }
        )

        let hostingView = NSHostingView(rootView: onboardingView)
        let sheet = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 440, height: 720),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        sheet.contentView = hostingView
        sheet.title = "Welcome to aterm"
        window.beginSheet(sheet)
    }

    private func promptForInitialProjectDirectory(
        using result: OnboardingResult,
        completion: @escaping (OnboardingResult) -> Void
    ) {
        let panel = NSOpenPanel()
        panel.title = "Choose Project Folder"
        panel.message = "Select a folder for the first workspace. Cancel to use your home directory."
        panel.prompt = AtermLocalization.text(ko: "폴더 사용", en: "Use Folder")
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        panel.resolvesAliases = true

        let projectsDirectory = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("projects", isDirectory: true)
        panel.directoryURL = FileManager.default.fileExists(atPath: projectsDirectory.path)
            ? projectsDirectory
            : URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)

        panel.beginSheetModal(for: window) { response in
            let selectedDirectory = response == .OK
                ? panel.url?.path
                : NSHomeDirectory()
            completion(
                OnboardingResult(
                    defaultCLI: result.defaultCLI,
                    defaultShell: result.defaultShell,
                    tailscaleEnabled: result.tailscaleEnabled,
                    initialProjectDirectory: selectedDirectory
                )
            )
        }
    }

    @objc private func showPreferences() {
        let settings = AtermSettings.shared
        settings.load()

        let settingsView = SettingsView(
            settings: settings,
            onApply: { [weak self] in
                self?.applySettings()
            }
        )

        let hostingView = NSHostingView(rootView: settingsView)
        let prefsWindow = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 520),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        prefsWindow.contentView = hostingView
        prefsWindow.title = AtermLocalization.text(ko: "설정", en: "Settings")
        prefsWindow.center()
        prefsWindow.makeKeyAndOrderFront(nil)
    }

    /// Apply current settings to all terminal views immediately.
    func applySettings() {
        for ws in managedWorkspaces.values {
            applySettingsToView(ws.terminalView)
        }
    }

    /// Apply current settings to a single terminal view.
    private func applySettingsToView(_ view: TerminalView) {
        guard let core = view.corePointer else { return }
        let settings = AtermSettings.shared
        let schemeIndex = AtermSettings.schemeIndex(settings.colorScheme)
        let fontSize = Float(settings.fontSize)
        let lineHeightPx = Float(settings.fontSize * settings.lineHeight)

        aterm_core_set_color_scheme(core, schemeIndex)
        aterm_core_set_font_size(core, fontSize)
        aterm_core_set_line_height(core, lineHeightPx)

        // Font size / line height changes affect grid dimensions — trigger resize
        let backingSize = view.convertToBacking(view.bounds).size
        if backingSize.width > 0 && backingSize.height > 0 {
            aterm_core_resize(core, UInt32(backingSize.width), UInt32(backingSize.height))
        }
        aterm_core_render(core)
    }

    private func setupPreferencesMenu() {
        if let appMenu = NSApp.mainMenu?.item(at: 0)?.submenu {
            let prefsItem = NSMenuItem(
                title: "Preferences...",
                action: #selector(showPreferences),
                keyEquivalent: ","
            )
            prefsItem.target = self
            // Insert after "About" (index 0) and separator (index 1)
            let insertIndex = min(2, appMenu.items.count)
            appMenu.insertItem(prefsItem, at: insertIndex)
            appMenu.insertItem(NSMenuItem.separator(), at: insertIndex)
        }
    }

    private func which(_ command: String) -> Bool {
        // Use login shell to find binaries — macOS app environment has limited PATH
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: shell)
        process.arguments = ["-l", "-c", "command -v \(command) >/dev/null 2>&1"]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }

    private func whichPath(_ command: String) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        process.arguments = [command]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else { return nil }
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let value = String(data: data, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard let value, !value.isEmpty else { return nil }
            return value
        } catch {
            return nil
        }
    }

    private func resolveTeleptyFromShell(_ shellPath: String) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: shellPath)
        process.arguments = ["-lc", "command -v telepty 2>/dev/null || which telepty 2>/dev/null"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            guard process.terminationStatus == 0 else { return nil }
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let value = String(data: data, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard let value,
                  !value.isEmpty,
                  FileManager.default.isExecutableFile(atPath: value) else { return nil }
            return value
        } catch {
            return nil
        }
    }

    private func collectNodeManagerTeleptyPaths(homeDirectory: String) -> [String] {
        var paths: [String] = []
        let fileManager = FileManager.default

        let nvmRoot = "\(homeDirectory)/.nvm/versions/node"
        if let entries = try? fileManager.contentsOfDirectory(atPath: nvmRoot) {
            for entry in entries.sorted(by: >) {
                paths.append("\(nvmRoot)/\(entry)/bin/telepty")
            }
        }

        let fnmRoot = "\(homeDirectory)/.fnm/node-versions"
        if let entries = try? fileManager.contentsOfDirectory(atPath: fnmRoot) {
            for entry in entries.sorted(by: >) {
                paths.append("\(fnmRoot)/\(entry)/installation/bin/telepty")
            }
        }

        return paths
    }

    /// Returns (path, isJS) — if isJS, run via "node <path> daemon" instead of "<path> daemon"
    private func findTeleptyBinary() -> (path: String, isJS: Bool)? {
        let environment = ProcessInfo.processInfo.environment
        let homeDirectory = environment["HOME"] ?? NSHomeDirectory()
        let fm = FileManager.default

        // 1. Shell-based resolution (picks up PATH from login shell)
        let shellCandidates = [
            environment["SHELL"],
            "/bin/zsh",
            "/bin/bash",
        ].compactMap { $0 }

        for shellPath in shellCandidates {
            if let resolved = resolveTeleptyFromShell(shellPath) {
                return (resolved, false)
            }
        }

        // 2. which
        if let resolved = whichPath("telepty"),
           fm.isExecutableFile(atPath: resolved) {
            return (resolved, false)
        }

        // 3. Direct binary paths
        let directPaths = [
            "\(homeDirectory)/.volta/bin/telepty",
            "\(homeDirectory)/.local/bin/telepty",
            "/opt/homebrew/bin/telepty",
            "/usr/local/bin/telepty",
        ] + collectNodeManagerTeleptyPaths(homeDirectory: homeDirectory)

        if let found = directPaths.first(where: { fm.isExecutableFile(atPath: $0) }) {
            return (found, false)
        }

        // 4. npm global node_modules cli.js paths (telepty is a dep of aterm, not top-level)
        let npmCliPaths = collectNpmTeleptyCliPaths(homeDirectory: homeDirectory)
        if let found = npmCliPaths.first(where: { fm.fileExists(atPath: $0) }) {
            return (found, true)
        }

        return nil
    }

    private func collectNpmTeleptyCliPaths(homeDirectory: String) -> [String] {
        let fm = FileManager.default
        let teleptyPkg = "@dmsdc-ai/aigentry-telepty/cli.js"
        let atermPkg = "@dmsdc-ai/aterm/node_modules/\(teleptyPkg)"
        var paths: [String] = []

        // Common npm global prefixes
        let globalPrefixes = [
            "/usr/local/lib/node_modules",
            "/opt/homebrew/lib/node_modules",
            "\(homeDirectory)/.npm-global/lib/node_modules",
        ]
        for prefix in globalPrefixes {
            // Hoisted (top-level dep)
            paths.append("\(prefix)/\(teleptyPkg)")
            // Nested inside aterm
            paths.append("\(prefix)/\(atermPkg)")
        }

        // nvm paths
        let nvmRoot = "\(homeDirectory)/.nvm/versions/node"
        if let entries = try? fm.contentsOfDirectory(atPath: nvmRoot) {
            for entry in entries.sorted(by: >) {
                let lib = "\(nvmRoot)/\(entry)/lib/node_modules"
                paths.append("\(lib)/\(teleptyPkg)")
                paths.append("\(lib)/\(atermPkg)")
            }
        }

        // fnm paths
        let fnmRoot = "\(homeDirectory)/.fnm/node-versions"
        if let entries = try? fm.contentsOfDirectory(atPath: fnmRoot) {
            for entry in entries.sorted(by: >) {
                let lib = "\(fnmRoot)/\(entry)/installation/lib/node_modules"
                paths.append("\(lib)/\(teleptyPkg)")
                paths.append("\(lib)/\(atermPkg)")
            }
        }

        // volta
        paths.append("\(homeDirectory)/.volta/tools/image/packages/@dmsdc-ai/aigentry-telepty/lib/node_modules/\(teleptyPkg)")

        return paths
    }

    private func ensureTeleptyDaemon() {
        DispatchQueue.global(qos: .utility).async {
            // Check if telepty daemon is already running
            guard let url = URL(string: "http://127.0.0.1:3848/api/sessions") else { return }
            var request = URLRequest(url: url)
            request.timeoutInterval = 2.0

            let semaphore = DispatchSemaphore(value: 0)
            var isRunning = false

            URLSession.shared.dataTask(with: request) { _, response, _ in
                if let http = response as? HTTPURLResponse, http.statusCode == 200 {
                    isRunning = true
                }
                semaphore.signal()
            }.resume()

            semaphore.wait()

            if isRunning {
                NSLog("[aterm] telepty daemon already running")
                return
            }

            guard let telepty = self.findTeleptyBinary() else {
                NSLog("[aterm] telepty binary not found")
                return
            }

            NSLog("[aterm] starting telepty daemon from %@ (js=%d)", telepty.path, telepty.isJS ? 1 : 0)
            let process = Process()
            if telepty.isJS {
                process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
                process.arguments = ["node", telepty.path, "daemon"]
            } else {
                process.executableURL = URL(fileURLWithPath: telepty.path)
                process.arguments = ["daemon"]
            }
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            process.environment = ProcessInfo.processInfo.environment

            do {
                try process.run()
                let daemonPID = process.processIdentifier
                NSLog("[aterm] telepty daemon started (pid %d)", daemonPID)
                DispatchQueue.main.async { [weak self] in
                    self?.excludedChildPIDs.insert(daemonPID)
                }
            } catch {
                NSLog("[aterm] failed to start telepty daemon: %@", error.localizedDescription)
            }
        }
    }

    private func startTailscale() {
        let env = ProcessInfo.processInfo.environment

        // Respect disabled setting: env var, UserDefaults, or aterm.json
        if let envFlag = env["ATERM_TAILSCALE_ENABLED"], envFlag == "0" || envFlag.lowercased() == "false" || envFlag.lowercased() == "no" {
            NSLog("[aterm] tailscale disabled via ATERM_TAILSCALE_ENABLED")
            return
        }
        if !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled") && UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil {
            NSLog("[aterm] tailscale disabled in settings")
            return
        }
        // Check aterm.json config (set by onboarding)
        let config = readConfig()
        let tailscale = config["tailscale"] as? [String: Any]
        let connectOnLaunch = tailscale?["connect_on_launch"] as? Bool ?? false
        if !connectOnLaunch {
            NSLog("[aterm] tailscale disabled in aterm.json (connect_on_launch=false)")
            return
        }

        let result = withOptionalCString(env["ATERM_TAILSCALE_HOSTNAME"]) { hostnamePtr in
            withOptionalCString(env["ATERM_TAILSCALE_CONTROL_URL"]) { controlURLPtr in
                withOptionalCString(env["ATERM_TAILSCALE_AUTHKEY"]) { authKeyPtr in
                    aterm_tailscale_connect(hostnamePtr, controlURLPtr, authKeyPtr)
                }
            }
        }

        if result != 0 {
            NSLog("[aterm] tailscale startup failed (no auth key or network issue) — skipping")
            return
        }

        logTailscaleStatus(prefix: "startup")
    }

    private func logTailscaleStatus(prefix: String) {
        guard let jsonPtr = aterm_tailscale_status_json() else { return }
        let json = String(cString: jsonPtr)
        aterm_core_free_string(jsonPtr)
        NSLog("[aterm] tailscale %@: %@", prefix, json)
    }

    private func beginWorkspaceCreation(_ request: WorkspaceCreationRequest) {
        let panel = NSOpenPanel()
        panel.title = "Choose Workspace Folders"
        panel.message = "Select one or more folders to open as workspaces."
        panel.prompt = "Choose"
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = false
        panel.resolvesAliases = true

        if let initialDirectory = request.initialDirectory,
           !initialDirectory.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            panel.directoryURL = URL(fileURLWithPath: initialDirectory, isDirectory: true)
        }

        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK else { return }
            self?.workspaceSidebarModel.presentCreationDrafts(
                for: panel.urls,
                preferredCommand: request.preferredCommand,
                preferredCustomCommand: request.preferredCustomCommand
            )
        }
    }

    private func createWorkspaces(from drafts: [WorkspaceDraft]) {
        let normalizedDrafts = drafts.filter { !$0.cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        guard !normalizedDrafts.isEmpty else { return }

        for (index, draft) in normalizedDrafts.enumerated() {
            createWorkspace(
                name: draft.name.trimmingCharacters(in: .whitespacesAndNewlines),
                command: draft.command,
                customCommand: draft.customCommand,
                cwd: draft.cwd,
                shouldSelect: index == normalizedDrafts.count - 1
            )
        }
    }

    private func createWorkspace(
        name: String,
        command: WorkspaceLaunchCommand,
        customCommand: String,
        cwd: String,
        shouldSelect: Bool,
        isSystem: Bool = false
    ) {
        let workspaceID = UUID()
        let resolvedName = uniqueWorkspaceName(for: name, cwd: cwd, excluding: nil)
        let bootstrapCommand = command.bootstrapCommand(customCommand: customCommand)
        let baselineChildPIDs: Set<Int32> = []
        let terminalView = TerminalView(frame: terminalContainerView.bounds)
        terminalView.workspaceName = resolvedName
        terminalView.spawnCommand = bootstrapCommand
        terminalView.initialWorkingDirectory = cwd
        terminalView.autoresizingMask = [.width, .height]
        terminalView.isHidden = true
        terminalView.onShellSpawned = { [weak self] result in
            guard let self else { return }
            DispatchQueue.main.async {
                self.refreshWorkspaceProcesses()
            }
        }

        let workspace = ManagedWorkspace(
            id: workspaceID,
            name: resolvedName,
            cwd: cwd,
            launchCommand: command,
            customCommand: customCommand,
            terminalView: terminalView,
            createdAt: Date(),
            baselineChildPIDs: baselineChildPIDs,
            isSystem: isSystem
        )
        terminalView.onActivity = { [weak workspace] in
            workspace?.lastActivityAt = Date()
        }
        workspace.lastLaunchTime = Date()
        managedWorkspaces[workspaceID] = workspace
        // System workspaces always first
        if isSystem {
            workspaceOrder.insert(workspaceID, at: 0)
        } else {
            workspaceOrder.append(workspaceID)
        }
        terminalContainerView.addSubview(terminalView)

        // Apply user settings (color scheme, font size, line height)
        applySettingsToView(terminalView)

        if shouldSelect {
            selectWorkspace(workspaceID)
        } else {
            rebuildSidebarState()
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            self?.refreshWorkspaceProcesses()
        }

        saveWorkspaces()

        // Delegate MD generation to aigentry-devkit (skip silently if not installed)
        devkitWorkspaceInit(cli: command.rawValue, cwd: cwd, workspaceID: workspaceID)
    }

    private func devkitWorkspaceInit(cli: String, cwd: String, workspaceID: UUID) {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        task.arguments = ["aigentry-devkit", "workspace-init", "--cli", cli, "--cwd", cwd]
        let pipe = Pipe()
        task.standardOutput = pipe
        task.standardError = FileHandle.nullDevice
        DispatchQueue.global(qos: .utility).async { [weak self] in
            do {
                try task.run()
                task.waitUntilExit()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: data, encoding: .utf8) ?? ""
                if output.contains("INJECT:/init") {
                    NSLog("[aterm] devkit signaled INJECT:/init for workspace %@", workspaceID.uuidString)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 3.0) {
                        guard let self,
                              let workspace = self.managedWorkspaces[workspaceID],
                              let core = workspace.terminalView.corePointer else { return }
                        let text = "/init\n"
                        text.withCString { ptr in
                            aterm_core_write_pty(core, ptr, text.utf8.count)
                        }
                        NSLog("[aterm] auto-injected /init into workspace '%@'", workspace.name)
                    }
                }
            } catch {
                // aigentry-devkit not installed — standalone mode, skip silently
            }
        }
    }

    private func bootstrapWorkspace(id: UUID, command: String) {
        guard let workspace = managedWorkspaces[id] else { return }
        workspace.lastLaunchTime = Date()
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { [weak self] in
            self?.sendBootstrapCommand(workspaceID: id, command: command)
        }
    }

    /// Pre-trust workspace for Claude Code by creating the project directory.
    private func ensureClaudeProjectTrust(cwd: String) {
        let encoded = cwd.replacingOccurrences(of: "/", with: "-")
        let claudeProjectDir = NSHomeDirectory() + "/.claude/projects/" + encoded
        try? FileManager.default.createDirectory(atPath: claudeProjectDir, withIntermediateDirectories: true)
    }

    private func sendBootstrapCommand(workspaceID: UUID, command: String) {
        guard let workspace = managedWorkspaces[workspaceID],
              let core = workspace.terminalView.corePointer else { return }

        // Pre-trust workspace for Claude Code
        if command.contains("claude") {
            ensureClaudeProjectTrust(cwd: workspace.cwd)
        }

        NSLog("[aterm] sending bootstrap to '%@' (len=%d, cliGaveUp=%d): %@",
              workspace.name, command.count, workspace.cliGaveUp ? 1 : 0, command)
        let text = command + "\n"
        text.withCString { ptr in
            aterm_core_write_pty(core, ptr, text.utf8.count)
        }
        workspace.lastLaunchTime = Date()

        // Watch for trust prompt after CLI starts
        let isCli = command.contains("claude") || command.contains("codex") || command.contains("gemini")
        if isCli {
            watchForTrustPrompt(workspaceID: workspaceID)
        }
    }

    /// Poll terminal screen for trust prompt; auto-accept with Enter when detected.
    private static let trustPatterns = ["trust", "Trust", "Do you trust"]

    private func watchForTrustPrompt(workspaceID: UUID, attempts: Int = 0) {
        guard attempts < 15 else {
            NSLog("[aterm] trust prompt watch timed out for workspace (15 attempts)")
            return
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            guard let self,
                  let workspace = self.managedWorkspaces[workspaceID],
                  let core = workspace.terminalView.corePointer else { return }

            // Sync PTY so screen is up to date
            aterm_core_sync_pty(core)

            var detected = false
            for pattern in Self.trustPatterns {
                let found = pattern.withCString { ptr in
                    aterm_core_screen_contains(core, ptr) != 0
                }
                if found {
                    detected = true
                    NSLog("[aterm] trust prompt detected (pattern: '%@', attempt: %d)", pattern, attempts)
                    break
                }
            }

            if detected {
                // Send Enter to accept the default (Yes) trust option
                "\r".withCString { ptr in
                    aterm_core_write_pty(core, ptr, 1)
                }
                NSLog("[aterm] auto-accepted workspace trust prompt for %@", workspace.name)
            } else {
                self.watchForTrustPrompt(workspaceID: workspaceID, attempts: attempts + 1)
            }
        }
    }

    private func restartSystemWorkspace(id: UUID) {
        guard let workspace = managedWorkspaces[id],
              workspace.isSystem,
              workspace.status == "restarting",
              !workspace.cliGaveUp else { return }

        NSLog("[aterm] re-sending CLI bootstrap to '%@' (attempt %d)", workspace.name, workspace.restartCount)

        // Shell (zsh) is already running — just send the CLI command to it
        if let bootstrapCmd = workspace.launchCommand.bootstrapCommand(customCommand: workspace.customCommand) {
            bootstrapWorkspace(id: id, command: bootstrapCmd)
        }
        workspace.status = "starting"
    }

    private func workspaceID(named name: String) -> UUID? {
        managedWorkspaces.first(where: { $0.value.name == name })?.key
    }

    private func selectWorkspace(_ id: UUID) {
        guard let workspace = managedWorkspaces[id] else { return }

        for candidateID in workspaceOrder {
            managedWorkspaces[candidateID]?.terminalView.isHidden = candidateID != id
        }

        terminalView = workspace.terminalView
        workspaceSidebarModel.selectedWorkspaceID = id
        busClient.setCore(workspace.terminalView.corePointer)
        window.makeFirstResponder(workspace.terminalView)
        rebuildSidebarState()

        // Retry shell spawn for views that failed when hidden during initial setup.
        // This ensures every workspace registers with telepty, not just the first one.
        if !workspace.terminalView.didSpawnShell {
            DispatchQueue.main.async {
                workspace.terminalView.retrySpawnIfNeeded()
            }
        }
    }

    private func renameWorkspace(id: UUID, to nextName: String) {
        guard let workspace = managedWorkspaces[id] else { return }
        let trimmed = nextName.trimmingCharacters(in: .whitespacesAndNewlines)
        workspace.name = uniqueWorkspaceName(
            for: trimmed,
            cwd: workspace.cwd,
            excluding: id
        )
        workspace.terminalView.workspaceName = workspace.name
        rebuildSidebarState()
        saveWorkspaces()
    }

    private func renameWorkspace(named oldName: String, toExact nextName: String) {
        guard let id = workspaceID(named: oldName),
              let workspace = managedWorkspaces[id] else { return }

        let trimmed = nextName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        if let existing = workspaceID(named: trimmed), existing != id {
            return
        }

        workspace.name = trimmed
        workspace.terminalView.workspaceName = trimmed
        rebuildSidebarState()
        saveWorkspaces()
    }

    private func closeWorkspace(id: UUID) {
        guard let workspace = managedWorkspaces[id] else { return }
        if workspace.isSystem { return } // System workspaces cannot be closed
        managedWorkspaces.removeValue(forKey: id)

        workspace.terminalView.removeFromSuperview()
        workspaceOrder.removeAll { $0 == id }

        if workspaceSidebarModel.selectedWorkspaceID == id {
            let nextSelection = workspaceOrder.first
            workspaceSidebarModel.selectedWorkspaceID = nextSelection
            if let nextSelection {
                selectWorkspace(nextSelection)
            } else {
                terminalView = nil
                rebuildSidebarState()
            }
        } else {
            rebuildSidebarState()
        }

        saveWorkspaces()
    }

    private func attachExternalSession(_ sessionID: String) {
        // Check if already attached — find existing workspace running telepty attach for this session
        for id in workspaceOrder {
            if let ws = managedWorkspaces[id],
               ws.launchCommand == .custom,
               ws.customCommand == "telepty attach \(sessionID)" {
                selectWorkspace(id)
                return
            }
        }

        // Find the session info from busClient for cwd
        let session = busClient.sessions.first { $0.id == sessionID }
        let cwd = session?.cwd ?? NSHomeDirectory()

        createWorkspace(
            name: sessionID,
            command: .custom,
            customCommand: "telepty attach \(sessionID)",
            cwd: cwd,
            shouldSelect: true
        )
    }

    private func sendKey(toWorkspaceNamed workspaceName: String, key: String) {
        guard let id = workspaceID(named: workspaceName),
              let workspace = managedWorkspaces[id],
              let core = workspace.terminalView.corePointer,
              let payload = keyPayload(for: key) else { return }

        payload.withCString { ptr in
            aterm_core_write_pty(core, ptr, payload.utf8.count)
        }
        workspace.lastActivityAt = Date()
    }

    private func keyPayload(for key: String) -> String? {
        switch key.lowercased() {
        case "enter", "return":
            return "\r"
        case "ctrl+c", "ctrl-c":
            return "\u{03}"
        case "ctrl+d", "ctrl-d":
            return "\u{04}"
        case "ctrl+l", "ctrl-l":
            return "\u{0c}"
        case "ctrl+z", "ctrl-z":
            return "\u{1a}"
        case "tab":
            return "\t"
        case "esc", "escape":
            return "\u{1b}"
        default:
            return nil
        }
    }

    private func reloadSettingsFromIPC() {
        AtermSettings.shared.load()
        applySettings()
        rebuildSidebarState()
    }

    private func rebuildSidebarState() {
        let settings = AtermSettings.shared
        workspaceSidebarModel.workspaces = workspaceOrder.compactMap { id in
            guard let workspace = managedWorkspaces[id] else { return nil }
            return SidebarWorkspace(
                id: id,
                name: workspace.name,
                cwd: workspace.cwd,
                launchCommand: workspace.launchCommand,
                customCommand: workspace.customCommand,
                foregroundProcessName: workspace.foregroundProcessName,
                status: workspace.status,
                cliIcon: cliIcon(for: workspace.launchCommand, useAscii: settings.useAsciiIcons),
                statusEmoji: statusIcon(for: workspace.status, useAscii: settings.useAsciiIcons),
                createdAt: workspace.createdAt,
                lastActivityAt: workspace.lastActivityAt,
                isSystem: workspace.isSystem
            )
        }
    }

    private func cliIcon(for command: WorkspaceLaunchCommand, useAscii: Bool) -> String {
        if useAscii {
            switch command {
            case .claude: return "[C]"
            case .codex: return "[X]"
            case .gemini: return "[G]"
            case .zsh, .custom: return "[S]"
            }
        } else {
            return command.cliIcon
        }
    }

    private func statusIcon(for status: String, useAscii: Bool) -> String {
        if useAscii {
            switch status {
            case "working": return "[*]"
            case "idle": return "[-]"
            case "dead": return "[!]"
            case "starting", "restarting": return "[>]"
            default: return "[?]"
            }
        } else {
            switch status {
            case "working": return "🔨"
            case "idle": return "💤"
            case "dead": return "🔴"
            case "starting", "restarting": return "🔄"
            default: return ""
            }
        }
    }

    private func startProcessPolling() {
        processPollTimer?.invalidate()
        processPollTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.refreshWorkspaceProcesses()
        }
        processPollTimer?.tolerance = 0.2
        refreshWorkspaceProcesses()
    }

    private func refreshWorkspaceProcesses() {
        let now = Date()
        for id in workspaceOrder {
            guard let workspace = managedWorkspaces[id] else { continue }
            let defaultProcessName = workspace.cliGaveUp
                ? "shell (CLI unavailable)"
                : workspace.launchCommand.displayTitle(customCommand: workspace.customCommand)

            guard workspace.terminalView.didSpawnShell else {
                workspace.status = "starting"
                workspace.foregroundProcessName = defaultProcessName
                continue
            }

            guard workspace.terminalView.isPtyAlive else {
                workspace.status = "dead"
                workspace.foregroundProcessName = defaultProcessName
                continue
            }

            // Determine if working or idle based on last activity (30s threshold)
            let idleTime = now.timeIntervalSince(workspace.lastActivityAt)
            if idleTime < 30 {
                workspace.status = "working"
            } else {
                workspace.status = "idle"
            }
            
            workspace.foregroundProcessName = defaultProcessName
        }

        rebuildSidebarState()
    }

    private func uniqueWorkspaceName(
        for preferredName: String,
        cwd: String,
        excluding excludedID: UUID? = nil
    ) -> String {
        let trimmed = preferredName.trimmingCharacters(in: .whitespacesAndNewlines)
        let base = trimmed.isEmpty ? defaultWorkspaceName(for: cwd) : trimmed
        let existingNames = Set(
            managedWorkspaces
                .filter { $0.key != excludedID }
                .map { $0.value.name.lowercased() }
        )

        if !existingNames.contains(base.lowercased()) {
            return base
        }

        var index = 2
        while existingNames.contains("\(base) \(index)".lowercased()) {
            index += 1
        }
        return "\(base) \(index)"
    }

    private func defaultWorkspaceName(for cwd: String) -> String {
        let path = cwd.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        if path.isEmpty {
            return "workspace"
        }

        let folderName = URL(fileURLWithPath: cwd).lastPathComponent
        return folderName.isEmpty ? "workspace" : folderName
    }

    private func directChildProcessIDs(of parentPID: Int32) -> Set<Int32> {
        let output = runPS(arguments: ["-axo", "pid=,ppid="])
        let pairs = output
            .split(separator: "\n")
            .compactMap { line -> (Int32, Int32)? in
                let parts = line
                    .split(whereSeparator: \.isWhitespace)
                    .map(String.init)
                guard parts.count >= 2,
                      let pid = Int32(parts[0]),
                      let ppid = Int32(parts[1]) else {
                    return nil
                }
                return (pid, ppid)
            }

        return Set(pairs.filter { $0.1 == parentPID }.map(\.0))
    }

    private func processExists(_ pid: Int32) -> Bool {
        if kill(pid, 0) == 0 {
            return true
        }
        return errno != ESRCH
    }

    private func processName(for pid: Int32) -> String? {
        let output = runPS(arguments: ["-o", "comm=", "-p", String(pid)])
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !output.isEmpty else { return nil }
        return displayProcessName(output)
    }

    private func foregroundProcessName(forRootPID rootPID: Int32) -> String? {
        let tty = runPS(arguments: ["-o", "tty=", "-p", String(rootPID)])
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !tty.isEmpty, tty != "??" else {
            return processName(for: rootPID)
        }

        let processes = ttyProcesses(for: tty)
        guard !processes.isEmpty else {
            return processName(for: rootPID)
        }

        let parentByPID = Dictionary(uniqueKeysWithValues: processes.map { ($0.pid, $0.ppid) })
        let foregroundCandidates = processes.filter { $0.stat.contains("+") }
        let relevantCandidates = foregroundCandidates.filter {
            $0.pid == rootPID || isDescendant($0.pid, of: rootPID, parentByPID: parentByPID)
        }

        let chosen = (relevantCandidates.isEmpty ? processes.filter { $0.pid == rootPID } : relevantCandidates)
            .sorted(by: { left, right in
                processPriority(for: left, rootPID: rootPID) > processPriority(for: right, rootPID: rootPID)
            })
            .first

        return chosen.map { displayProcessName($0.command) }
    }

    private func ttyProcesses(for tty: String) -> [TTYProcessSnapshot] {
        let output = runPS(arguments: ["-t", tty, "-o", "pid=,ppid=,stat=,comm="])
        return output
            .split(separator: "\n")
            .compactMap { line in
                let parts = line
                    .split(maxSplits: 3, omittingEmptySubsequences: true, whereSeparator: \.isWhitespace)
                    .map(String.init)
                guard parts.count == 4,
                      let pid = Int32(parts[0]),
                      let ppid = Int32(parts[1]) else {
                    return nil
                }
                return TTYProcessSnapshot(
                    pid: pid,
                    ppid: ppid,
                    stat: parts[2],
                    command: parts[3]
                )
            }
    }

    private func isDescendant(_ pid: Int32, of ancestorPID: Int32, parentByPID: [Int32: Int32]) -> Bool {
        var current = pid
        while let parent = parentByPID[current] {
            if parent == ancestorPID {
                return true
            }
            if parent == current {
                break
            }
            current = parent
        }
        return false
    }

    private func processPriority(for process: TTYProcessSnapshot, rootPID: Int32) -> (Int, Int, Int32) {
        let name = displayProcessName(process.command)
        return (
            knownShells.contains(name) ? 0 : 1,
            process.pid == rootPID ? 0 : 1,
            process.pid
        )
    }

    private func displayProcessName(_ command: String) -> String {
        URL(fileURLWithPath: command).lastPathComponent
    }

    private func runPS(arguments: [String]) -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/ps")
        process.arguments = arguments

        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = FileHandle.nullDevice

        do {
            try process.run()
            process.waitUntilExit()
            let data = stdout.fileHandleForReading.readDataToEndOfFile()
            return String(decoding: data, as: UTF8.self)
        } catch {
            return ""
        }
    }
}

private func withOptionalCString<T>(_ value: String?, _ body: (UnsafePointer<CChar>?) -> T) -> T {
    guard let value,
          !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return body(nil)
    }

    return value.withCString { ptr in
        body(ptr)
    }
}

private let knownShells: Set<String> = ["bash", "fish", "sh", "zsh"]

private final class ManagedWorkspace {
    let id: UUID
    let cwd: String
    let launchCommand: WorkspaceLaunchCommand
    let customCommand: String
    let terminalView: TerminalView
    let createdAt: Date
    let baselineChildPIDs: Set<Int32>

    var name: String
    var rootProcessID: Int32?
    var foregroundProcessName: String
    var status: String
    var isSystem: Bool
    var restartCount: Int = 0
    var lastLaunchTime: Date?
    var lastActivityAt: Date
    var cliGaveUp: Bool = false

    init(
        id: UUID,
        name: String,
        cwd: String,
        launchCommand: WorkspaceLaunchCommand,
        customCommand: String,
        terminalView: TerminalView,
        createdAt: Date,
        baselineChildPIDs: Set<Int32>,
        isSystem: Bool = false
    ) {
        self.id = id
        self.name = name
        self.cwd = cwd
        self.launchCommand = launchCommand
        self.customCommand = customCommand
        self.terminalView = terminalView
        self.createdAt = createdAt
        self.baselineChildPIDs = baselineChildPIDs
        self.foregroundProcessName = launchCommand.displayTitle(customCommand: customCommand)
        self.status = "starting"
        self.lastActivityAt = createdAt
        self.isSystem = isSystem
    }
}

private struct TTYProcessSnapshot {
    let pid: Int32
    let ppid: Int32
    let stat: String
    let command: String
}
