import Foundation

/// Event types from telepty bus
enum BusEventType: String, Codable {
    case sessionHealth = "session_health"
    case sessionStateReport = "session_state_report"
    case deliberationHealth = "deliberation_health"
    case injectWritten = "inject_written"
    case sessionDisconnect = "session_disconnect"
    case sessionReconnect = "session_reconnect"
    case sessionStale = "session_stale"
}

/// Session health status
enum SessionHealthStatus: String, Codable {
    case active
    case idle
    case blocked
    case error
    case dead
    case unknown
}

/// Task phase for session state reports
enum TaskPhase: String, Codable {
    case implementing
    case blocked
    case testing
    case idle
}

/// Session info from telepty list
struct TeleptySession: Identifiable, Codable {
    let id: String
    let project: String?
    let command: String?
    let cwd: String?
    let status: String?
    let host: String?
    /// Terminal program that registered this session (e.g. "aterm", "ghostty")
    let termProgram: String?

    // Semantic fields (from session_state_report)
    var phase: TaskPhase?
    var currentTask: String?
    var blocker: String?
    var needsInput: Bool?

    enum CodingKeys: String, CodingKey {
        case id, project, command, cwd, status, host, termProgram
        case phase, currentTask = "current_task", blocker
        case needsInput = "needs_input"
    }
}

/// Bus event envelope (normalized observer schema from telepty)
struct BusEvent: Codable {
    let eventType: String
    let sessionId: String?
    let timestamp: String?

    // Transport block
    let transport: TransportBlock?
    // Semantic block
    let semantic: SemanticBlock?

    enum CodingKeys: String, CodingKey {
        case eventType = "event_type"
        case sessionId = "session_id"
        case timestamp
        case transport, semantic
    }
}

struct TransportBlock: Codable {
    let healthStatus: String?
    let healthReason: String?

    enum CodingKeys: String, CodingKey {
        case healthStatus = "health_status"
        case healthReason = "health_reason"
    }
}

struct SemanticBlock: Codable {
    let phase: String?
    let currentTask: String?
    let blocker: String?
    let needsInput: Bool?

    enum CodingKeys: String, CodingKey {
        case phase
        case currentTask = "current_task"
        case blocker
        case needsInput = "needs_input"
    }
}

/// Internal workspace from aterm-core PtyManager
struct AtermWorkspace: Identifiable, Codable {
    let id: String
    let cwd: String
    let command: String
    let args: [String]
    let status: String
    let createdAt: String
    let bufferLines: Int
}

/// WebSocket client that connects to telepty bus
class TeleptyBusClient: ObservableObject {
    @Published var sessions: [TeleptySession] = []
    @Published var workspaces: [AtermWorkspace] = []
    @Published var connected: Bool = false

    private var webSocketTask: URLSessionWebSocketTask?
    private let busURL: URL
    private var reconnectWorkItem: DispatchWorkItem?
    private var corePtr: OpaquePointer?  // AtermCore*
    private let webSocketSession: URLSession

    // Exponential backoff state
    private var failureCount: Int = 0
    private var silentMode: Bool = false
    private var lastSilentLog: Date = .distantPast
    private static let maxBackoffInterval: TimeInterval = 60.0
    private static let silentThreshold: Int = 5
    private static let silentLogInterval: TimeInterval = 60.0

    init(host: String = "127.0.0.1", port: Int = 3848) {
        self.busURL = URL(string: "ws://\(host):\(port)/api/bus")!
        self.webSocketSession = URLSession(configuration: .default)
        loadInitialSessions()
        connect()
    }

    /// Set the aterm-core pointer for workspace polling
    func setCore(_ core: OpaquePointer?) {
        self.corePtr = core
    }

    /// Poll internal workspaces from aterm-core FFI
    func refreshWorkspaces() {
        guard let core = corePtr else { return }
        guard let jsonPtr = aterm_core_list_workspaces(core) else { return }
        let json = String(cString: jsonPtr)
        aterm_core_free_string(jsonPtr)

        guard let data = json.data(using: .utf8),
              let list = try? JSONDecoder().decode([AtermWorkspace].self, from: data) else { return }
        workspaces = list
    }

    deinit {
        disconnect()
    }

    // MARK: - Initial snapshot via telepty list --json

    private func loadInitialSessions() {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let proc = Process()
            proc.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            proc.arguments = ["telepty", "list", "--json"]
            let pipe = Pipe()
            proc.standardOutput = pipe
            proc.standardError = FileHandle.nullDevice

            do {
                try proc.run()
                proc.waitUntilExit()
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                if let sessions = try? JSONDecoder().decode([TeleptySession].self, from: data) {
                    DispatchQueue.main.async {
                        self?.sessions = sessions
                    }
                }
            } catch {
                NSLog("[telepty-bus] list failed: %@", error.localizedDescription)
            }
        }
    }

    // MARK: - WebSocket connection

    func connect() {
        reconnectWorkItem?.cancel()
        reconnectWorkItem = nil

        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = webSocketSession.webSocketTask(with: busURL)
        webSocketTask?.resume()
        // connected = true only on first successful receive (not here)

        if !silentMode {
            NSLog("[telepty-bus] connecting to %@", busURL.absoluteString)
        }

        receiveMessage()
    }

    func disconnect() {
        reconnectWorkItem?.cancel()
        reconnectWorkItem = nil
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        connected = false
    }

    private func receiveMessage() {
        webSocketTask?.receive { [weak self] result in
            switch result {
            case .success(let message):
                // Connection confirmed working — reset backoff
                self?.failureCount = 0
                self?.silentMode = false
                DispatchQueue.main.async {
                    self?.connected = true
                }

                switch message {
                case .string(let text):
                    self?.handleMessage(text)
                case .data(let data):
                    if let text = String(data: data, encoding: .utf8) {
                        self?.handleMessage(text)
                    }
                @unknown default:
                    break
                }
                // Continue receiving
                self?.receiveMessage()

            case .failure(let error):
                if !(self?.silentMode ?? false) {
                    NSLog("[telepty-bus] WebSocket error: %@", error.localizedDescription)
                }
                DispatchQueue.main.async {
                    self?.connected = false
                    self?.scheduleReconnect()
                }
            }
        }
    }

    private func handleMessage(_ text: String) {
        guard let data = text.data(using: .utf8),
              let event = try? JSONDecoder().decode(BusEvent.self, from: data) else {
            return
        }

        DispatchQueue.main.async { [weak self] in
            self?.applyEvent(event)
        }
    }

    private func applyEvent(_ event: BusEvent) {
        guard let sessionId = event.sessionId else { return }

        if let idx = sessions.firstIndex(where: { $0.id == sessionId }) {
            // Update existing session
            if let semantic = event.semantic {
                if let phase = semantic.phase {
                    sessions[idx].phase = TaskPhase(rawValue: phase)
                }
                sessions[idx].currentTask = semantic.currentTask ?? sessions[idx].currentTask
                sessions[idx].blocker = semantic.blocker
                sessions[idx].needsInput = semantic.needsInput ?? sessions[idx].needsInput
            }
        } else {
            // New session — add it
            let newSession = TeleptySession(
                id: sessionId,
                project: nil,
                command: nil,
                cwd: nil,
                status: event.transport?.healthStatus ?? "active",
                host: nil,
                termProgram: nil,
                phase: event.semantic?.phase.flatMap(TaskPhase.init),
                currentTask: event.semantic?.currentTask,
                blocker: event.semantic?.blocker,
                needsInput: event.semantic?.needsInput
            )
            sessions.append(newSession)
        }

        // Handle disconnect — remove session
        if event.eventType == "session_disconnect" {
            sessions.removeAll { $0.id == sessionId }
        }
    }

    private func scheduleReconnect() {
        reconnectWorkItem?.cancel()
        reconnectWorkItem = nil
        failureCount += 1

        if failureCount >= Self.silentThreshold {
            silentMode = true
        }

        // Exponential backoff: 3s → 6s → 12s → 24s → 48s → 60s cap
        let interval = min(3.0 * pow(2.0, Double(failureCount - 1)), Self.maxBackoffInterval)

        if silentMode {
            let now = Date()
            if now.timeIntervalSince(lastSilentLog) >= Self.silentLogInterval {
                NSLog("[telepty-bus] reconnect attempts: %d (silent mode, next in %.0fs)", failureCount, interval)
                lastSilentLog = now
            }
        } else {
            NSLog("[telepty-bus] reconnecting in %.0fs (attempt %d)", interval, failureCount)
        }

        let workItem = DispatchWorkItem { [weak self] in
            self?.connect()
        }
        reconnectWorkItem = workItem
        DispatchQueue.main.asyncAfter(deadline: .now() + interval, execute: workItem)
    }
}
