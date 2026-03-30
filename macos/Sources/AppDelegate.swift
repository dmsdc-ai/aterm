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

    func applicationDidFinishLaunching(_ notification: Notification) {
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
        createWorkspace(
            name: "main",
            command: .zsh,
            customCommand: "",
            cwd: NSHomeDirectory(),
            shouldSelect: true
        )
        startProcessPolling()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        processPollTimer?.invalidate()
        processPollTimer = nil
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
