import SwiftUI

struct SessionSidebarView: View {
    @ObservedObject var busClient: TeleptyBusClient

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
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
                    // Primary: My Workspaces (always visible)
                    sectionHeader("My Workspaces")

                    if busClient.workspaces.isEmpty {
                        Text("No workspaces")
                            .font(.system(size: 11))
                            .foregroundColor(.gray)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                    } else {
                        ForEach(busClient.workspaces) { ws in
                            WorkspaceRowView(workspace: ws)
                        }
                    }

                    // Secondary: External Sessions (telepty)
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
        .onAppear {
            busClient.refreshWorkspaces()
        }
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

struct WorkspaceRowView: View {
    let workspace: AtermWorkspace

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Circle()
                    .fill(workspace.status == "running" ? Color.green : Color.red)
                    .frame(width: 6, height: 6)
                Text(workspace.id)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(.white)
                    .lineLimit(1)
                Spacer()
                Text(workspace.status)
                    .font(.system(size: 9))
                    .foregroundColor(.gray)
            }
            HStack(spacing: 4) {
                Text(workspace.command.components(separatedBy: "/").last ?? workspace.command)
                    .font(.system(size: 10))
                    .foregroundColor(.cyan.opacity(0.8))
                if !workspace.args.isEmpty {
                    Text(workspace.args.joined(separator: " "))
                        .font(.system(size: 10))
                        .foregroundColor(.gray)
                        .lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.white.opacity(0.03))
        .cornerRadius(4)
        .padding(.horizontal, 4)
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
