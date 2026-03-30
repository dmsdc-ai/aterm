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
            return .gray
        case .claude:
            return .orange
        case .codex:
            return .cyan
        case .gemini:
            return .blue
        case .custom:
            return .gray
        }
    }

    func bootstrapCommand(customCommand: String) -> String? {
        switch self {
        case .zsh:
            return nil
        case .claude:
            return "exec claude --dangerously-skip-permissions --continue"
        case .codex:
            return "exec codex resume --last --dangerously-bypass-approvals-and-sandbox"
        case .gemini:
            return "exec gemini resume -y"
        case .custom:
            let trimmed = customCommand.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : "exec \(trimmed)"
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
}

struct SidebarWorkspace: Identifiable, Equatable {
    let id: UUID
    var name: String
    var cwd: String
    var launchCommand: WorkspaceLaunchCommand
    var customCommand: String
    var foregroundProcessName: String
    var status: String
    var createdAt: Date
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

struct SessionSidebarView: View {
    @ObservedObject var busClient: TeleptyBusClient
    @ObservedObject var workspaceModel: WorkspaceSidebarModel

    let onBeginWorkspaceCreation: (WorkspaceCreationRequest) -> Void
    let onCreateWorkspaces: ([WorkspaceDraft]) -> Void
    let onSelectWorkspace: (UUID) -> Void
    let onRenameWorkspace: (UUID, String) -> Void
    let onCloseWorkspace: (UUID) -> Void

    @State private var renameTarget: SidebarWorkspace?
    @State private var renameText = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Sessions")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(.white)
                Spacer()
                Circle()
                    .fill(busClient.connected ? Color.green : Color.red)
                    .frame(width: 8, height: 8)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(Color(nsColor: NSColor(white: 0.15, alpha: 1.0)))

            Divider()

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    workspaceHeader

                    if workspaceModel.workspaces.isEmpty {
                        Text("No workspaces")
                            .font(.system(size: 11))
                            .foregroundColor(.gray)
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

                    if !busClient.sessions.isEmpty {
                        sectionHeader("External Sessions")

                        ForEach(busClient.sessions) { session in
                            SessionRowView(session: session)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
        }
        .background(Color(nsColor: NSColor(white: 0.1, alpha: 1.0)))
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

    private var workspaceHeader: some View {
        HStack(spacing: 8) {
            Text("My Workspaces")
                .font(.system(size: 10, weight: .semibold))
                .foregroundColor(.gray)
                .textCase(.uppercase)
            Spacer()
            Button {
                onBeginWorkspaceCreation(WorkspaceCreationRequest())
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundColor(.white)
                    .frame(width: 18, height: 18)
                    .background(Color.white.opacity(0.08))
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
            .foregroundColor(.gray)
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

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    ForEach($drafts) { $draft in
                        VStack(alignment: .leading, spacing: 10) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(draft.folderName)
                                    .font(.system(size: 13, weight: .semibold))
                                Text(draft.cwd)
                                    .font(.system(size: 10, design: .monospaced))
                                    .foregroundColor(.secondary)
                                    .lineLimit(2)
                            }

                            VStack(alignment: .leading, spacing: 6) {
                                Text("Name")
                                    .font(.system(size: 11, weight: .medium))
                                    .foregroundColor(.secondary)
                                TextField("Optional custom name", text: $draft.name)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Command")
                                    .font(.system(size: 11, weight: .medium))
                                    .foregroundColor(.secondary)
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
                                        .foregroundColor(.secondary)
                                        .lineLimit(2)
                                }
                            }
                        }
                        .padding(12)
                        .background(Color(nsColor: NSColor.controlBackgroundColor))
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

            Text(workspaceName)
                .font(.system(size: 11, design: .monospaced))
                .foregroundColor(.secondary)

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
                Circle()
                    .fill(statusColor)
                    .frame(width: 6, height: 6)
                Text(workspace.name)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(.white)
                    .lineLimit(1)
                Spacer()
                Text(workspace.status)
                    .font(.system(size: 9))
                    .foregroundColor(.gray)
            }

            HStack(spacing: 6) {
                Text(shortSidebarPath(workspace.cwd))
                    .font(.system(size: 10))
                    .foregroundColor(.gray)
                    .lineLimit(1)
                Text("·")
                    .font(.system(size: 10))
                    .foregroundColor(.gray.opacity(0.6))
                Text(workspace.foregroundProcessName)
                    .font(.system(size: 10))
                    .foregroundColor(workspace.launchCommand.accent.opacity(0.85))
                    .lineLimit(1)
            }

            Text(relativeSidebarTime(workspace.createdAt))
                .font(.system(size: 9))
                .foregroundColor(.gray)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(isSelected ? Color.white.opacity(0.08) : Color.white.opacity(0.03))
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(isSelected ? Color.white.opacity(0.18) : Color.clear, lineWidth: 1)
        )
        .cornerRadius(4)
        .padding(.horizontal, 4)
        .contentShape(RoundedRectangle(cornerRadius: 4))
        .onTapGesture(perform: onSelect)
        .contextMenu {
            Button("New Session", action: onNew)
            Button("Rename", action: onRename)
            Divider()
            Button("Close", role: .destructive, action: onClose)
        }
    }

    private var statusColor: Color {
        switch workspace.status {
        case "running":
            return .green
        case "dead":
            return .red
        default:
            return .yellow
        }
    }
}

struct SessionRowView: View {
    let session: TeleptySession

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            // Session ID + status indicator
            HStack(spacing: 6) {
                Circle()
                    .fill(statusColor)
                    .frame(width: 6, height: 6)

                Text(session.id)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(.white)
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
                        .foregroundColor(.cyan.opacity(0.8))
                }
                if let command = session.command {
                    Text(command)
                        .font(.system(size: 10))
                        .foregroundColor(.gray)
                }
            }

            // Current task
            if let task = session.currentTask {
                Text(task)
                    .font(.system(size: 10))
                    .foregroundColor(.white.opacity(0.7))
                    .lineLimit(2)
            }

            // Blocker
            if let blocker = session.blocker {
                HStack(spacing: 3) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 8))
                        .foregroundColor(.orange)
                    Text(blocker)
                        .font(.system(size: 9))
                        .foregroundColor(.orange.opacity(0.9))
                        .lineLimit(1)
                }
            }

            // Needs input indicator
            if session.needsInput == true {
                HStack(spacing: 3) {
                    Image(systemName: "keyboard")
                        .font(.system(size: 8))
                        .foregroundColor(.yellow)
                    Text("Needs input")
                        .font(.system(size: 9))
                        .foregroundColor(.yellow.opacity(0.9))
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.white.opacity(0.03))
        .cornerRadius(4)
        .padding(.horizontal, 4)
    }

    private var statusColor: Color {
        switch session.status {
        case "running", "active": return .green
        case "idle": return .yellow
        case "blocked", "error": return .orange
        case "dead": return .red
        default: return .gray
        }
    }

    private func phaseColor(_ phase: TaskPhase) -> Color {
        switch phase {
        case .implementing: return .blue
        case .blocked: return .orange
        case .testing: return .purple
        case .idle: return .gray
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
