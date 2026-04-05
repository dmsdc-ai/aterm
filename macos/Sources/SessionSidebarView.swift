import Foundation
import SwiftUI

enum WorkspaceLaunchCommand: String, CaseIterable, Identifiable {
    case zsh
    case claude
    case codex
    case gemini
    case custom

    var id: String { rawValue }

    var title: String { rawValue }

    var accent: Color {
        switch self {
        case .zsh:
            return Color(nsColor: AtermTheme.textSecondary)
        case .claude:
            return Color(nsColor: AtermTheme.accent)
        case .codex:
            return Color(nsColor: AtermTheme.info)
        case .gemini:
            return Color(nsColor: AtermTheme.gemini)
        case .custom:
            return Color(nsColor: AtermTheme.textSecondary)
        }
    }

    func bootstrapCommand(customCommand: String, cliArgs: String = "") -> String? {
        switch self {
        case .zsh:
            return nil
        case .claude:
            let args = cliArgs.trimmingCharacters(in: .whitespacesAndNewlines)
            return "claude " + (args.isEmpty ? "--dangerously-skip-permissions --continue" : args)
        case .codex:
            let args = cliArgs.trimmingCharacters(in: .whitespacesAndNewlines)
            return "codex " + (args.isEmpty ? "resume --last --dangerously-bypass-approvals-and-sandbox" : args)
        case .gemini:
            let args = cliArgs.trimmingCharacters(in: .whitespacesAndNewlines)
            return "gemini " + (args.isEmpty ? "resume -y" : args)
        case .custom:
            let trimmed = customCommand.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        }
    }

    func displayTitle(customCommand: String) -> String {
        switch self {
        case .custom:
            let trimmed = customCommand.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return "custom" }
            return trimmed.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? trimmed
        default:
            return title
        }
    }

    var cliIcon: String {
        switch self {
        case .claude:
            return "🤖"
        case .codex:
            return "🦊"
        case .gemini:
            return "💎"
        case .zsh, .custom:
            return "🐚"
        }
    }
}

struct SidebarWorkspace: Identifiable, Equatable {
    let id: String
    var name: String
    var cwd: String
    var launchCommand: WorkspaceLaunchCommand
    var customCommand: String
    var foregroundProcessName: String
    var status: String
    var cliIcon: String
    var statusEmoji: String
    var createdAt: Date
    var lastActivityAt: Date
    var isSystem: Bool = false
}

struct WorkspaceDraft: Identifiable, Equatable {
    let id = UUID()
    let cwd: String
    var name: String = ""
    var command: WorkspaceLaunchCommand = .zsh
    var customCommand: String = ""
    var cliArgs: String = ""

    var folderName: String {
        let trimmed = cwd.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !trimmed.isEmpty else { return "workspace" }
        return URL(fileURLWithPath: cwd).lastPathComponent
    }
}

struct WorkspaceCreationRequest {
    var preferredCommand: WorkspaceLaunchCommand = .zsh
    var preferredCustomCommand: String = ""
    var initialDirectory: String?
}

private struct IpcWorkspaceListResponse: Decodable {
    let status: String
    let data: [IpcWorkspaceRecord]?
    let message: String?
}

private struct IpcWorkspaceRecord: Decodable {
    let id: String
    let name: String
    let cli: String
    let cwd: String
    let status: String
    let customCommand: String?
    let createdAt: String?
    let lastActivityAt: String?
    let isSystem: Bool?

    enum CodingKeys: String, CodingKey {
        case id, name, cli, cwd, status
        case customCommand = "custom_command"
        case createdAt = "created_at"
        case lastActivityAt = "last_activity_at"
        case isSystem = "is_system"
    }
}

final class WorkspaceSidebarModel: ObservableObject {
    @Published var workspaces: [SidebarWorkspace] = []
    @Published var selectedWorkspaceName: String?
    @Published var creationDrafts: [WorkspaceDraft] = []
    @Published var isCreateSheetPresented = false

    private var eventSubscriberFD: Int32 = -1
    private var eventQueue: DispatchQueue?
    private var fallbackTimer: Timer?
    private var coalesceWorkItem: DispatchWorkItem?
    private var lastRefreshTime: CFAbsoluteTime = 0
    private var refreshInFlight = false
    /// Last seen IPC sequence number for gap detection
    private var lastSeq: UInt64 = 0

    deinit {
        stopAutoRefresh()
    }

    func startAutoRefresh() {
        refreshWorkspaces()
        startEventSubscription()
    }

    func stopAutoRefresh() {
        fallbackTimer?.invalidate()
        fallbackTimer = nil
        coalesceWorkItem?.cancel()
        coalesceWorkItem = nil
        let fd = eventSubscriberFD
        eventSubscriberFD = -1
        if fd >= 0 { Darwin.close(fd) }
        eventQueue = nil
    }

    func refreshWorkspaces() {
        guard !refreshInFlight else { return }
        refreshInFlight = true

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let refreshed = Self.fetchWorkspacesFromIPC()
            DispatchQueue.main.async {
                if let refreshed {
                    self?.workspaces = refreshed
                }
                self?.refreshInFlight = false
            }
        }
    }

    // MARK: - IPC Event Subscription

    private func startEventSubscription() {
        let fd = eventSubscriberFD
        if fd >= 0 { Darwin.close(fd) }
        eventSubscriberFD = -1
        fallbackTimer?.invalidate()
        fallbackTimer = nil

        let queue = DispatchQueue(label: "com.aterm.sidebar-events", qos: .userInitiated)
        eventQueue = queue
        queue.async { [weak self] in
            self?.connectAndSubscribe()
        }
    }

    private func connectAndSubscribe() {
        guard let pathPtr = aterm_ipc_socket_path() else {
            startFallbackPolling()
            return
        }
        let socketPath = String(cString: pathPtr)
        aterm_core_free_string(pathPtr)
        guard !socketPath.isEmpty else {
            startFallbackPolling()
            return
        }

        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            startFallbackPolling()
            return
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutablePointer(to: &addr.sun_path.0) { pathBuf in
            socketPath.withCString { src in _ = strcpy(pathBuf, src) }
        }
        let connected = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(fd, sa, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            Darwin.close(fd)
            startFallbackPolling()
            return
        }

        eventSubscriberFD = fd

        // Send Subscribe action (empty events = all events)
        let msg = "{\"action\":\"Subscribe\",\"events\":[]}\n"
        msg.withCString { ptr in _ = Darwin.write(fd, ptr, Int(strlen(ptr))) }

        // Read event lines (blocks until data or close)
        var readBuf = [UInt8](repeating: 0, count: 4096)
        var partial = Data()

        while eventSubscriberFD >= 0 {
            let n = Darwin.read(fd, &readBuf, readBuf.count)
            if n <= 0 { break }

            partial.append(contentsOf: readBuf[0..<n])

            while let newline = partial.firstIndex(of: 0x0A) {
                let lineData = partial[partial.startIndex..<newline]
                partial = Data(partial[partial.index(after: newline)...])

                guard let str = String(data: Data(lineData), encoding: .utf8),
                      !str.isEmpty else { continue }

                // Try to parse JSON for snapshot/seq handling
                if let jsonData = str.data(using: .utf8),
                   let json = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any] {

                    // Handle snapshot (initial Subscribe response or re-snapshot)
                    if let dataObj = json["data"] as? [String: Any],
                       let eventType = dataObj["type"] as? String,
                       eventType == "Snapshot" {
                        if let seq = dataObj["seq"] as? UInt64 {
                            lastSeq = seq
                        }
                        // Snapshot contains full workspace list — apply directly
                        scheduleCoalescedRefresh()
                        continue
                    }

                    // Handle inline Snapshot event (gap re-sync from server)
                    if let eventType = json["type"] as? String, eventType == "Snapshot" {
                        if let seq = json["seq"] as? UInt64 {
                            lastSeq = seq
                        }
                        scheduleCoalescedRefresh()
                        continue
                    }

                    // Regular event — check seq for gap detection
                    if let eventSeq = json["seq"] as? UInt64 {
                        if lastSeq > 0 && eventSeq > lastSeq + 1 {
                            // Gap detected — server should send re-snapshot automatically,
                            // but trigger a full refresh as fallback
                            scheduleCoalescedRefresh()
                        }
                        lastSeq = eventSeq
                    }

                    // Skip non-event lines (e.g. Subscribe OK status response)
                    guard json["type"] != nil else { continue }
                    scheduleCoalescedRefresh()
                    continue
                }

                // Fallback: skip non-event responses
                if !str.contains("\"type\"") { continue }
                scheduleCoalescedRefresh()
            }
        }

        // Stream disconnected — fallback to polling
        Darwin.close(fd)
        if eventSubscriberFD == fd { eventSubscriberFD = -1 }
        startFallbackPolling()
    }

    /// Leading-edge throttle: first event fires immediately, then coalesce within 25ms.
    private func scheduleCoalescedRefresh() {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let now = CFAbsoluteTimeGetCurrent()
            let elapsed = now - self.lastRefreshTime

            self.coalesceWorkItem?.cancel()

            if elapsed >= 0.025 {
                self.lastRefreshTime = now
                self.refreshWorkspaces()
            } else {
                let delay = 0.025 - elapsed
                let item = DispatchWorkItem { [weak self] in
                    guard let self else { return }
                    self.lastRefreshTime = CFAbsoluteTimeGetCurrent()
                    self.refreshWorkspaces()
                }
                self.coalesceWorkItem = item
                DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: item)
            }
        }
    }

    /// Graceful degradation: poll every 5s if event stream disconnects.
    private func startFallbackPolling() {
        DispatchQueue.main.async { [weak self] in
            guard let self, self.fallbackTimer == nil else { return }
            NSLog("[aterm-sidebar] event stream disconnected, falling back to 5s polling")
            self.fallbackTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { [weak self] _ in
                self?.refreshWorkspaces()
            }
            self.fallbackTimer?.tolerance = 1.0
        }
    }

    func presentCreationDrafts(for urls: [URL], preferredCommand: WorkspaceLaunchCommand) {
        presentCreationDrafts(
            for: urls,
            preferredCommand: preferredCommand,
            preferredCustomCommand: ""
        )
    }

    func presentCreationDrafts(
        for urls: [URL],
        preferredCommand: WorkspaceLaunchCommand,
        preferredCustomCommand: String
    ) {
        let directories = urls.filter { $0.hasDirectoryPath || FileManager.default.directoryExists(at: $0) }
        guard !directories.isEmpty else { return }

        creationDrafts = directories.map {
            var draft = WorkspaceDraft(
                cwd: $0.path,
                command: preferredCommand,
                customCommand: preferredCustomCommand
            )
            draft.cliArgs = AtermSettings.shared.cliDefaults[preferredCommand.rawValue] ?? ""
            return draft
        }
        isCreateSheetPresented = true
    }

    func dismissCreationSheet() {
        creationDrafts = []
        isCreateSheetPresented = false
    }

    private static func fetchWorkspacesFromIPC() -> [SidebarWorkspace]? {
        let request = #"{"action":"ListWorkspaces"}"#
        let responsePtr = request.withCString { ptr in
            aterm_dispatch(ptr, request.utf8.count)
        }

        guard let responsePtr else { return nil }
        let responseJson = String(cString: responsePtr)
        aterm_core_free_string(responsePtr)

        guard let data = responseJson.data(using: .utf8),
              let response = try? JSONDecoder().decode(IpcWorkspaceListResponse.self, from: data),
              response.status == "Data",
              let records = response.data else {
            return nil
        }

        return records.map(Self.sidebarWorkspace(from:))
    }

    private static func sidebarWorkspace(from record: IpcWorkspaceRecord) -> SidebarWorkspace {
        let command = WorkspaceLaunchCommand(rawValue: record.cli) ?? .custom
        let customCommand: String
        if command == .custom {
            customCommand = record.customCommand ?? record.cli
        } else {
            customCommand = record.customCommand ?? ""
        }
        let createdAt = parseTimestamp(record.createdAt) ?? Date()
        let lastActivityAt = parseTimestamp(record.lastActivityAt) ?? createdAt
        let useAscii = AtermSettings.shared.useAsciiIcons

        return SidebarWorkspace(
            id: record.id,
            name: record.name,
            cwd: record.cwd,
            launchCommand: command,
            customCommand: customCommand,
            foregroundProcessName: command.displayTitle(customCommand: customCommand),
            status: record.status,
            cliIcon: sidebarCliIcon(for: command, useAscii: useAscii),
            statusEmoji: sidebarStatusIcon(for: record.status, useAscii: useAscii),
            createdAt: createdAt,
            lastActivityAt: lastActivityAt,
            isSystem: record.isSystem ?? false
        )
    }

    private static func parseTimestamp(_ value: String?) -> Date? {
        guard let value, !value.isEmpty else { return nil }
        if let parsed = sidebarWorkspaceDateFormatter.date(from: value) {
            return parsed
        }
        return ISO8601DateFormatter().date(from: value)
    }
}

private let sidebarWorkspaceDateFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
}()

private func sidebarCliIcon(for command: WorkspaceLaunchCommand, useAscii: Bool) -> String {
    if useAscii {
        switch command {
        case .claude: return "[C]"
        case .codex: return "[X]"
        case .gemini: return "[G]"
        case .zsh, .custom: return "[S]"
        }
    }

    return command.cliIcon
}

private func sidebarStatusIcon(for status: String, useAscii: Bool) -> String {
    if useAscii {
        switch status {
        case "working": return "[*]"
        case "idle": return "[-]"
        case "dead": return "[!]"
        case "starting", "restarting": return "[>]"
        default: return "[?]"
        }
    }

    switch status {
    case "working": return "🔨"
    case "idle": return "💤"
    case "dead": return "🔴"
    case "starting", "restarting": return "🔄"
    default: return ""
    }
}

// MARK: - Task Queue

struct TaskQueueItem: Identifiable, Codable {
    let id: Int
    let desc: String
    let priority: String
    let status: String
    let session: String?
    let note: String?
}

private struct TaskQueueFile: Codable {
    let tasks: [TaskQueueItem]
}

final class TaskQueueLoader: ObservableObject {
    @Published var tasks: [TaskQueueItem] = []

    private var fileSource: DispatchSourceFileSystemObject?

    var activeCount: Int { tasks.filter { $0.status == "in_progress" }.count }
    var totalCount: Int { tasks.count }

    func startAutoRefresh() {
        load()
        startFileWatcher()
    }

    func stopAutoRefresh() {
        fileSource?.cancel()
        fileSource = nil
    }

    deinit { stopAutoRefresh() }

    private func startFileWatcher() {
        stopAutoRefresh()
        guard let path = findTaskQueuePath() else { return }

        let fd = Darwin.open(path, O_EVTONLY)
        guard fd >= 0 else { return }

        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .rename, .delete],
            queue: DispatchQueue.global(qos: .utility)
        )
        source.setEventHandler { [weak self] in
            guard let self else { return }
            if source.data.contains(.delete) || source.data.contains(.rename) {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
                    self?.load()
                    self?.startFileWatcher()
                }
            } else {
                self.load()
            }
        }
        source.setCancelHandler { Darwin.close(fd) }
        fileSource = source
        source.resume()
    }

    private func findTaskQueuePath() -> String? {
        let knownPath = NSHomeDirectory() + "/projects/aigentry-orchestrator/state/task-queue.json"
        if FileManager.default.fileExists(atPath: knownPath) { return knownPath }

        let projectsDir = NSHomeDirectory() + "/projects"
        if let entries = try? FileManager.default.contentsOfDirectory(atPath: projectsDir) {
            for entry in entries {
                let candidate = projectsDir + "/" + entry + "/state/task-queue.json"
                if FileManager.default.fileExists(atPath: candidate) { return candidate }
            }
        }
        return nil
    }

    private func load() {
        let knownPath = NSHomeDirectory() + "/projects/aigentry-orchestrator/state/task-queue.json"
        if loadFrom(path: knownPath) { return }

        let fm = FileManager.default
        let projectsDir = NSHomeDirectory() + "/projects"
        if let entries = try? fm.contentsOfDirectory(atPath: projectsDir) {
            for entry in entries {
                let candidate = projectsDir + "/" + entry + "/state/task-queue.json"
                if loadFrom(path: candidate) { return }
            }
        }
    }

    @discardableResult
    private func loadFrom(path: String) -> Bool {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let file = try? JSONDecoder().decode(TaskQueueFile.self, from: data)
        else { return false }
        DispatchQueue.main.async { self.tasks = file.tasks }
        return true
    }
}

// MARK: - Task Row

struct TaskRowView: View {
    let task: TaskQueueItem
    @State private var showPopover = false

    var body: some View {
        HStack(spacing: 6) {
            Text(statusEmoji)
                .font(.system(size: 11))
            Text("#\(task.id) \(task.desc)")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer()
            Text(task.status)
                .font(.system(size: 9, weight: .medium))
                .foregroundColor(statusColor)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color(nsColor: AtermTheme.secondaryRowBackground))
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture { showPopover = true }
        .popover(isPresented: $showPopover) {
            taskDetailPopover
        }
    }

    private var statusEmoji: String {
        switch task.status {
        case "in_progress": return "🔨"
        case "pending":     return "⏳"
        case "completed":   return "✅"
        case "blocked":     return "🚫"
        case "delegated":   return "📤"
        default:            return "⏳"
        }
    }

    private var statusColor: Color {
        switch task.status {
        case "completed":   return Color(nsColor: AtermTheme.statusSuccess)
        case "in_progress": return Color(nsColor: AtermTheme.statusWarning)
        case "pending":     return Color(nsColor: AtermTheme.textMuted)
        case "blocked":     return Color(nsColor: AtermTheme.statusDanger)
        case "delegated":   return Color(nsColor: AtermTheme.info)
        default:            return Color(nsColor: AtermTheme.textMuted)
        }
    }

    private var taskDetailPopover: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("#\(task.id)")
                    .font(.system(size: 13, weight: .bold, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                Text(task.priority)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundColor(priorityColor)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 2)
                    .background(priorityColor.opacity(0.15))
                    .cornerRadius(4)
                Spacer()
                Text(statusEmoji + " " + task.status)
                    .font(.system(size: 11))
                    .foregroundColor(statusColor)
            }

            Text(task.desc)
                .font(.system(size: 12))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                .fixedSize(horizontal: false, vertical: true)

            if let session = task.session {
                HStack(spacing: 4) {
                    Text(AtermLocalization.text(ko: "세션:", en: "Session:"))
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    Text(session)
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(Color(nsColor: AtermTheme.info))
                }
            }

            if let note = task.note, !note.isEmpty {
                Text(note)
                    .font(.system(size: 11))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(14)
        .frame(minWidth: 260, maxWidth: 340)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }

    private var priorityColor: Color {
        switch task.priority {
        case "P0": return Color(nsColor: AtermTheme.statusDanger)
        case "P1": return Color(nsColor: AtermTheme.statusWarning)
        default:   return Color(nsColor: AtermTheme.textMuted)
        }
    }
}

// MARK: - Sidebar

struct SessionSidebarView: View {
    @ObservedObject var busClient: TeleptyBusClient
    @ObservedObject var workspaceModel: WorkspaceSidebarModel

    let onBeginWorkspaceCreation: (WorkspaceCreationRequest) -> Void
    let onCreateWorkspaces: ([WorkspaceDraft]) -> Void
    let onSelectWorkspace: (String) -> Void
    let onRenameWorkspace: (String, String) -> Void
    let onCloseWorkspace: (String) -> Void
    let onChangeWorkspaceCLI: (String, String) -> Void
    let onRestartWorkspace: (String) -> Void
    let onAttachExternalSession: (String) -> Void
    let onOpenSettings: () -> Void

    @StateObject private var taskLoader = TaskQueueLoader()
    @State private var renameTarget: SidebarWorkspace?
    @State private var renameText = ""
    @State private var selectedWorkspaceIds: Set<String> = []
    @State private var lastSelectionIndex: Int?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Sessions")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                Spacer()
                Circle()
                    .fill(busClient.connected ? Color(nsColor: AtermTheme.statusSuccess) : Color(nsColor: AtermTheme.statusDanger))
                    .frame(width: 8, height: 8)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(Color(nsColor: AtermTheme.sidebarHeaderBackground))

            Divider().overlay(Color(nsColor: AtermTheme.border))

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    workspaceHeader

                    if workspaceModel.workspaces.isEmpty {
                        Text("No workspaces")
                            .font(.system(size: 11))
                            .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                    } else {
                        let orchestrators = workspaceModel.workspaces.enumerated().filter { $0.element.isSystem }
                        let regulars = workspaceModel.workspaces.enumerated().filter { !$0.element.isSystem }

                        // — Orchestrator section (#181)
                        if !orchestrators.isEmpty {
                            HStack(spacing: 5) {
                                Image(systemName: "tower.broadcast")
                                    .font(.system(size: 9, weight: .semibold))
                                    .foregroundColor(Color(nsColor: AtermTheme.orchestrator))
                                Text("ORCHESTRATOR")
                                    .font(.system(size: 9, weight: .semibold))
                                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                            }
                            .padding(.horizontal, 12)
                            .padding(.top, 8)
                            .padding(.bottom, 4)

                            ForEach(Array(orchestrators), id: \.element.id) { item in
                                let index = item.offset
                                let workspace = item.element
                                OrchestratorRowView(
                                    workspace: workspace,
                                    isActive: workspace.id == workspaceModel.selectedWorkspaceName,
                                    isInMultiSelection: selectedWorkspaceIds.contains(workspace.id),
                                    onSelect: {
                                        handleWorkspaceClick(workspace: workspace, index: index)
                                    }
                                )
                                .contextMenu {
                                    workspaceContextMenu(for: workspace)
                                }
                            }

                            Divider()
                                .overlay(Color(nsColor: AtermTheme.orchestratorBorder))
                                .padding(.vertical, 8)
                                .padding(.horizontal, 8)
                        }

                        // — Regular workspaces
                        ForEach(Array(regulars), id: \.element.id) { item in
                            let index = item.offset
                            let workspace = item.element
                            WorkspaceRowView(
                                workspace: workspace,
                                isActive: workspace.id == workspaceModel.selectedWorkspaceName,
                                isInMultiSelection: selectedWorkspaceIds.contains(workspace.id),
                                onSelect: {
                                    handleWorkspaceClick(workspace: workspace, index: index)
                                }
                            )
                            .contextMenu {
                                workspaceContextMenu(for: workspace)
                            }
                        }
                    }

                    if !externalSessions.isEmpty {
                        sectionHeader("External Sessions")

                        ForEach(externalSessions) { session in
                            SessionRowView(session: session, onAttach: {
                                onAttachExternalSession(session.id)
                            })
                        }
                    }

                    if AtermSettings.shared.showTaskBoard && !taskLoader.tasks.isEmpty {
                        taskBoardSection
                    }
                }
                .padding(.vertical, 4)
            }
            .onAppear {
                taskLoader.startAutoRefresh()
                workspaceModel.startAutoRefresh()
            }
            .onDisappear {
                workspaceModel.stopAutoRefresh()
            }

            Divider().overlay(Color(nsColor: AtermTheme.border))

            Button(action: onOpenSettings) {
                HStack(spacing: 6) {
                    Image(systemName: "gearshape")
                        .font(.system(size: 12))
                    Text(AtermLocalization.text(ko: "설정", en: "Settings"))
                        .font(.system(size: 12))
                }
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
            .buttonStyle(.plain)
        }
        .background(Color(nsColor: AtermTheme.sidebarBackground))
        .sheet(
            isPresented: Binding(
                get: { workspaceModel.isCreateSheetPresented },
                set: { presented in
                    if !presented {
                        workspaceModel.dismissCreationSheet()
                    }
                }
            )
        ) {
            WorkspaceCreateSheet(
                drafts: Binding(
                    get: { workspaceModel.creationDrafts },
                    set: { workspaceModel.creationDrafts = $0 }
                ),
                onCancel: { workspaceModel.dismissCreationSheet() },
                onCreate: {
                    onCreateWorkspaces(workspaceModel.creationDrafts)
                    workspaceModel.dismissCreationSheet()
                }
            )
        }
        .sheet(item: $renameTarget) { workspace in
            WorkspaceRenameSheet(
                workspaceName: workspace.name,
                renameText: $renameText,
                onCancel: {
                    renameTarget = nil
                    renameText = ""
                },
                onRename: {
                    onRenameWorkspace(workspace.id, renameText)
                    renameTarget = nil
                    renameText = ""
                }
            )
        }
    }

    private var taskBoardSection: some View {
        Group {
            sectionHeader(
                AtermLocalization.text(
                    ko: "태스크 (\(taskLoader.activeCount) 진행 중 / \(taskLoader.totalCount) 전체)",
                    en: "TASKS (\(taskLoader.activeCount) active / \(taskLoader.totalCount) total)"
                )
            )
            ForEach(taskLoader.tasks) { task in
                TaskRowView(task: task)
            }
        }
    }

    private var externalSessions: [TeleptySession] {
        let ownNames = Set(workspaceModel.workspaces.map(\.name))
        let ownIDs = Set(workspaceModel.workspaces.map(\.id))
        let ownCWDs = Set(workspaceModel.workspaces.map(\.cwd))
        return busClient.sessions.filter { session in
            // Exclude if session id matches any internal workspace name or id
            if ownNames.contains(session.id) || ownIDs.contains(session.id) {
                return false
            }
            // Exclude if registered by this aterm instance
            if session.termProgram == "aterm" {
                return false
            }
            // Exclude if session cwd matches any internal workspace cwd
            if let cwd = session.cwd, ownCWDs.contains(cwd) {
                return false
            }
            return true
        }
    }

    private var workspaceHeader: some View {
        HStack(spacing: 8) {
            Text("My Workspaces")
                .font(.system(size: 10, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                .textCase(.uppercase)
            Spacer()
            Button {
                onBeginWorkspaceCreation(WorkspaceCreationRequest())
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                    .frame(width: 18, height: 18)
                    .background(Color(nsColor: AtermTheme.panelInsetBackground))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 12)
        .padding(.top, 10)
        .padding(.bottom, 4)
    }

    private func sectionHeader(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 10, weight: .semibold))
            .foregroundColor(Color(nsColor: AtermTheme.textMuted))
            .textCase(.uppercase)
            .padding(.horizontal, 12)
            .padding(.top, 10)
            .padding(.bottom, 4)
    }

    // MARK: - Multi-Selection

    private func handleWorkspaceClick(workspace: SidebarWorkspace, index: Int) {
        let flags = NSEvent.modifierFlags
        let hasCmd = flags.contains(.command)
        let hasShift = flags.contains(.shift)

        if hasCmd && hasShift {
            // Cmd+Shift+click: union range into existing selection
            if let lastIndex = lastSelectionIndex {
                let lo = min(lastIndex, index)
                let hi = max(lastIndex, index)
                for i in lo...hi where i < workspaceModel.workspaces.count {
                    selectedWorkspaceIds.insert(workspaceModel.workspaces[i].id)
                }
            } else {
                selectedWorkspaceIds.insert(workspace.id)
                lastSelectionIndex = index
            }
        } else if hasCmd {
            // Cmd+click: toggle individual
            if selectedWorkspaceIds.contains(workspace.id) {
                selectedWorkspaceIds.remove(workspace.id)
            } else {
                selectedWorkspaceIds.insert(workspace.id)
            }
            lastSelectionIndex = index
        } else if hasShift {
            // Shift+click: range select from lastSelectionIndex
            if let lastIndex = lastSelectionIndex {
                selectedWorkspaceIds.removeAll()
                let lo = min(lastIndex, index)
                let hi = max(lastIndex, index)
                for i in lo...hi where i < workspaceModel.workspaces.count {
                    selectedWorkspaceIds.insert(workspaceModel.workspaces[i].id)
                }
            } else {
                selectedWorkspaceIds = [workspace.id]
                lastSelectionIndex = index
            }
        } else {
            // Plain click: reset to single selection
            selectedWorkspaceIds = [workspace.id]
            lastSelectionIndex = index
        }

        // Always switch terminal to clicked workspace
        onSelectWorkspace(workspace.id)
    }

    @ViewBuilder
    private func workspaceContextMenu(for workspace: SidebarWorkspace) -> some View {
        let isInSelection = selectedWorkspaceIds.contains(workspace.id) && selectedWorkspaceIds.count > 1
        let targets = isInSelection
            ? workspaceModel.workspaces.filter { selectedWorkspaceIds.contains($0.id) }
            : [workspace]
        let count = targets.count
        let nonSystemTargets = targets.filter { !$0.isSystem }

        Button("New Session") {
            onBeginWorkspaceCreation(
                WorkspaceCreationRequest(
                    preferredCommand: workspace.launchCommand,
                    preferredCustomCommand: workspace.customCommand,
                    initialDirectory: workspace.cwd
                )
            )
        }

        if !workspace.isSystem {
            if count == 1 {
                Button("Rename") {
                    renameTarget = workspace
                    renameText = workspace.name
                }
            }

            Menu("Change CLI") {
                ForEach(WorkspaceLaunchCommand.allCases.filter { $0 != .custom }) { command in
                    Button(command.title) {
                        for target in nonSystemTargets {
                            onChangeWorkspaceCLI(target.id, command.rawValue)
                        }
                    }
                    .disabled(count == 1 && command == workspace.launchCommand)
                }
            }

            Divider()

            Button(count > 1 ? "Restart Workspaces" : "Restart Workspace") {
                for target in nonSystemTargets {
                    onRestartWorkspace(target.id)
                }
            }

            Button(count > 1 ? "Close Workspaces" : "Close Workspace", role: .destructive) {
                for target in nonSystemTargets {
                    onCloseWorkspace(target.id)
                }
                selectedWorkspaceIds.removeAll()
                lastSelectionIndex = nil
            }

            Button("Close Other Workspaces", role: .destructive) {
                let targetIds = Set(targets.map(\.id))
                for ws in workspaceModel.workspaces where !targetIds.contains(ws.id) && !ws.isSystem {
                    onCloseWorkspace(ws.id)
                }
            }
        }
    }
}

private struct WorkspaceCreateSheet: View {
    @Binding var drafts: [WorkspaceDraft]

    let onCancel: () -> Void
    let onCreate: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(drafts.count == 1 ? "New Workspace" : "New Workspaces")
                .font(.system(size: 16, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach($drafts) { $draft in
                        VStack(alignment: .leading, spacing: 10) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(draft.folderName)
                                    .font(.system(size: 13, weight: .semibold))
                                Text(draft.cwd)
                                    .font(.system(size: 10, design: .monospaced))
                                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                                    .lineLimit(2)
                            }

                            VStack(alignment: .leading, spacing: 6) {
                                Text("Name")
                                    .font(.system(size: 11, weight: .medium))
                                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                                TextField("Optional custom name", text: $draft.name)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Command")
                                    .font(.system(size: 11, weight: .medium))
                                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                                Picker("Command", selection: $draft.command) {
                                    ForEach(WorkspaceLaunchCommand.allCases) { command in
                                        Text(command.title).tag(command)
                                    }
                                }
                                .pickerStyle(.segmented)
                                .onChange(of: draft.command) {
                                    if draft.command != .custom {
                                        draft.cliArgs = AtermSettings.shared.cliDefaults[draft.command.rawValue] ?? ""
                                    }
                                }

                                if draft.command == .custom {
                                    TextField("e.g. opencode --full-auto", text: $draft.customCommand)
                                        .textFieldStyle(.roundedBorder)
                                        .font(.system(size: 12, design: .monospaced))
                                } else if draft.command == .zsh {
                                    Text("zsh")
                                        .font(.system(size: 11, design: .monospaced))
                                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                                } else {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text("Arguments")
                                            .font(.system(size: 10, weight: .medium))
                                            .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                                        TextField(
                                            AtermSettings.defaultCliArgs[draft.command.rawValue] ?? "",
                                            text: $draft.cliArgs
                                        )
                                        .textFieldStyle(.roundedBorder)
                                        .font(.system(size: 12, design: .monospaced))

                                        Text(draft.command.bootstrapCommand(customCommand: draft.customCommand, cliArgs: draft.cliArgs) ?? "")
                                            .font(.system(size: 10, design: .monospaced))
                                            .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                                            .lineLimit(2)
                                    }
                                }
                            }
                        }
                        .padding(12)
                        .background(Color(nsColor: AtermTheme.panelInsetBackground))
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                    }
                }
            }

            Spacer()

            HStack {
                Spacer()
                Button("Cancel", action: onCancel)
                Button("Create") {
                    // Save edited CLI args as new defaults
                    for draft in drafts {
                        if draft.command != .custom && draft.command != .zsh {
                            let args = draft.cliArgs.trimmingCharacters(in: .whitespacesAndNewlines)
                            if !args.isEmpty {
                                AtermSettings.shared.cliDefaults[draft.command.rawValue] = args
                            }
                        }
                    }
                    AtermSettings.shared.save()
                    onCreate()
                }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canCreate)
            }
        }
        .padding(20)
        .frame(width: 460, height: min(CGFloat(220 + drafts.count * 110), CGFloat(560)))
        .background(Color(nsColor: AtermTheme.panelBackground))
    }

    private var canCreate: Bool {
        drafts.allSatisfy { draft in
            if draft.command == .custom {
                return !draft.customCommand.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            }
            return true
        }
    }
}

private struct WorkspaceRenameSheet: View {
    let workspaceName: String
    @Binding var renameText: String

    let onCancel: () -> Void
    let onRename: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Rename Workspace")
                .font(.system(size: 16, weight: .semibold))
                .foregroundColor(Color(nsColor: AtermTheme.textPrimary))

            Text(workspaceName)
                .font(.system(size: 11, design: .monospaced))
                .foregroundColor(Color(nsColor: AtermTheme.textSecondary))

            TextField("Workspace name", text: $renameText)
                .textFieldStyle(.roundedBorder)

            Spacer()

            HStack {
                Spacer()
                Button("Cancel", action: onCancel)
                Button("Rename", action: onRename)
                    .keyboardShortcut(.defaultAction)
                    .disabled(renameText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 320, height: 180)
        .background(Color(nsColor: AtermTheme.panelBackground))
    }
}

struct WorkspaceRowView: View {
    let workspace: SidebarWorkspace
    let isActive: Bool
    let isInMultiSelection: Bool
    let onSelect: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                if AtermSettings.shared.showStatusEmoji {
                    Text(workspace.cliIcon)
                        .font(.system(size: 11))
                } else {
                    Circle()
                        .fill(statusColor)
                        .frame(width: 6, height: 6)
                }
                
                Text(workspace.name)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                    .lineLimit(1)
                
                Spacer()
                
                if AtermSettings.shared.showStatusEmoji {
                    Text(workspace.statusEmoji)
                        .font(.system(size: 11))
                } else {
                    Text(workspace.status)
                        .font(.system(size: 9))
                        .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                }
            }

            HStack(spacing: 6) {
                Text(shortSidebarPath(workspace.cwd))
                    .font(.system(size: 10))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(1)
                Text("·")
                    .font(.system(size: 10))
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                Text(workspace.foregroundProcessName)
                    .font(.system(size: 10))
                    .foregroundColor(workspace.launchCommand.accent.opacity(0.85))
                    .lineLimit(1)
            }

            Text(relativeSidebarTime(workspace.createdAt))
                .font(.system(size: 9))
                .foregroundColor(Color(nsColor: AtermTheme.textMuted))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(
            isActive
                ? Color(nsColor: AtermTheme.selectedRowBackground)
                : isInMultiSelection
                    ? Color(nsColor: AtermTheme.accent).opacity(0.25)
                    : Color(nsColor: AtermTheme.secondaryRowBackground)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(
                    isActive
                        ? Color(nsColor: AtermTheme.selectedRowStroke)
                        : isInMultiSelection
                            ? Color(nsColor: AtermTheme.accent).opacity(0.35)
                            : Color(nsColor: AtermTheme.border).opacity(0.35),
                    lineWidth: 1
                )
        )
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture(perform: onSelect)
    }

    private var statusColor: Color {
        switch workspace.status {
        case "working":
            return Color(nsColor: AtermTheme.statusSuccess)
        case "idle":
            return Color(nsColor: AtermTheme.statusWarning)
        case "dead":
            return Color(nsColor: AtermTheme.statusDanger)
        case "starting":
            return Color(nsColor: AtermTheme.statusWarning)
        case "restarting":
            return Color(nsColor: AtermTheme.statusWarning)
        default:
            return Color(nsColor: AtermTheme.statusWarning)
        }
    }
}

// MARK: - Orchestrator Row (#181)
struct OrchestratorRowView: View {
    let workspace: SidebarWorkspace
    let isActive: Bool
    let isInMultiSelection: Bool
    let onSelect: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            // Left accent border — always visible
            RoundedRectangle(cornerRadius: 1)
                .fill(Color(nsColor: AtermTheme.orchestrator))
                .frame(width: 2)
                .padding(.vertical, 4)

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Image(systemName: "tower.broadcast")
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.orchestrator))

                    Text(workspace.name)
                        .font(.system(size: 11, weight: .semibold, design: .monospaced))
                        .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                        .lineLimit(1)

                    // CTRL badge
                    Text("CTRL")
                        .font(.system(size: 8, weight: .semibold))
                        .foregroundColor(Color(nsColor: AtermTheme.orchestrator))
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Color(nsColor: AtermTheme.orchestratorSubtle))
                        .overlay(
                            RoundedRectangle(cornerRadius: 3)
                                .stroke(Color(nsColor: AtermTheme.orchestratorBorder), lineWidth: 1)
                        )
                        .cornerRadius(3)

                    Spacer()

                    if AtermSettings.shared.showStatusEmoji {
                        Text(workspace.statusEmoji)
                            .font(.system(size: 11))
                    } else {
                        Text(workspace.status)
                            .font(.system(size: 9))
                            .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    }
                }

                HStack(spacing: 6) {
                    Text(shortSidebarPath(workspace.cwd))
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                        .lineLimit(1)
                    Text("·")
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.textMuted))
                    Text(workspace.foregroundProcessName)
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.orchestrator).opacity(0.75))
                        .lineLimit(1)
                }

                Text(relativeSidebarTime(workspace.createdAt))
                    .font(.system(size: 9))
                    .foregroundColor(Color(nsColor: AtermTheme.textMuted))
            }
            .padding(.leading, 8)
            .padding(.trailing, 10)
            .padding(.vertical, 7)
        }
        .background(
            isActive
                ? Color(nsColor: AtermTheme.orchestrator).opacity(0.20)
                : isInMultiSelection
                    ? Color(nsColor: AtermTheme.orchestrator).opacity(0.25)
                    : Color(nsColor: AtermTheme.orchestratorSubtle)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(
                    isActive
                        ? Color(nsColor: AtermTheme.orchestrator).opacity(0.45)
                        : isInMultiSelection
                            ? Color(nsColor: AtermTheme.orchestrator).opacity(0.35)
                            : Color(nsColor: AtermTheme.orchestratorBorder),
                    lineWidth: 1
                )
        )
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture(perform: onSelect)
    }
}

struct SessionRowView: View {
    let session: TeleptySession
    let onAttach: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            // Session ID + status indicator
            HStack(spacing: 6) {
                Circle()
                    .fill(statusColor)
                    .frame(width: 6, height: 6)

                Text(session.id)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(Color(nsColor: AtermTheme.textPrimary))
                    .lineLimit(1)
                    .truncationMode(.middle)

                Spacer()

                if let phase = session.phase {
                    Text(phase.rawValue)
                        .font(.system(size: 9, weight: .medium))
                        .foregroundColor(phaseColor(phase))
                        .padding(.horizontal, 4)
                        .padding(.vertical, 1)
                        .background(phaseColor(phase).opacity(0.15))
                        .cornerRadius(3)
                }
            }

            // Project + command
            HStack(spacing: 4) {
                if let project = session.project {
                    Text(project)
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.info).opacity(0.9))
                }
                if let command = session.command {
                    Text(command)
                        .font(.system(size: 10))
                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                }
            }

            // Current task
            if let task = session.currentTask {
                Text(task)
                    .font(.system(size: 10))
                    .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                    .lineLimit(2)
            }

            // Blocker
            if let blocker = session.blocker {
                HStack(spacing: 3) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 8))
                        .foregroundColor(Color(nsColor: AtermTheme.accent))
                    Text(blocker)
                        .font(.system(size: 9))
                        .foregroundColor(Color(nsColor: AtermTheme.accent))
                        .lineLimit(1)
                }
            }

            // Needs input indicator
            if session.needsInput == true {
                HStack(spacing: 3) {
                    Image(systemName: "keyboard")
                        .font(.system(size: 8))
                        .foregroundColor(Color(nsColor: AtermTheme.statusWarning))
                    Text("Needs input")
                        .font(.system(size: 9))
                        .foregroundColor(Color(nsColor: AtermTheme.statusWarning))
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color(nsColor: AtermTheme.secondaryRowBackground))
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture(perform: onAttach)
        .contextMenu {
            Button("Attach") { onAttach() }
        }
    }

    private var statusColor: Color {
        switch session.status {
        case "running", "active": return Color(nsColor: AtermTheme.statusSuccess)
        case "idle": return Color(nsColor: AtermTheme.statusWarning)
        case "blocked", "error": return Color(nsColor: AtermTheme.accent)
        case "dead": return Color(nsColor: AtermTheme.statusDanger)
        default: return Color(nsColor: AtermTheme.textMuted)
        }
    }

    private func phaseColor(_ phase: TaskPhase) -> Color {
        switch phase {
        case .implementing: return Color(nsColor: AtermTheme.info)
        case .blocked: return Color(nsColor: AtermTheme.accent)
        case .testing: return Color(nsColor: AtermTheme.gemini)
        case .idle: return Color(nsColor: AtermTheme.textMuted)
        }
    }
}

private func relativeSidebarTime(_ date: Date) -> String {
    let elapsed = Date().timeIntervalSince(date)
    if elapsed < 60 { return "just now" }
    if elapsed < 3600 { return "\(Int(elapsed / 60))m ago" }
    if elapsed < 86400 { return "\(Int(elapsed / 3600))h ago" }
    return "\(Int(elapsed / 86400))d ago"
}

private func shortSidebarPath(_ cwd: String) -> String {
    let home = NSHomeDirectory()
    if cwd.hasPrefix(home) {
        return "~" + String(cwd.dropFirst(home.count))
    }
    return cwd
}

private extension FileManager {
    func directoryExists(at url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        return fileExists(atPath: url.path, isDirectory: &isDirectory) && isDirectory.boolValue
    }
}
