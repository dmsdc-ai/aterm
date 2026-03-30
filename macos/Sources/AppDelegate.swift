import AppKit
import Darwin
import Foundation
import SwiftUI

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
            }
        )
        let sidebarHost = NSHostingView(rootView: sidebarView)
        sidebarHost.frame = NSRect(x: 0, y: 0, width: 240, height: rect.height)

        // Create terminal container
        let terminalRect = NSRect(x: 0, y: 0, width: rect.width - 240, height: rect.height)
        terminalContainerView = NSView(frame: terminalRect)
        terminalContainerView.autoresizingMask = [.width, .height]

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
        let restoredCount = restoreWorkspaces()
        if restoredCount == 0 {
            createDefaultWorkspace()
        }
        startProcessPolling()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        saveWorkspaces()
        processPollTimer?.invalidate()
        processPollTimer = nil
        let env = ProcessInfo.processInfo.environment
        let tailscaleDisabledEnv = env["ATERM_TAILSCALE_ENABLED"].map { $0 == "0" || $0.lowercased() == "false" || $0.lowercased() == "no" } ?? false
        let tailscaleDisabledDefaults = UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil && !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled")
        if !tailscaleDisabledEnv && !tailscaleDisabledDefaults {
            aterm_tailscale_shutdown()
        }
    }

    // MARK: - Workspace Persistence

    private static let workspacesFileURL: URL = {
        let home = NSHomeDirectory()
        return URL(fileURLWithPath: "\(home)/.aigentry/config/sessions.json")
    }()

    private func saveWorkspaces() {
        let entries: [[String: Any]] = workspaceOrder.compactMap { id in
            guard let ws = managedWorkspaces[id] else { return nil }
            return [
                "name": ws.name,
                "command": ws.launchCommand.rawValue,
                "customCommand": ws.customCommand,
                "cwd": ws.cwd,
                "resumeCommand": ws.launchCommand.bootstrapCommand(customCommand: ws.customCommand) ?? "",
                "isActive": ws.status != "dead",
            ] as [String: Any]
        }
        do {
            let dir = Self.workspacesFileURL.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let data = try JSONSerialization.data(withJSONObject: entries, options: .prettyPrinted)
            try data.write(to: Self.workspacesFileURL)
        } catch {
            NSLog("[aterm] save workspaces failed: %@", error.localizedDescription)
        }
    }

    private func isSetupCompleted() -> Bool {
        let configPath = NSHomeDirectory() + "/.aigentry/config/aterm.json"
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
              let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return false
        }
        return config["setupCompleted"] as? Bool ?? false
    }

    @discardableResult
    private func restoreWorkspaces() -> Int {
        guard isSetupCompleted() else {
            NSLog("[aterm] first-run wizard not completed — skipping restore")
            return 0
        }

        guard let data = try? Data(contentsOf: Self.workspacesFileURL),
              let entries = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return 0
        }

        // Also migrate from old location if needed
        let oldURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("aterm/sessions.json")
        if entries.isEmpty, let oldData = try? Data(contentsOf: oldURL),
           let oldEntries = try? JSONSerialization.jsonObject(with: oldData) as? [[String: Any]], !oldEntries.isEmpty {
            return restoreEntries(oldEntries)
        }

        return restoreEntries(entries)
    }

    private func restoreEntries(_ entries: [[String: Any]]) -> Int {
        var count = 0
        let activeEntries = entries.filter { ($0["isActive"] as? Bool) != false }
        for (index, entry) in activeEntries.enumerated() {
            guard let name = entry["name"] as? String,
                  let commandRaw = entry["command"] as? String,
                  let cwd = entry["cwd"] as? String else { continue }

            let custom = entry["customCommand"] as? String ?? ""
            let effectiveCwd = FileManager.default.fileExists(atPath: cwd) ? cwd : NSHomeDirectory()

            // CLI binary fallback: if CLI not installed, fall back to zsh
            let requestedCommand = WorkspaceLaunchCommand(rawValue: commandRaw) ?? .zsh
            let command = cliAvailable(for: requestedCommand) ? requestedCommand : .zsh

            createWorkspace(
                name: name,
                command: command,
                customCommand: custom,
                cwd: effectiveCwd,
                shouldSelect: index == activeEntries.count - 1
            )
            count += 1
        }
        if count > 0 {
            NSLog("[aterm] restored %d workspaces", count)
        }
        return count
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

    private func createDefaultWorkspace() {
        let orchestratorDir = NSHomeDirectory() + "/projects/aigentry-orchestrator"
        let hasClaude = FileManager.default.isExecutableFile(atPath: "/usr/local/bin/claude")
            || FileManager.default.isExecutableFile(
                atPath: (ProcessInfo.processInfo.environment["HOME"] ?? "") + "/.nvm/versions/node/v20.20.0/bin/claude"
            )
            || which("claude")

        let orchestratorExists = FileManager.default.fileExists(atPath: orchestratorDir)

        if hasClaude && orchestratorExists {
            createWorkspace(
                name: "orchestrator",
                command: .claude,
                customCommand: "",
                cwd: orchestratorDir,
                shouldSelect: true
            )
        } else {
            createWorkspace(
                name: hasClaude ? "orchestrator" : "main",
                command: hasClaude ? .claude : .zsh,
                customCommand: "",
                cwd: orchestratorExists ? orchestratorDir : NSHomeDirectory(),
                shouldSelect: true
            )
        }
    }

    private func which(_ command: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        process.arguments = [command]
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

    private func findTeleptyBinary() -> String? {
        let environment = ProcessInfo.processInfo.environment
        let homeDirectory = environment["HOME"] ?? NSHomeDirectory()

        let shellCandidates = [
            environment["SHELL"],
            "/bin/zsh",
            "/bin/bash",
        ].compactMap { $0 }

        for shellPath in shellCandidates {
            if let resolved = resolveTeleptyFromShell(shellPath) {
                return resolved
            }
        }

        if let resolved = whichPath("telepty"),
           FileManager.default.isExecutableFile(atPath: resolved) {
            return resolved
        }

        let searchPaths = [
            "\(homeDirectory)/.volta/bin/telepty",
            "\(homeDirectory)/.local/bin/telepty",
            "/opt/homebrew/bin/telepty",
            "/usr/local/bin/telepty",
        ] + collectNodeManagerTeleptyPaths(homeDirectory: homeDirectory)

        return searchPaths.first(where: { FileManager.default.isExecutableFile(atPath: $0) })
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

            guard let teleptyPath = self.findTeleptyBinary() else {
                NSLog("[aterm] telepty binary not found")
                return
            }

            NSLog("[aterm] starting telepty daemon from %@", teleptyPath)
            let process = Process()
            process.executableURL = URL(fileURLWithPath: teleptyPath)
            process.arguments = ["daemon"]
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

        // Respect disabled setting: ATERM_TAILSCALE_ENABLED=0 or UserDefaults
        if let envFlag = env["ATERM_TAILSCALE_ENABLED"], envFlag == "0" || envFlag.lowercased() == "false" || envFlag.lowercased() == "no" {
            NSLog("[aterm] tailscale disabled via ATERM_TAILSCALE_ENABLED")
            return
        }
        if !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled") && UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil {
            NSLog("[aterm] tailscale disabled in settings")
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
        shouldSelect: Bool
    ) {
        let workspaceID = UUID()
        let baselineChildPIDs = directChildProcessIDs(of: ProcessInfo.processInfo.processIdentifier)
        let terminalView = TerminalView(frame: terminalContainerView.bounds)
        terminalView.initialWorkingDirectory = cwd
        terminalView.autoresizingMask = [.width, .height]
        terminalView.isHidden = true
        terminalContainerView.addSubview(terminalView)

        let workspace = ManagedWorkspace(
            id: workspaceID,
            name: uniqueWorkspaceName(for: name, cwd: cwd, excluding: nil),
            cwd: cwd,
            launchCommand: command,
            customCommand: customCommand,
            terminalView: terminalView,
            createdAt: Date(),
            baselineChildPIDs: baselineChildPIDs
        )
        managedWorkspaces[workspaceID] = workspace
        workspaceOrder.append(workspaceID)

        if shouldSelect {
            selectWorkspace(workspaceID)
        } else {
            rebuildSidebarState()
        }

        if let bootstrapCommand = command.bootstrapCommand(customCommand: customCommand) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                self?.bootstrapWorkspace(id: workspaceID, command: bootstrapCommand)
            }
        }

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            self?.refreshWorkspaceProcesses()
        }

        saveWorkspaces()
    }

    private func bootstrapWorkspace(id: UUID, command: String) {
        guard let workspace = managedWorkspaces[id],
              let core = workspace.terminalView.corePointer else {
            return
        }

        let text = command + "\n"
        text.withCString { ptr in
            aterm_core_write_pty(core, ptr, text.utf8.count)
        }
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
    }

    private func renameWorkspace(id: UUID, to nextName: String) {
        guard let workspace = managedWorkspaces[id] else { return }
        let trimmed = nextName.trimmingCharacters(in: .whitespacesAndNewlines)
        workspace.name = uniqueWorkspaceName(
            for: trimmed,
            cwd: workspace.cwd,
            excluding: id
        )
        rebuildSidebarState()
    }

    private func closeWorkspace(id: UUID) {
        guard let workspace = managedWorkspaces.removeValue(forKey: id) else { return }

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

    private func rebuildSidebarState() {
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
                createdAt: workspace.createdAt
            )
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
        let appPID = ProcessInfo.processInfo.processIdentifier
        let currentChildren = directChildProcessIDs(of: appPID)
        var assignedRootPIDs = Set(managedWorkspaces.values.compactMap(\.rootProcessID))

        for id in workspaceOrder {
            guard let workspace = managedWorkspaces[id], workspace.rootProcessID == nil else {
                continue
            }

            let candidates = currentChildren
                .subtracting(workspace.baselineChildPIDs)
                .subtracting(assignedRootPIDs)
                .subtracting(excludedChildPIDs)
            if let candidate = candidates.max() {
                workspace.rootProcessID = candidate
                assignedRootPIDs.insert(candidate)
            }
        }

        for id in workspaceOrder {
            guard let workspace = managedWorkspaces[id] else { continue }

            guard let rootPID = workspace.rootProcessID else {
                workspace.status = "starting"
                workspace.foregroundProcessName = workspace.launchCommand.displayTitle(
                    customCommand: workspace.customCommand
                )
                continue
            }

            if !currentChildren.contains(rootPID) && !processExists(rootPID) {
                workspace.status = "dead"
                workspace.foregroundProcessName = workspace.launchCommand.displayTitle(
                    customCommand: workspace.customCommand
                )
                continue
            }

            workspace.status = "running"
            workspace.foregroundProcessName = foregroundProcessName(forRootPID: rootPID)
                ?? processName(for: rootPID)
                ?? workspace.launchCommand.displayTitle(customCommand: workspace.customCommand)
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

    init(
        id: UUID,
        name: String,
        cwd: String,
        launchCommand: WorkspaceLaunchCommand,
        customCommand: String,
        terminalView: TerminalView,
        createdAt: Date,
        baselineChildPIDs: Set<Int32>
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
    }
}

private struct TTYProcessSnapshot {
    let pid: Int32
    let ppid: Int32
    let stat: String
    let command: String
}
