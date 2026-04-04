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

    // Exponential backoff state (bug #134 — cmux-benchmarked params)
    private var failureCount: Int = 0
    private var didLogHighFailureWarning: Bool = false
    private static let baseBackoffMs: Double = 10.0     // 10ms base
    private static let maxBackoffMs: Double = 5000.0     // 5s cap
    private static let highFailureThreshold: Int = 50

    init(host: String = "127.0.0.1", port: Int = Int(ProcessInfo.processInfo.environment["ATERM_TELEPTY_PORT"] ?? "") ?? 3848) {
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

        if shouldLogFailure() {
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
                self?.didLogHighFailureWarning = false
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
                // Error classification: auth errors are fatal, everything else retries
                if let urlError = error as? URLError,
                   urlError.code == .userAuthenticationRequired ||
                   urlError.code == .userCancelledAuthentication {
                    NSLog("[telepty-bus] fatal auth error, stopping retries: %@", error.localizedDescription)
                    DispatchQueue.main.async {
                        self?.connected = false
                    }
                    return
                }

                // Retryable error — schedule reconnect with backoff
                if self?.shouldLogFailure() == true {
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

    /// Log dedup: first 3 failures log each, then only at powers-of-two (4, 8, 16, 32...)
    private func shouldLogFailure() -> Bool {
        if failureCount <= 3 { return true }
        return failureCount > 0 && (failureCount & (failureCount - 1)) == 0
    }

    private func scheduleReconnect() {
        reconnectWorkItem?.cancel()
        reconnectWorkItem = nil
        failureCount += 1

        // 50 consecutive failures — log warning once
        if failureCount == Self.highFailureThreshold && !didLogHighFailureWarning {
            NSLog("[telepty-bus] WARNING: %d consecutive failures, daemon may be down. Continuing with capped backoff.", failureCount)
            didLogHighFailureWarning = true
        }

        // Exponential backoff: 10ms → 20ms → 40ms → ... → 5000ms cap
        let intervalMs = min(Self.baseBackoffMs * pow(2.0, Double(failureCount - 1)), Self.maxBackoffMs)
        let interval = intervalMs / 1000.0

        if shouldLogFailure() {
            NSLog("[telepty-bus] reconnecting in %.0fms (attempt %d)", intervalMs, failureCount)
        }

        let workItem = DispatchWorkItem { [weak self] in
            self?.connect()
        }
        reconnectWorkItem = workItem
        DispatchQueue.main.asyncAfter(deadline: .now() + interval, execute: workItem)
    }
}
