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

    func bootstrapCommand(customCommand: String) -> String? {
        switch self {
        case .zsh:
            return nil
        case .claude:
            return "claude --dangerously-skip-permissions --continue"
        case .codex:
            return "codex resume --last --dangerously-bypass-approvals-and-sandbox"
        case .gemini:
            return "gemini resume -y"
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
    let id: UUID
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

final class WorkspaceSidebarModel: ObservableObject {
    @Published var workspaces: [SidebarWorkspace] = []
    @Published var selectedWorkspaceID: UUID?
    @Published var creationDrafts: [WorkspaceDraft] = []
    @Published var isCreateSheetPresented = false

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
            WorkspaceDraft(
                cwd: $0.path,
                command: preferredCommand,
                customCommand: preferredCustomCommand
            )
        }
        isCreateSheetPresented = true
    }

    func dismissCreationSheet() {
        creationDrafts = []
        isCreateSheetPresented = false
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

    private var timer: Timer?

    var activeCount: Int { tasks.filter { $0.status == "in_progress" }.count }
    var totalCount: Int { tasks.count }

    func startAutoRefresh() {
        load()
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            self?.load()
        }
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
    let onSelectWorkspace: (UUID) -> Void
    let onRenameWorkspace: (UUID, String) -> Void
    let onCloseWorkspace: (UUID) -> Void
    let onAttachExternalSession: (String) -> Void
    let onOpenSettings: () -> Void

    @StateObject private var taskLoader = TaskQueueLoader()
    @State private var renameTarget: SidebarWorkspace?
    @State private var renameText = ""

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
                        ForEach(workspaceModel.workspaces) { workspace in
                            WorkspaceRowView(
                                workspace: workspace,
                                isSelected: workspace.id == workspaceModel.selectedWorkspaceID,
                                onSelect: { onSelectWorkspace(workspace.id) },
                                onNew: {
                                    onBeginWorkspaceCreation(
                                        WorkspaceCreationRequest(
                                            preferredCommand: workspace.launchCommand,
                                            preferredCustomCommand: workspace.customCommand,
                                            initialDirectory: workspace.cwd
                                        )
                                    )
                                },
                                onRename: {
                                    renameTarget = workspace
                                    renameText = workspace.name
                                },
                                onClose: { onCloseWorkspace(workspace.id) }
                            )
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
            .onAppear { taskLoader.startAutoRefresh() }

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
        let ownWorkspaceIDs = Set(workspaceModel.workspaces.map(\.name))
        return busClient.sessions.filter { !ownWorkspaceIDs.contains($0.id) }
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

                                if draft.command == .custom {
                                    TextField("e.g. opencode --full-auto", text: $draft.customCommand)
                                        .textFieldStyle(.roundedBorder)
                                        .font(.system(size: 12, design: .monospaced))
                                } else {
                                    Text(draft.command.bootstrapCommand(customCommand: draft.customCommand) ?? "zsh")
                                        .font(.system(size: 11, design: .monospaced))
                                        .foregroundColor(Color(nsColor: AtermTheme.textSecondary))
                                        .lineLimit(2)
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
                Button("Create", action: onCreate)
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
    let isSelected: Bool
    let onSelect: () -> Void
    let onNew: () -> Void
    let onRename: () -> Void
    let onClose: () -> Void

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
            isSelected
                ? Color(nsColor: AtermTheme.selectedRowBackground)
                : Color(nsColor: AtermTheme.secondaryRowBackground)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(
                    isSelected
                        ? Color(nsColor: AtermTheme.selectedRowStroke)
                        : Color(nsColor: AtermTheme.border).opacity(0.35),
                    lineWidth: 1
                )
        )
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture(perform: onSelect)
        .contextMenu {
            Button("New Session", action: onNew)
            if !workspace.isSystem {
                Button("Rename", action: onRename)
                Divider()
                Button("Close", role: .destructive, action: onClose)
            }
        }
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
