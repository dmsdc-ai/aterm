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

/// Invisible 6pt-wide drag handle between sidebar and terminal.
/// Adapted from ghostty SplitView.Divider (0pt visible + 6pt hitbox).
private class SidebarDragHandleView: NSView {
  var onDrag: ((CGFloat) -> Void)?

  override func resetCursorRects() {
    addCursorRect(bounds, cursor: .resizeLeftRight)
  }

  override func mouseDragged(with event: NSEvent) {
    guard let sv = superview else { return }
    let x = sv.convert(event.locationInWindow, from: nil).x
    onDrag?(x)
  }

  override var acceptsFirstResponder: Bool { true }
  override var focusRingType: NSFocusRingType { get { .none } set {} }
}

class AppDelegate: NSObject, NSApplicationDelegate {
  var window: NSWindow!
  var terminalView: TerminalView?
  var busClient: TeleptyBusClient!
  var containerView: NSView!
  private var sidebarWidthConstraint: NSLayoutConstraint!
  var terminalContainerView: NSView!
  private var orchestratorInputBar: OrchestratorInputBar!
  private var terminalContainerBottomToWindow: NSLayoutConstraint!
  private var terminalContainerBottomToInputBar: NSLayoutConstraint!

  private let workspaceSidebarModel = WorkspaceSidebarModel()
  private var managedWorkspaces: [UUID: ManagedWorkspace] = [:]
  private var workspaceOrder: [UUID] = []
  /// Pending bootstraps waiting for ShellReady event: workspace UUID → (command, fallback timer)
  private var pendingBootstraps: [UUID: (command: String, fallbackTimer: DispatchWorkItem)] = [:]
  private var excludedChildPIDs: Set<Int32> = []
  /// Attach failure counter per session ID — stops retry after 3 failures (#205).
  private var attachFailures: [String: Int] = [:]
  /// Dedup guard: prevents redundant hide-all + show-selected + rebuildSidebar cycle
  private var currentSelectedWorkspaceId: UUID?

  func applicationDidFinishLaunching(_ notification: Notification) {
    // Single instance enforcement — skip in sandbox mode (ATERM_DATA_ROOT set)
    // so sandbox and production aterm can run simultaneously
    if ProcessInfo.processInfo.environment["ATERM_DATA_ROOT"] == nil {
      let bundleID = Bundle.main.bundleIdentifier ?? "com.aigentry.aterm"
      let running = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
      let others = running.filter { $0 != NSRunningApplication.current }
      if let existing = others.first {
        NSLog("[aterm] already running (PID %d), activating existing instance", existing.processIdentifier)
        existing.activate()
        exit(0)
      }
    } else {
      NSLog("[aterm] sandbox mode (ATERM_DATA_ROOT set), skipping single-instance check")
    }

    NSApp.setActivationPolicy(.regular)

    // Force dark chrome for all aterm UI (sidebar, Settings, window frame, input bar)
    // regardless of macOS system appearance. Terminal content colorScheme is separate
    // and already configurable via Settings. Follow-up: add Appearance picker in Settings.
    NSApp.appearance = NSAppearance(named: .darkAqua)

    ensureTeleptyDaemon()
    startTailscale()

    let rect = NSRect(x: 0, y: 0, width: 1280, height: 768)
    window = NSWindow(
      contentRect: rect,
      styleMask: [.titled, .closable, .resizable, .miniaturizable],
      backing: .buffered,
      defer: false
    )
    window.isReleasedWhenClosed = false
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
      onSelectWorkspace: { [weak self] workspaceName in
        self?.selectWorkspace(named: workspaceName)
      },
      onRenameWorkspace: { [weak self] workspaceName, name in
        self?.renameWorkspace(named: workspaceName, toSuggested: name)
      },
      onCloseWorkspace: { [weak self] workspaceName in
        self?.closeWorkspace(named: workspaceName)
      },
      onBatchCloseWorkspaces: { [weak self] workspaceNames in
        guard let self else { return }
        let uuids = workspaceNames.compactMap { self.workspaceID(named: $0) }
        self.removeWorkspaces(ids: uuids)
      },
      onChangeWorkspaceCLI: { [weak self] workspaceName, newCli in
        self?.changeWorkspaceCLI(named: workspaceName, to: newCli)
      },
      onRestartWorkspace: { [weak self] workspaceName in
        self?.restartWorkspace(named: workspaceName)
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
    sidebarHost.translatesAutoresizingMaskIntoConstraints = false

    DispatchQueue.main.async { [weak sidebarHost] in
      Self.suppressScrollViewFocusRings(in: sidebarHost)
    }

    // Create terminal container
    let terminalRect = NSRect(x: 0, y: 0, width: rect.width - 240, height: rect.height)
    terminalContainerView = NSView(frame: terminalRect)
    terminalContainerView.translatesAutoresizingMaskIntoConstraints = false
    terminalContainerView.wantsLayer = true
    terminalContainerView.layer?.backgroundColor = NSColor.atermHex(0x1A1B26).cgColor
    terminalContainerView.layer?.isOpaque = true

    // -- Container (replaces NSSplitView) --
    // Pattern: ghostty TerminalViewContainer.swift:55-63
    containerView = NSView(frame: rect)
    containerView.autoresizingMask = [.width, .height]
    containerView.wantsLayer = true

    containerView.addSubview(sidebarHost)
    containerView.addSubview(terminalContainerView)

    // Sidebar: pinned left/top/bottom, width = 240 (min 180, max 400)
    sidebarWidthConstraint = sidebarHost.widthAnchor.constraint(equalToConstant: 240)
    sidebarWidthConstraint.priority = .defaultHigh
    NSLayoutConstraint.activate([
      sidebarHost.leadingAnchor.constraint(equalTo: containerView.leadingAnchor),
      sidebarHost.topAnchor.constraint(equalTo: containerView.topAnchor),
      sidebarHost.bottomAnchor.constraint(equalTo: containerView.bottomAnchor),
      sidebarWidthConstraint,
      sidebarHost.widthAnchor.constraint(greaterThanOrEqualToConstant: 180),
      sidebarHost.widthAnchor.constraint(lessThanOrEqualToConstant: 400),
    ])

    // Orchestrator input bar (hidden by default, shown for isSystem workspaces)
    orchestratorInputBar = OrchestratorInputBar()
    orchestratorInputBar.translatesAutoresizingMaskIntoConstraints = false
    orchestratorInputBar.isHidden = true
    containerView.addSubview(orchestratorInputBar)

    orchestratorInputBar.onSubmit = { [weak self] text in
      guard let self, let core = self.terminalView?.corePointer else { return }
      let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmed.isEmpty else { return }
      // Write text first, then CR after a short delay.
      // TUI CLIs (Codex, Gemini) use multi-line editors where text + \r in a single
      // write causes the \r to be processed as a newline inside the editor.
      // Splitting the write gives the TUI time to process the text, then the
      // subsequent \r is interpreted as "submit" rather than "newline in editor".
      trimmed.withCString { ptr in
        aterm_core_write_pty(core, ptr, trimmed.utf8.count)
      }
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
        guard let core = self.terminalView?.corePointer else { return }
        "\r".withCString { ptr in
          aterm_core_write_pty(core, ptr, 1)
        }
      }
    }
    orchestratorInputBar.onEscape = { [weak self] in
      guard let self, let tv = self.terminalView else { return }
      self.window.makeFirstResponder(tv)
    }
    orchestratorInputBar.onCtrlKey = { [weak self] byte in
      guard let self, let core = self.terminalView?.corePointer else { return }
      var b = byte
      withUnsafePointer(to: &b) { ptr in
        ptr.withMemoryRebound(to: CChar.self, capacity: 1) { cptr in
          aterm_core_write_pty(core, cptr, 1)
        }
      }
    }

    // Input bar manages its own height (58–200pt) — no fixed height constraint here
    NSLayoutConstraint.activate([
      orchestratorInputBar.leadingAnchor.constraint(equalTo: sidebarHost.trailingAnchor),
      orchestratorInputBar.trailingAnchor.constraint(equalTo: containerView.trailingAnchor),
      orchestratorInputBar.bottomAnchor.constraint(equalTo: containerView.bottomAnchor),
    ])

    // Terminal: flush right of sidebar, fills remaining space
    terminalContainerBottomToWindow = terminalContainerView.bottomAnchor.constraint(
      equalTo: containerView.bottomAnchor)
    terminalContainerBottomToInputBar = terminalContainerView.bottomAnchor.constraint(
      equalTo: orchestratorInputBar.topAnchor)
    terminalContainerBottomToInputBar.isActive = false

    NSLayoutConstraint.activate([
      terminalContainerView.leadingAnchor.constraint(equalTo: sidebarHost.trailingAnchor),
      terminalContainerView.topAnchor.constraint(equalTo: containerView.topAnchor),
      terminalContainerBottomToWindow,
      terminalContainerView.trailingAnchor.constraint(equalTo: containerView.trailingAnchor),
    ])

    // Invisible drag handle (6pt wide, between sidebar and terminal)
    // Pattern: ghostty SplitView.Divider (splitterInvisibleSize = 6)
    let dragHandle = SidebarDragHandleView()
    dragHandle.translatesAutoresizingMaskIntoConstraints = false
    containerView.addSubview(dragHandle)
    NSLayoutConstraint.activate([
      dragHandle.centerXAnchor.constraint(equalTo: sidebarHost.trailingAnchor),
      dragHandle.topAnchor.constraint(equalTo: containerView.topAnchor),
      dragHandle.bottomAnchor.constraint(equalTo: containerView.bottomAnchor),
      dragHandle.widthAnchor.constraint(equalToConstant: 6),
    ])
    dragHandle.onDrag = { [weak self] x in
      guard let self else { return }
      let clamped = min(max(x, 180), 400)
      self.sidebarWidthConstraint.constant = clamped
    }

    window.contentView = containerView
    window.makeKeyAndOrderFront(nil)

    // Fix #217: Remove default 1px gray titlebar separator
    if #available(macOS 12.0, *) {
        window.titlebarSeparatorStyle = .none
        window.titlebarAppearsTransparent = true
    }

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
    // Force sidebar re-fetch now that host callbacks are registered.
    // restoreWorkspaces() ran before registerHostCallbacks(), so the initial
    // IPC ListWorkspaces hit the fallback path (host=None) where is_system=None.
    rebuildSidebarState()

    // Re-register all workspaces with telepty after restore to ensure
    // none are missing (fire-and-forget registration can silently fail).
    // Runs off main thread to avoid blocking activation on network timeouts.
    if restoredCount > 0 {
      DispatchQueue.global(qos: .userInitiated).async {
        aterm_sync_telepty()
      }
    }
    refreshWorkspaceProcesses()

    // Background pre-spawn: stagger PTY init for non-selected workspaces
    // to eliminate delay on first session switch (#177)
    if restoredCount > 1 {
      var staggerIndex = 0
      for wsID in workspaceOrder {
        guard let ws = managedWorkspaces[wsID],
              !ws.terminalView.didSpawnShell else { continue }
        staggerIndex += 1
        DispatchQueue.main.asyncAfter(deadline: .now() + Double(staggerIndex) * 0.3) { [weak self] in
          guard let self, let ws = self.managedWorkspaces[wsID] else { return }
          guard !ws.terminalView.didSpawnShell else { return }
          ws.terminalView.preSpawnInBackground()
          if !ws.terminalView.didSpawnShell {
            NSLog("[aterm] background pre-spawn failed for '%@'", ws.name)
          }
        }
      }
      if staggerIndex > 0 {
        NSLog("[aterm] background pre-spawn: %d workspaces queued (300ms stagger)", staggerIndex)
      }
    }
  }

  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return true
  }

  func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    if !flag {
      window?.makeKeyAndOrderFront(nil)
    }
    return true
  }

  func applicationWillResignActive(_ notification: Notification) {
    NSLog("[DIAG-LIFECYCLE] RESIGN ACTIVE: window isVisible=%d isKey=%d activationPolicy=%ld",
          window?.isVisible ?? false ? 1 : 0, window?.isKeyWindow ?? false ? 1 : 0,
          NSApp.activationPolicy().rawValue)
  }

  func applicationDidResignActive(_ notification: Notification) {
    NSLog("[DIAG-LIFECYCLE] DID RESIGN: activationPolicy=%ld", NSApp.activationPolicy().rawValue)
  }

  func applicationDidBecomeActive(_ notification: Notification) {
    window?.makeKeyAndOrderFront(nil)
  }

  func applicationWillTerminate(_ notification: Notification) {
    saveWorkspaces()
    // Deregister all workspaces from telepty daemon to prevent ghost sessions
    deregisterTeleptyWorkspaces()
    // Invalidate per-workspace idle timers
    for (_, ws) in managedWorkspaces {
      ws.idleTimer?.invalidate()
      ws.idleTimer = nil
    }
    let env = ProcessInfo.processInfo.environment
    let tailscaleDisabledEnv =
      env["ATERM_TAILSCALE_ENABLED"].map {
        $0 == "0" || $0.lowercased() == "false" || $0.lowercased() == "no"
      } ?? false
    let tailscaleDisabledDefaults =
      UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil
      && !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled")
    if !tailscaleDisabledEnv && !tailscaleDisabledDefaults {
      aterm_tailscale_shutdown()
    }
  }

  private func deregisterTeleptyWorkspaces() {
    let port = Int(ProcessInfo.processInfo.environment["ATERM_TELEPTY_PORT"] ?? "") ?? 3848
    for (_, workspace) in managedWorkspaces {
      let name = workspace.name
      guard !name.isEmpty,
        let url = URL(string: "http://localhost:\(port)/api/sessions/\(name)")
      else { continue }
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

  /// Deregister a single workspace from telepty. Fire-and-forget.
  private func deregisterTeleptyWorkspace(name: String) {
    let port = Int(ProcessInfo.processInfo.environment["ATERM_TELEPTY_PORT"] ?? "") ?? 3848
    guard !name.isEmpty,
      let url = URL(string: "http://localhost:\(port)/api/sessions/\(name)")
    else { return }
    var req = URLRequest(url: url)
    req.httpMethod = "DELETE"
    req.timeoutInterval = 2
    URLSession.shared.dataTask(with: req).resume()
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
      let list = delegate.workspaceOrder.compactMap { id -> [String: Any]? in
        guard let ws = delegate.managedWorkspaces[id] else { return nil }
        // #205: Show dead workspaces in sidebar so users can delete them.
        // Previously filtered out dead/closing — caused invisible ghost workspaces.
        var payload: [String: Any] = [
          "id": ws.name,
          "name": ws.name,
          "cli": ws.launchCommand.rawValue,
          "cwd": ws.cwd,
          "status": ws.status,
          "created_at": workspaceInfoTimestampFormatter.string(from: ws.createdAt),
          "last_activity_at": workspaceInfoTimestampFormatter.string(from: ws.lastActivityAt),
          "is_system": ws.isSystem,
        ]
        if !ws.customCommand.isEmpty {
          payload["custom_command"] = ws.customCommand
        }
        return payload
      }
      guard let data = try? JSONSerialization.data(withJSONObject: list),
        let str = String(data: data, encoding: .utf8)
      else { return nil }
      return strdup(str)
    }
    callbacks.on_events_available = { userdata in
      guard let userdata else { return }
      let delegate = Unmanaged<AppDelegate>.fromOpaque(userdata).takeUnretainedValue()
      DispatchQueue.main.async {
        delegate.drainEvents()
      }
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
      let config = try? JSONDecoder().decode(IpcWorkspaceConfig.self, from: data)
    else {
      NSLog("[aterm-ipc] failed to parse workspace config: %@", configJson)
      return
    }
    let command = WorkspaceLaunchCommand(rawValue: config.cli) ?? .zsh
    let cwd = config.cwd.isEmpty ? NSHomeDirectory() : config.cwd
    // #193: IPC-created workspaces are ephemeral — not persisted to sessions.json.
    // They exist for the current app session only and won't be auto-restored on next launch.
    createWorkspace(
      name: config.name, command: command, customCommand: "", cwd: cwd, shouldSelect: true,
      skipSave: true)
    // Mark as ephemeral so future saveWorkspaces() calls also skip it
    if let wsID = workspaceID(named: config.name) {
      managedWorkspaces[wsID]?.isEphemeral = true
    }
  }

  /// Wakeup+drain: called on main thread when Rust signals events are available.
  /// Drains all pending events as C structs — no JSON decode overhead.
  private func drainEvents() {
    let batch = aterm_drain_events()
    guard batch.count > 0, let events = batch.events else {
      aterm_free_events(batch)
      return
    }
    for i in 0..<Int(batch.count) {
      let event = events[i]
      let id = event.id.map { String(cString: $0) } ?? ""
      switch Int(event.event_type) {
      case Int(ATERM_EVENT_CLOSED):
        // #194: Don't remove immediately — mark dead and let cleanupStaleWorkspaces handle it.
        // Prevents cascade deletion when multiple workspaces die on startup.
        if let wsID = workspaceID(named: id) {
          managedWorkspaces[wsID]?.status = "dead"
          managedWorkspaces[wsID]?.lastActivityAt = Date()
          rebuildSidebarState()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 5.0) { [weak self] in
          self?.cleanupStaleWorkspaces()
        }
      case Int(ATERM_EVENT_STATUS_CHANGED):
        let status = event.status.map { String(cString: $0) } ?? ""
        if status == "dead" || status == "closing" {
          // #194: Don't remove immediately — update status and defer cleanup.
          if let wsID = workspaceID(named: id) {
            managedWorkspaces[wsID]?.status = status
            managedWorkspaces[wsID]?.lastActivityAt = Date()
            rebuildSidebarState()
          }
          DispatchQueue.main.asyncAfter(deadline: .now() + 5.0) { [weak self] in
            self?.cleanupStaleWorkspaces()
          }
        }
      case Int(ATERM_EVENT_SHELL_READY):
        if let wsID = workspaceID(named: id),
          let pending = pendingBootstraps.removeValue(forKey: wsID)
        {
          pending.fallbackTimer.cancel()
          NSLog("[aterm] shell_ready received for '%@' — bootstrapping immediately", id)
          sendBootstrapCommand(workspaceID: wsID, command: pending.command)
        }
      case Int(ATERM_EVENT_TRUST_PROMPT):
        if let wsID = workspaceID(named: id),
          let workspace = managedWorkspaces[wsID],
          let core = workspace.terminalView.corePointer
        {
          "\r".withCString { ptr in
            aterm_core_write_pty(core, ptr, 1)
          }
          NSLog("[aterm] auto-accepted trust prompt for '%@' (event-driven)", workspace.name)
        }
      case Int(ATERM_EVENT_BATCH_CLOSED):
        // Rust completed batch close — single sidebar rebuild
        NSLog("[aterm] batch close event received")
        rebuildSidebarState()
      case Int(ATERM_EVENT_CREATION_FAILED):
        // Rollback workspace that failed to create on Rust side
        if let wsID = workspaceID(named: id) {
          NSLog("[aterm] creation failed event for '%@' — rolling back", id)
          managedWorkspaces[wsID]?.terminalView.removeFromSuperview()
          managedWorkspaces.removeValue(forKey: wsID)
          workspaceOrder.removeAll { $0 == wsID }
          rebuildSidebarState()
        }
      case Int(ATERM_EVENT_RESTORED):
        // Crash recovery restore — rebuild sidebar to show restored workspaces
        NSLog("[aterm] workspace restored event for '%@'", id)
        rebuildSidebarState()
      default:
        break
      }
    }
    aterm_free_events(batch)
  }

  // MARK: - Workspace Persistence

  private func saveWorkspaces() {
    // Build ordered entries from workspaceOrder, then append any orphaned
    // workspaces in managedWorkspaces that aren't tracked in workspaceOrder.
    // This ensures dynamically created workspaces are always persisted (#177).
    var seenIDs = Set<UUID>()
    var entries: [[String: Any]] = []

    func entryDict(for ws: ManagedWorkspace) -> [String: Any]? {
      let effectiveName =
        ws.name.isEmpty
        ? URL(fileURLWithPath: ws.cwd).lastPathComponent
        : ws.name
      guard !effectiveName.isEmpty else { return nil }
      return [
        "id": effectiveName,
        "cwd": ws.cwd,
        "command": ws.launchCommand.rawValue,
        "args": [] as [String],
        "customCommand": ws.customCommand,
        "cliArgs": ws.cliArgs,
        "isSystem": ws.isSystem,
        "resumeCommand": ws.launchCommand.bootstrapCommand(customCommand: ws.customCommand, cliArgs: ws.cliArgs) ?? "",
      ] as [String: Any]
    }

    for id in workspaceOrder {
      guard let ws = managedWorkspaces[id], !ws.isEphemeral, let entry = entryDict(for: ws) else { continue }
      seenIDs.insert(id)
      entries.append(entry)
    }

    // Capture orphaned workspaces not in workspaceOrder (defensive — #177)
    for (id, ws) in managedWorkspaces where !seenIDs.contains(id) {
      guard !ws.isEphemeral, let entry = entryDict(for: ws) else { continue }
      NSLog("[aterm] saveWorkspaces: orphaned workspace '%@' not in workspaceOrder — including", ws.name)
      entries.append(entry)
    }
    let wrapper: [String: Any] = ["sessions": entries]
    do {
      let sessionsURL = URL(fileURLWithPath: AtermSettings.dataRoot + "/data/sessions.json")
      let dir = sessionsURL.deletingLastPathComponent()
      try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
      let data = try JSONSerialization.data(
        withJSONObject: wrapper, options: [.prettyPrinted, .sortedKeys])
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
    return fm.fileExists(atPath: AtermSettings.dataRoot + "/data/sessions.json")
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

    // Collect all entries FIRST before creating any workspaces.
    // createWorkspace() calls saveWorkspaces() which overwrites sessions.json,
    // and aterm_session_get() re-reads from disk each call — so the file would
    // be truncated to 1 entry after the first workspace is created.
    struct RestoredEntry {
      let name: String
      let cwd: String
      let command: String
      let customCommand: String
      let isSystem: Bool
    }
    var entries: [RestoredEntry] = []
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

      let effectiveName =
        name.isEmpty
        ? URL(fileURLWithPath: cwd).lastPathComponent
        : name
      if effectiveName.isEmpty { continue }

      // Failsafe: force isSystem=true if this entry matches the configured orchestrator name.
      // Prevents self-reinforcing corruption where sessions.json has isSystem:false for orchestrator.
      let orchName = AtermSettings.shared.orchestratorName
      let resolvedIsSystem = entry.is_system
        || effectiveName == orchName
        || effectiveName == "orchestrator"

      entries.append(
        RestoredEntry(
          name: effectiveName,
          cwd: cwd,
          command: commandStr,
          customCommand: custom,
          isSystem: resolvedIsSystem
        ))
    }

    // Determine which entry to auto-select at launch (P0 fix):
    //   1. First isSystem entry (orchestrator) — takes priority per direct user
    //      quote "default가 orchestrator여야 함". Orchestrator is expected to be
    //      first in workspaceOrder (per insert(at: 0) at createWorkspace:1502)
    //      but may be any index in the restore entries array.
    //   2. Fallback: last entry (preserves prior behavior when no orchestrator
    //      exists, e.g. first-install or user removed orchestrator).
    let shouldSelectIndex = entries.firstIndex(where: { $0.isSystem })
      ?? max(0, entries.count - 1)

    // Now create workspaces from collected data (skipSave until the end)
    for (i, se) in entries.enumerated() {
      let effectiveCwd: String
      if FileManager.default.fileExists(atPath: se.cwd) {
        effectiveCwd = se.cwd
      } else if se.isSystem {
        let orchestratorDir = AtermSettings.shared.effectiveOrchestratorCWD
        try? FileManager.default.createDirectory(
          atPath: orchestratorDir, withIntermediateDirectories: true)
        effectiveCwd = orchestratorDir
      } else {
        effectiveCwd = NSHomeDirectory()
      }
      let requestedCommand = WorkspaceLaunchCommand(rawValue: se.command) ?? .zsh
      let command = cliAvailable(for: requestedCommand) ? requestedCommand : .zsh
      let restoredCliArgs = AtermSettings.shared.cliDefaults[command.rawValue] ?? ""

      createWorkspace(
        name: se.name,
        command: command,
        customCommand: se.customCommand,
        cliArgs: restoredCliArgs,
        cwd: effectiveCwd,
        shouldSelect: i == shouldSelectIndex,
        isSystem: se.isSystem,
        skipSave: true
      )
    }

    if !entries.isEmpty {
      saveWorkspaces()
      NSLog("[aterm] restored %d workspaces", entries.count)
    }
    return entries.count
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
    // Orchestrator uses its own CLI setting, not the default CLI
    let orchCli = result.orchestratorCLI
    let command: WorkspaceLaunchCommand
    switch orchCli {
    case "claude": command = .claude
    case "codex": command = .codex
    case "gemini": command = .gemini
    default: command = .zsh
    }

    let userSelectedOrchestrator = orchCli != "none"
    let name = userSelectedOrchestrator ? "orchestrator" : "main"
    let cwd: String
    if userSelectedOrchestrator {
      let orchestratorDir = result.orchestratorCWD.isEmpty
        ? AtermSettings.shared.aigentryRoot + "/orchestrator"
        : result.orchestratorCWD
      try? FileManager.default.createDirectory(
        atPath: orchestratorDir, withIntermediateDirectories: true)
      cwd = orchestratorDir
    } else {
      cwd = NSHomeDirectory()
    }

    // Pre-create Claude Code trust directory so the trust prompt is skipped
    if command == .claude {
      ensureClaudeProjectTrust(cwd: cwd)
    }

    NSLog(
      "[aterm] creating workspace from onboarding: name=%@, cli=%@, command=%@, cwd=%@", name,
      orchCli, command.rawValue, cwd)
    createWorkspace(
      name: name,
      command: command,
      customCommand: "",
      cwd: cwd,
      shouldSelect: true,
      isSystem: userSelectedOrchestrator
    )
  }

  private func createDefaultWorkspace() {
    // Read orchestrator config from aterm.json
    let configPath = AtermSettings.dataRoot + "/config/aterm.json"
    var parsedConfig: [String: Any]?
    if let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
      let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    {
      parsedConfig = config
    }

    // Read orchestrator-specific config, fall back to ai.defaultCLI for migration
    let orchestratorConfig = parsedConfig?["orchestrator"] as? [String: Any]
    let ai = parsedConfig?["ai"] as? [String: Any]
    let orchCli = orchestratorConfig?["cli"] as? String
      ?? ai?["defaultCLI"] as? String
      ?? "none"

    // Map CLI to command
    let requestedCommand: WorkspaceLaunchCommand
    switch orchCli {
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
      NSLog("[aterm] %@ not found, starting with zsh", orchCli)
      command = .zsh
    } else {
      command = .zsh
    }

    let orchestratorDir = orchestratorConfig?["cwd"] as? String
      ?? (AtermSettings.shared.aigentryRoot + "/orchestrator")
    try? FileManager.default.createDirectory(
      atPath: orchestratorDir, withIntermediateDirectories: true)
    let cwd = orchestratorDir

    // Pre-create Claude Code trust directory for orchestrator CWD
    if command == .claude {
      ensureClaudeProjectTrust(cwd: cwd)
    }

    // Name is 'orchestrator' if user selected a CLI (even if binary not found)
    let userSelectedOrchestrator = orchCli != "none"
    let name = userSelectedOrchestrator ? "orchestrator" : "main"

    createWorkspace(
      name: name,
      command: command,
      customCommand: "",
      cwd: cwd,
      shouldSelect: true,
      isSystem: userSelectedOrchestrator
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
      let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Bool]
    else {
      return CliStatus(claude: false, codex: false, gemini: false)
    }
    return CliStatus(
      claude: dict["claude"] ?? false,
      codex: dict["codex"] ?? false,
      gemini: dict["gemini"] ?? false
    )
  }

  private func needsOnboarding() -> Bool {
    let config = readConfig()
    // Flag not set or false → needs onboarding
    if config["onboarding_completed"] as? Bool != true { return true }
    // Sessions file missing or empty → re-onboard so orchestrator gets created
    if !hasPersistedSessions() { return true }
    if Int(aterm_session_count(nil)) == 0 { return true }
    return false
  }

  private func readConfig() -> [String: Any] {
    let configPath = AtermSettings.dataRoot + "/config/aterm.json"
    guard let data = try? Data(contentsOf: URL(fileURLWithPath: configPath)),
      let config = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
      return [:]
    }
    return config
  }

  private func saveOnboardingResult(_ result: OnboardingResult) {
    let configPath = AtermSettings.dataRoot + "/config/aterm.json"
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
    var orchestrator = config["orchestrator"] as? [String: Any] ?? [:]
    orchestrator["cli"] = result.orchestratorCLI
    // GAP 4: Save orchestrator name so Rust read_orchestrator_session_name() can find it
    orchestrator["name"] = result.orchestratorCLI != "none" ? "orchestrator" : "main"
    let orchestratorDir = result.orchestratorCWD.isEmpty
      ? AtermSettings.shared.aigentryRoot + "/orchestrator"
      : result.orchestratorCWD
    try? FileManager.default.createDirectory(
      atPath: orchestratorDir, withIntermediateDirectories: true)
    orchestrator["cwd"] = orchestratorDir
    config["orchestrator"] = orchestrator
    // Pre-create Claude Code trust directory for orchestrator CWD
    if result.orchestratorCLI == "claude" {
      ensureClaudeProjectTrust(cwd: orchestratorDir)
    }
    // Sync CWD to in-memory settings
    AtermSettings.shared.orchestratorCWD = result.orchestratorCWD
    config["onboarding_completed"] = true

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
      onRefreshClis: { [weak self] in
        self?.detectClis() ?? CliStatus(claude: false, codex: false, gemini: false)
      },
      onComplete: { [weak self] result in
        self?.saveOnboardingResult(result)
        if self?.managedWorkspaces.isEmpty ?? true {
          self?.createWorkspaceFromOnboarding(result)
        }
      }
    )

    let hostingView = NSHostingView(rootView: onboardingView)
    let sheet = NSPanel(
      contentRect: NSRect(x: 0, y: 0, width: 440, height: 860),
      styleMask: [.titled, .closable],
      backing: .buffered,
      defer: false
    )
    sheet.contentView = hostingView
    sheet.title = "Welcome to aterm"
    window.beginSheet(sheet)
  }

  @objc private func showPreferences() {
    let settings = AtermSettings.shared
    settings.load()

    let orchestratorWS = managedWorkspaces.values.first(where: {
      $0.isSystem && ($0.name == settings.orchestratorName || $0.name == "orchestrator")
    })
    let isOrchestratorRunning = orchestratorWS != nil && orchestratorWS?.status != "dead"

    let settingsView = SettingsView(
      settings: settings,
      onApply: { [weak self] in
        self?.applySettings()
      },
      onApplyOrchestrator: { [weak self] in
        self?.restartOrchestrator()
      },
      isOrchestratorRunning: isOrchestratorRunning
    )

    let hostingView = NSHostingView(rootView: settingsView)
    let prefsWindow = NSPanel(
      contentRect: NSRect(x: 0, y: 0, width: 480, height: 560),
      styleMask: [.titled, .closable],
      backing: .buffered,
      defer: false
    )
    prefsWindow.contentView = hostingView
    prefsWindow.title = "Settings"
    prefsWindow.center()
    prefsWindow.makeKeyAndOrderFront(nil)
  }

  private func restartOrchestrator() {
    let settings = AtermSettings.shared
    let name = settings.orchestratorName.isEmpty ? "orchestrator" : settings.orchestratorName

    // Close existing orchestrator workspace (allowSystem: true to remove system workspace)
    if let existingID = managedWorkspaces.first(where: {
      $0.value.isSystem
        && ($0.value.name == name || $0.value.name == "orchestrator")
    })?.key {
      removeWorkspace(id: existingID, allowSystem: true)
    }

    // Map CLI string to command
    let command: WorkspaceLaunchCommand
    switch settings.orchestratorCLI {
    case "claude": command = .claude
    case "codex": command = .codex
    case "gemini": command = .gemini
    case "custom": command = .custom
    default: command = .claude
    }

    let orchestratorDir = settings.effectiveOrchestratorCWD
    try? FileManager.default.createDirectory(atPath: orchestratorDir, withIntermediateDirectories: true)
    let cwd = orchestratorDir

    // Pre-create Claude Code trust directory
    if command == .claude {
      ensureClaudeProjectTrust(cwd: cwd)
    }

    NSLog("[aterm] restarting orchestrator: name=%@, cli=%@, cwd=%@", name, command.rawValue, cwd)
    createWorkspace(
      name: name,
      command: command,
      customCommand: settings.orchestratorArgs,
      cwd: cwd,
      shouldSelect: true,
      isSystem: true
    )
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

    // Apply font family FIRST — invalidates atlas under lock if changed, so the
    // subsequent cell-metric calculation uses the new font, and the next render
    // frame re-rasterizes glyphs from it. Ordering is enforced: setFontFamily →
    // metric recompute → FFI push → aterm_core_resize → render. Never reversed.
    GlyphAtlas.shared.setFontFamily(settings.fontFamily)
    let resolvedFontName = GlyphAtlas.shared.resolvedFontFamily

    // Derive cell dimensions from CTFont metrics (Ghostty formula).
    // P0 cursor fix: take max(primary, CJK fallback) line height so cells always
    // accommodate the tallest glyph. Without this, CJK glyphs (whose fallback
    // font has larger ascent+descent+leading) overflow the cell bounding box,
    // causing cursors to appear top-aligned.
    let ctFont = CTFontCreateWithName(resolvedFontName as CFString, CGFloat(settings.fontSize), nil)
    let primaryAscent = CTFontGetAscent(ctFont)
    let primaryDescent = CTFontGetDescent(ctFont)   // positive value
    let primaryLeading = CTFontGetLeading(ctFont)
    let primaryTotal = primaryAscent + primaryDescent + primaryLeading

    // Measure CJK fallback font metrics — these are typically 1-3pt taller than Latin.
    // CTFontCreateForString triggers the same font substitution that GlyphAtlas uses
    // at rasterize time, so we measure the exact fonts that will render CJK glyphs.
    var cjkMaxTotal = primaryTotal
    for cjkProbe in ["한", "漢", "\u{23FA}"] as [CFString] {
        let fallback = CTFontCreateForString(ctFont, cjkProbe, CFRange(location: 0, length: 1))
        let total = CTFontGetAscent(fallback) + CTFontGetDescent(fallback) + CTFontGetLeading(fallback)
        if total > cjkMaxTotal { cjkMaxTotal = total }
    }
    let cellHeight = Float(ceil(max(primaryTotal, cjkMaxTotal)))

    // Cell width: max advance across printable ASCII glyphs
    var glyphs = [CGGlyph](repeating: 0, count: 95)
    let chars: [UniChar] = Array(UniChar(32)...UniChar(126))
    CTFontGetGlyphsForCharacters(ctFont, chars, &glyphs, 95)
    var advances = [CGSize](repeating: .zero, count: 95)
    CTFontGetAdvancesForGlyphs(ctFont, .horizontal, glyphs, &advances, 95)
    let cellWidth = Float(round(advances.map { CGFloat($0.width) }.max() ?? CGFloat(settings.fontSize) * 0.6))

    aterm_core_set_color_scheme(core, schemeIndex)
    aterm_core_set_font_size(core, fontSize)
    view.currentFontSize = fontSize
    view.cursorStyleSetting = settings.cursorStyle
    aterm_core_set_line_height(core, cellHeight)
    aterm_core_set_cell_width(core, cellWidth)

    // Sync Rust default fg/bg and MetalRenderer bg from scheme palette
    var fgR: UInt8 = 0, fgG: UInt8 = 0, fgB: UInt8 = 0
    var bgR: UInt8 = 0, bgG: UInt8 = 0, bgB: UInt8 = 0
    aterm_core_scheme_fg_color(schemeIndex, &fgR, &fgG, &fgB)
    aterm_core_scheme_bg_color(schemeIndex, &bgR, &bgG, &bgB)
    aterm_core_set_default_colors(core, fgR, fgG, fgB, bgR, bgG, bgB)
    view.applySchemeBackground(schemeIndex)

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
        FileManager.default.isExecutableFile(atPath: value)
      else { return nil }
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
      fm.isExecutableFile(atPath: resolved)
    {
      return (resolved, false)
    }

    // 3. Direct binary paths
    let directPaths =
      [
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
    paths.append(
      "\(homeDirectory)/.volta/tools/image/packages/@dmsdc-ai/aigentry-telepty/lib/node_modules/\(teleptyPkg)"
    )

    return paths
  }

  private static func suppressScrollViewFocusRings(in view: NSView?) {
    guard let view = view else { return }
    view.subviews.forEach { sub in
      (sub as? NSScrollView)?.focusRingType = .none
      suppressScrollViewFocusRings(in: sub)
    }
  }

  private func ensureTeleptyDaemon() {
    DispatchQueue.global(qos: .utility).async {
      // Check if telepty daemon is already running
      let teleptyPort = Int(ProcessInfo.processInfo.environment["ATERM_TELEPTY_PORT"] ?? "") ?? 3848
      guard let url = URL(string: "http://127.0.0.1:\(teleptyPort)/api/sessions") else { return }
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
    if let envFlag = env["ATERM_TAILSCALE_ENABLED"],
      envFlag == "0" || envFlag.lowercased() == "false" || envFlag.lowercased() == "no"
    {
      NSLog("[aterm] tailscale disabled via ATERM_TAILSCALE_ENABLED")
      return
    }
    if !UserDefaults.standard.bool(forKey: "AtermTailscaleEnabled")
      && UserDefaults.standard.object(forKey: "AtermTailscaleEnabled") != nil
    {
      NSLog("[aterm] tailscale disabled in settings")
      return
    }
    // Check aterm.json config (set by onboarding)
    let config = readConfig()
    let tailscale = config["tailscale"] as? [String: Any]
    let connectOnLaunch = tailscale?["connect_on_launch"] as? Bool ?? false
    if !connectOnLaunch { return }

    // Capture env values before async dispatch (env dict is reference-safe
    // but capture explicitly for clarity)
    let hostname = env["ATERM_TAILSCALE_HOSTNAME"]
    let controlURL = env["ATERM_TAILSCALE_CONTROL_URL"]
    let authKey = env["ATERM_TAILSCALE_AUTHKEY"]

    // Run tailscale connect off main thread to avoid blocking activation
    DispatchQueue.global(qos: .userInitiated).async { [weak self] in
      let result = withOptionalCString(hostname) { hostnamePtr in
        withOptionalCString(controlURL) { controlURLPtr in
          withOptionalCString(authKey) { authKeyPtr in
            aterm_tailscale_connect(hostnamePtr, controlURLPtr, authKeyPtr)
          }
        }
      }

      if result != 0 {
        NSLog("[aterm] tailscale startup failed (no auth key or network issue) — skipping")
        return
      }

      self?.logTailscaleStatus(prefix: "startup")
    }
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
      !initialDirectory.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    {
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
    let normalizedDrafts = drafts.filter {
      !$0.cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    guard !normalizedDrafts.isEmpty else { return }

    for (index, draft) in normalizedDrafts.enumerated() {
      createWorkspace(
        name: draft.name.trimmingCharacters(in: .whitespacesAndNewlines),
        command: draft.command,
        customCommand: draft.customCommand,
        cliArgs: draft.cliArgs,
        cwd: draft.cwd,
        shouldSelect: index == normalizedDrafts.count - 1
      )
    }
  }

  private func createWorkspace(
    name: String,
    command: WorkspaceLaunchCommand,
    customCommand: String,
    cliArgs: String = "",
    cwd: String,
    shouldSelect: Bool,
    isSystem: Bool = false,
    skipSave: Bool = false
  ) {
    // Dedup: skip creation if a workspace with the same name AND cwd already exists.
    // Prevents duplicate workspaces from IPC CreateWorkspace or external directory scans
    // re-creating workspaces that are already running.
    let normalizedName = name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if managedWorkspaces.values.contains(where: { ws in
      ws.name.lowercased() == normalizedName && ws.cwd == cwd
    }) {
      NSLog("[aterm] skipped duplicate workspace: '%@' (cwd: %@)", name, cwd)
      if shouldSelect, let existingID = workspaceID(named: name) {
        selectWorkspace(existingID)
      }
      return
    }

    // Pre-create Claude Code trust directory so the trust prompt is skipped
    if command == .claude {
      ensureClaudeProjectTrust(cwd: cwd)
    }

    let workspaceID = UUID()
    let resolvedName = uniqueWorkspaceName(for: name, cwd: cwd, excluding: nil)
    let bootstrapCommand = command.bootstrapCommand(customCommand: customCommand, cliArgs: cliArgs)
    let baselineChildPIDs: Set<Int32> = []
    let terminalView = TerminalView(frame: terminalContainerView.bounds)
    terminalView.workspaceName = resolvedName
    terminalView.spawnCommand = bootstrapCommand
    terminalView.initialWorkingDirectory = cwd
    terminalView.autoresizingMask = [.width, .height]
    terminalView.isHidden = true
    terminalView.onShellSpawned = { [weak self, isSystem, resolvedName] result in
      guard let self else { return }
      DispatchQueue.main.async {
        self.refreshWorkspaceProcesses()
      }
      // Propagate isSystem flag to Rust global session registry after spawn registers the session
      if isSystem {
        resolvedName.withCString { namePtr in
          aterm_core_set_workspace_system(nil, namePtr, true)
        }
      }
    }

    let workspace = ManagedWorkspace(
      id: workspaceID,
      name: resolvedName,
      cwd: cwd,
      launchCommand: command,
      customCommand: customCommand,
      cliArgs: cliArgs,
      terminalView: terminalView,
      createdAt: Date(),
      baselineChildPIDs: baselineChildPIDs,
      isSystem: isSystem
    )
    terminalView.onActivity = { [weak self, workspaceID] in
      guard let self else { return }
      DispatchQueue.main.async {
        guard let ws = self.managedWorkspaces[workspaceID] else { return }
        ws.lastActivityAt = Date()
        if ws.status == "idle" {
          ws.status = "working"
          self.rebuildSidebarState()
        }
        self.scheduleIdleCheck(for: workspaceID)
      }
    }
    // Item 6: Rollback workspace if GPU init fails
    terminalView.onGPUInitFailed = { [weak self, workspaceID] in
      guard let self else { return }
      NSLog("[aterm] GPU init failed for workspace — rolling back")
      self.managedWorkspaces.removeValue(forKey: workspaceID)
      self.workspaceOrder.removeAll { $0 == workspaceID }
      self.rebuildSidebarState()
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

    if !skipSave {
      saveWorkspaces()
    }

    // Pre-spawn PTY for non-selected workspaces created mid-session (#177).
    // Without this, only the launch-time pre-spawn loop covers background init;
    // dynamically added workspaces would stay unspawned until first switch.
    if !shouldSelect, window.isVisible {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
        guard let self,
              let ws = self.managedWorkspaces[workspaceID],
              !ws.terminalView.didSpawnShell else { return }
        ws.terminalView.preSpawnInBackground()
      }
    }

    // Delegate MD generation to aigentry-devkit (skip silently if not installed)
    devkitWorkspaceInit(cli: command.rawValue, cwd: cwd, workspaceID: workspaceID)
  }

  private func devkitWorkspaceInit(cli: String, cwd: String, workspaceID: UUID) {
    // GAP 4: Read orchestrator session name from config for hook routing
    let config = readConfig()
    let orchestratorName = (config["orchestrator"] as? [String: Any])?["name"] as? String

    let task = Process()
    task.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    var arguments = ["aigentry-devkit", "workspace-init", "--cli", cli, "--cwd", cwd]
    // GAP 4: Pass orchestrator session name so devkit can configure hooks correctly
    if let orchName = orchestratorName, !orchName.isEmpty {
      arguments += ["--orchestrator-session", orchName]
    }
    task.arguments = arguments

    // Fix: prepend common npm/homebrew bin paths so devkit is found when
    // aterm.app is launched from Finder (which has restricted default PATH).
    // Without this, /usr/bin/env cannot resolve 'aigentry-devkit' → exit 127.
    var env = ProcessInfo.processInfo.environment
    let home = NSHomeDirectory()
    let extraPaths = [
      "\(home)/.npm-global/bin",
      "/usr/local/bin",
      "/opt/homebrew/bin",
    ]
    if let existing = env["PATH"] {
      env["PATH"] = extraPaths.joined(separator: ":") + ":" + existing
    }
    task.environment = env

    // GAP 2: Capture stdout to parse INJECT: lines (was FileHandle.nullDevice)
    let stdoutPipe = Pipe()
    task.standardOutput = stdoutPipe
    task.standardError = FileHandle.nullDevice

    DispatchQueue.global(qos: .utility).async { [weak self] in
      do {
        try task.run()
        task.waitUntilExit()

        // GAP 2: Parse stdout for INJECT: lines and forward to workspace PTY
        let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
        if let output = String(data: stdoutData, encoding: .utf8) {
          let injectLines = output.components(separatedBy: "\n")
            .filter { $0.hasPrefix("INJECT:") }
          if !injectLines.isEmpty {
            DispatchQueue.main.async { [weak self] in
              guard let self,
                    let workspace = self.managedWorkspaces[workspaceID],
                    let core = workspace.terminalView.corePointer else { return }
              for line in injectLines {
                let payload = String(line.dropFirst("INJECT:".count))
                  .trimmingCharacters(in: .whitespacesAndNewlines)
                if !payload.isEmpty {
                  let typed = payload + "\r"
                  typed.withCString { ptr in
                    aterm_core_write_pty(core, ptr, typed.utf8.count)
                  }
                }
              }
            }
          }
        }

        // GAP 3: Log MCP registration status
        let exitCode = task.terminationStatus
        if exitCode == 0 {
          NSLog("[aterm] devkit workspace-init succeeded for %@ — MCP registered", cwd)
        } else {
          NSLog("[aterm] devkit workspace-init exited with %d for %@ — MCP may not be registered", exitCode, cwd)
          if exitCode == 127 {
            NSLog("[aterm] devkit not installed (exit 127) — generating fallback CLAUDE.md/AGENTS.md for %@", cwd)
            self?.generateFallbackMDFiles(cli: cli, cwd: cwd)
          }
        }
      } catch {
        // GAP 1: devkit not installed — generate fallback CLAUDE.md/AGENTS.md
        NSLog("[aterm] aigentry-devkit not installed — generating fallback CLAUDE.md/AGENTS.md for %@", cwd)
        // GAP 3: Log MCP not registered
        NSLog("[aterm] MCP not registered (devkit not installed)")
        self?.generateFallbackMDFiles(cli: cli, cwd: cwd)
      }
    }
  }

  /// GAP 1: Generate minimal CLAUDE.md and AGENTS.md when aigentry-devkit is not installed.
  private func generateFallbackMDFiles(cli: String, cwd: String) {
    let fm = FileManager.default
    let projectName = (cwd as NSString).lastPathComponent

    let claudePath = (cwd as NSString).appendingPathComponent("CLAUDE.md")
    if !fm.fileExists(atPath: claudePath) {
      let content = """
        # \(projectName)

        ## Session
        - CLI: \(cli)
        - Working Directory: \(cwd)

        ## Guidelines
        - Follow project conventions
        - Write clean, tested code
        """.replacingOccurrences(of: "        ", with: "")
      try? content.write(toFile: claudePath, atomically: true, encoding: .utf8)
      NSLog("[aterm] generated fallback CLAUDE.md at %@", claudePath)
    }

    let agentsPath = (cwd as NSString).appendingPathComponent("AGENTS.md")
    if !fm.fileExists(atPath: agentsPath) {
      let content = """
        # \(projectName)

        ## Role
        AI development workspace managed by aterm.

        ## Directory
        \(cwd)
        """.replacingOccurrences(of: "        ", with: "")
      try? content.write(toFile: agentsPath, atomically: true, encoding: .utf8)
      NSLog("[aterm] generated fallback AGENTS.md at %@", agentsPath)
    }
  }

  private func bootstrapWorkspace(id: UUID, command: String) {
    guard let workspace = managedWorkspaces[id] else { return }
    workspace.lastLaunchTime = Date()

    // Cancel any existing pending bootstrap for this workspace
    pendingBootstraps[id]?.fallbackTimer.cancel()

    // Create fallback timer (10s) in case ShellReady never fires
    let fallbackTimer = DispatchWorkItem { [weak self] in
      guard let self else { return }
      NSLog("[aterm] shell_ready fallback timeout (10s) for workspace %@", workspace.name)
      self.pendingBootstraps.removeValue(forKey: id)
      self.sendBootstrapCommand(workspaceID: id, command: command)
    }

    pendingBootstraps[id] = (command: command, fallbackTimer: fallbackTimer)
    DispatchQueue.main.asyncAfter(deadline: .now() + 10.0, execute: fallbackTimer)
  }

  /// Pre-trust workspace for Claude Code by creating the project directory.
  private func ensureClaudeProjectTrust(cwd: String) {
    let encoded = cwd.replacingOccurrences(of: "/", with: "-")
    let claudeProjectDir = NSHomeDirectory() + "/.claude/projects/" + encoded
    try? FileManager.default.createDirectory(
      atPath: claudeProjectDir, withIntermediateDirectories: true)
  }

  private func sendBootstrapCommand(workspaceID: UUID, command: String) {
    guard let workspace = managedWorkspaces[workspaceID],
      let core = workspace.terminalView.corePointer
    else { return }

    // Pre-trust workspace for Claude Code
    if command.contains("claude") {
      ensureClaudeProjectTrust(cwd: workspace.cwd)
    }

    NSLog(
      "[aterm] sending bootstrap to '%@' (len=%d, cliGaveUp=%d): %@",
      workspace.name, command.count, workspace.cliGaveUp ? 1 : 0, command)
    let text = command + "\n"
    text.withCString { ptr in
      aterm_core_write_pty(core, ptr, text.utf8.count)
    }
    workspace.lastLaunchTime = Date()
  }

  private func restartSystemWorkspace(id: UUID) {
    guard let workspace = managedWorkspaces[id],
      workspace.isSystem,
      workspace.status == "restarting",
      !workspace.cliGaveUp
    else { return }

    NSLog(
      "[aterm] re-sending CLI bootstrap to '%@' (attempt %d)", workspace.name,
      workspace.restartCount)

    // Shell (zsh) is already running — just send the CLI command to it
    if let bootstrapCmd = workspace.launchCommand.bootstrapCommand(
      customCommand: workspace.customCommand, cliArgs: workspace.cliArgs)
    {
      bootstrapWorkspace(id: id, command: bootstrapCmd)
    }
    workspace.status = "starting"
  }

  private func workspaceID(named name: String) -> UUID? {
    managedWorkspaces.first(where: { $0.value.name == name })?.key
  }

  private func selectWorkspace(_ id: UUID) {
    guard id != currentSelectedWorkspaceId else { return }
    guard let workspace = managedWorkspaces[id] else { return }

    for candidateID in workspaceOrder {
      let hidden = candidateID != id
      managedWorkspaces[candidateID]?.terminalView.isHidden = hidden
      // #208: Suspend GPU for inactive workspaces (saves ~76MB swapchain each)
      if hidden, let core = managedWorkspaces[candidateID]?.terminalView.corePointer {
        aterm_core_suspend_gpu(core)
      }
    }

    terminalView = workspace.terminalView
    workspaceSidebarModel.selectedWorkspaceName = workspace.name
    busClient.setCore(workspace.terminalView.corePointer)

    // #240: Show orchestrator input bar for isSystem workspaces
    let showInputBar = workspace.isSystem
    orchestratorInputBar.isHidden = !showInputBar
    terminalContainerBottomToWindow.isActive = !showInputBar
    terminalContainerBottomToInputBar.isActive = showInputBar

    // Tell input bar which CLI is active so / dropdown shows the right commands
    if showInputBar {
      orchestratorInputBar.setCLI(workspace.launchCommand.rawValue)
    }

    // ESC toggle: terminal → input bar (only for isSystem workspaces)
    workspace.terminalView.onEscapeToInputBar = showInputBar ? { [weak self] in
      self?.orchestratorInputBar.focus()
    } : nil

    if showInputBar {
      orchestratorInputBar.focus()
    } else {
      window.makeFirstResponder(workspace.terminalView)
    }
    currentSelectedWorkspaceId = id
    rebuildSidebarState()

    // Retry shell spawn for views that failed when hidden during initial setup.
    // This ensures every workspace registers with telepty, not just the first one.
    if !workspace.terminalView.didSpawnShell {
      DispatchQueue.main.async {
        workspace.terminalView.retrySpawnIfNeeded()
      }
    }
  }

  private func selectWorkspace(named name: String) {
    guard let id = workspaceID(named: name) else { return }
    selectWorkspace(id)
  }

  private func renameWorkspace(id: UUID, to nextName: String) {
    guard let workspace = managedWorkspaces[id] else { return }
    let trimmed = nextName.trimmingCharacters(in: .whitespacesAndNewlines)
    let wasSelected = workspaceSidebarModel.selectedWorkspaceName == workspace.name
    workspace.name = uniqueWorkspaceName(
      for: trimmed,
      cwd: workspace.cwd,
      excluding: id
    )
    workspace.terminalView.workspaceName = workspace.name
    if wasSelected {
      workspaceSidebarModel.selectedWorkspaceName = workspace.name
    }
    rebuildSidebarState()
    saveWorkspaces()
  }

  private func renameWorkspace(named currentName: String, toSuggested nextName: String) {
    guard let id = workspaceID(named: currentName) else { return }
    renameWorkspace(id: id, to: nextName)
  }

  private func renameWorkspace(named oldName: String, toExact nextName: String) {
    guard let id = workspaceID(named: oldName),
      let workspace = managedWorkspaces[id]
    else { return }

    let trimmed = nextName.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return }

    if let existing = workspaceID(named: trimmed), existing != id {
      return
    }

    let wasSelected = workspaceSidebarModel.selectedWorkspaceName == workspace.name
    workspace.name = trimmed
    workspace.terminalView.workspaceName = trimmed
    if wasSelected {
      workspaceSidebarModel.selectedWorkspaceName = trimmed
    }
    rebuildSidebarState()
    saveWorkspaces()
  }

  private func removeWorkspace(id: UUID, allowSystem: Bool) {
    guard let workspace = managedWorkspaces[id] else { return }
    if workspace.isSystem && !allowSystem { return }
    workspace.idleTimer?.invalidate()
    workspace.idleTimer = nil
    deregisterTeleptyWorkspace(name: workspace.name)
    managedWorkspaces.removeValue(forKey: id)

    workspace.terminalView.removeFromSuperview()
    workspaceOrder.removeAll { $0 == id }

    if workspaceSidebarModel.selectedWorkspaceName == workspace.name {
      let nextSelection = workspaceOrder.first
      if let nextSelection {
        selectWorkspace(nextSelection)
      } else {
        terminalView = nil
        workspaceSidebarModel.selectedWorkspaceName = nil
        rebuildSidebarState()
      }
    } else {
      rebuildSidebarState()
    }

    saveWorkspaces()
  }

  /// Batch-remove multiple workspaces with a single sidebar rebuild + save (cmux pattern).
  private func removeWorkspaces(ids: [UUID]) {
    var removedNames: [String] = []
    let currentSelection = workspaceSidebarModel.selectedWorkspaceName

    for id in ids {
      guard let workspace = managedWorkspaces[id] else { continue }
      if workspace.isSystem { continue }
      // Explicit PTY cleanup via FFI (deterministic, not ARC-dependent)
      workspace.name.withCString { aterm_workspace_close($0) }
      workspace.idleTimer?.invalidate()
      workspace.idleTimer = nil
      deregisterTeleptyWorkspace(name: workspace.name)
      workspace.terminalView.removeFromSuperview()
      removedNames.append(workspace.name)
      managedWorkspaces.removeValue(forKey: id)
      workspaceOrder.removeAll { $0 == id }
    }

    guard !removedNames.isEmpty else { return }

    // Single selection update after ALL removals
    if let current = currentSelection, removedNames.contains(current) {
      if let nextId = workspaceOrder.first {
        selectWorkspace(nextId)
      } else {
        terminalView = nil
        workspaceSidebarModel.selectedWorkspaceName = nil
        currentSelectedWorkspaceId = nil
      }
    }
    rebuildSidebarState()
    saveWorkspaces()
  }

  private func closeWorkspace(id: UUID) {
    removeWorkspace(id: id, allowSystem: false)
  }

  private func closeWorkspace(named name: String) {
    guard let id = workspaceID(named: name) else { return }
    closeWorkspace(id: id)
  }

  private func restartWorkspace(named name: String) {
    guard let id = workspaceID(named: name),
      let workspace = managedWorkspaces[id]
    else { return }
    let cwd = workspace.cwd
    let wasSelected = workspaceSidebarModel.selectedWorkspaceName == workspace.name
    let isSystem = workspace.isSystem
    let command = workspace.launchCommand
    let customCommand = workspace.customCommand
    let cliArgs = workspace.cliArgs
    removeWorkspace(id: id, allowSystem: true)
    createWorkspace(
      name: name,
      command: command,
      customCommand: customCommand,
      cliArgs: cliArgs,
      cwd: cwd,
      shouldSelect: wasSelected,
      isSystem: isSystem
    )
  }

  private func changeWorkspaceCLI(named name: String, to newCli: String) {
    guard let id = workspaceID(named: name),
      let workspace = managedWorkspaces[id]
    else { return }
    let cwd = workspace.cwd
    let wasSelected = workspaceSidebarModel.selectedWorkspaceName == workspace.name
    let isSystem = workspace.isSystem
    let command: WorkspaceLaunchCommand
    switch newCli {
    case "claude": command = .claude
    case "codex": command = .codex
    case "gemini": command = .gemini
    case "zsh": command = .zsh
    default: command = .custom
    }
    removeWorkspace(id: id, allowSystem: true)
    createWorkspace(
      name: name,
      command: command,
      customCommand: command == .custom ? newCli : "",
      cwd: cwd,
      shouldSelect: wasSelected,
      isSystem: isSystem
    )
  }

  private func removeWorkspace(named name: String, allowSystem: Bool) {
    guard let id = workspaceID(named: name) else { return }
    removeWorkspace(id: id, allowSystem: allowSystem)
  }

  private func attachExternalSession(_ sessionID: String) {
    // Fix 2 (#205): Stop retry after 3 failures
    let failures = attachFailures[sessionID] ?? 0
    if failures >= 3 {
      NSLog("[aterm] attach '%@' blocked — %d consecutive failures", sessionID, failures)
      return
    }

    // Dedup: if a native workspace with this name already exists, don't create attach
    if managedWorkspaces.values.contains(where: { $0.name == sessionID && !$0.isEphemeral }) {
      if let wsID = workspaceID(named: sessionID) {
        selectWorkspace(wsID)
      }
      return
    }

    // Check if already attached — find existing workspace running telepty attach for this session
    for id in workspaceOrder {
      if let ws = managedWorkspaces[id],
        ws.launchCommand == .custom,
        ws.customCommand == "telepty attach \(sessionID)"
      {
        // Fix 1 (#205): If dead, remove ghost and increment failure counter
        if ws.status == "dead" {
          attachFailures[sessionID] = failures + 1
          removeWorkspace(id: id, allowSystem: true)
          if (attachFailures[sessionID] ?? 0) >= 3 {
            NSLog("[aterm] attach '%@' gave up after 3 failures", sessionID)
            return
          }
          break  // removed, fall through to recreate
        }
        selectWorkspace(id)
        return
      }
    }

    // Find the session info from busClient for cwd
    let session = busClient.sessions.first { $0.id == sessionID }
    let cwd = session?.cwd ?? NSHomeDirectory()

    // Fix 3 (#205): Attach workspaces are ephemeral — not persisted to sessions.json
    createWorkspace(
      name: sessionID,
      command: .custom,
      customCommand: "telepty attach \(sessionID)",
      cwd: cwd,
      shouldSelect: true,
      skipSave: true
    )
    if let wsID = workspaceID(named: sessionID) {
      managedWorkspaces[wsID]?.isEphemeral = true
    }
  }

  private func sendKey(toWorkspaceNamed workspaceName: String, key: String) {
    guard let id = workspaceID(named: workspaceName),
      let workspace = managedWorkspaces[id],
      let core = workspace.terminalView.corePointer,
      let payload = keyPayload(for: key)
    else { return }

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
    let oldSettings = AtermSettings.shared
    let prevOrchestratorCLI = oldSettings.orchestratorCLI

    AtermSettings.shared.load()
    applySettings()
    rebuildSidebarState()

    // Detect orchestrator config changes and trigger re-creation
    let newSettings = AtermSettings.shared
    if newSettings.orchestratorCLI != prevOrchestratorCLI
    {
      NSLog("[aterm] orchestrator config changed via IPC, restarting orchestrator")
      restartOrchestrator()
    }
  }

  private func rebuildSidebarState() {
    workspaceSidebarModel.refreshWorkspaces()
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

  /// Schedule a 30s idle check for a workspace. Replaces 1Hz polling.
  private func scheduleIdleCheck(for id: UUID) {
    guard let ws = managedWorkspaces[id] else { return }
    ws.idleTimer?.invalidate()
    ws.idleTimer = Timer.scheduledTimer(withTimeInterval: 30.0, repeats: false) { [weak self] _ in
      guard let self, let ws = self.managedWorkspaces[id] else { return }
      let idleTime = Date().timeIntervalSince(ws.lastActivityAt)
      if idleTime >= 30 && ws.status == "working" {
        ws.status = "idle"
        self.rebuildSidebarState()
      } else if ws.status == "working" {
        // Still active — reschedule for remaining time
        self.scheduleIdleCheck(for: id)
      }
    }
  }

  private func cleanupStaleWorkspaces() {
    let now = Date()
    let staleThreshold: TimeInterval = 15
    var staleIDs: [UUID] = []

    for (id, workspace) in managedWorkspaces {
      if workspace.status == "dead" {
        if now.timeIntervalSince(workspace.lastActivityAt) > staleThreshold {
          staleIDs.append(id)
        }
      }
    }

    for id in staleIDs {
      if let ws = managedWorkspaces[id] {
        print("[aterm] removing stale dead workspace: \(ws.name)")
      }
      removeWorkspace(id: id, allowSystem: true)
    }
  }

  private func refreshWorkspaceProcesses() {
    let now = Date()
    for id in workspaceOrder {
      guard let workspace = managedWorkspaces[id] else { continue }
      let defaultProcessName =
        workspace.cliGaveUp
        ? "shell (CLI unavailable)"
        : workspace.launchCommand.displayTitle(customCommand: workspace.customCommand)

      guard workspace.terminalView.didSpawnShell else {
        workspace.status = "starting"
        workspace.foregroundProcessName = defaultProcessName
        continue
      }

      if workspace.status == "dead" || workspace.status == "closing" {
        workspace.foregroundProcessName = defaultProcessName
        continue
      }

      // Determine if working or idle based on last activity (30s threshold)
      let idleTime = now.timeIntervalSince(workspace.lastActivityAt)
      if idleTime < 30 {
        workspace.status = "working"
        scheduleIdleCheck(for: id)
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
    let pairs =
      output
      .split(separator: "\n")
      .compactMap { line -> (Int32, Int32)? in
        let parts =
          line
          .split(whereSeparator: \.isWhitespace)
          .map(String.init)
        guard parts.count >= 2,
          let pid = Int32(parts[0]),
          let ppid = Int32(parts[1])
        else {
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

    let chosen =
      (relevantCandidates.isEmpty ? processes.filter { $0.pid == rootPID } : relevantCandidates)
      .sorted(by: { left, right in
        processPriority(for: left, rootPID: rootPID) > processPriority(for: right, rootPID: rootPID)
      })
      .first

    return chosen.map { displayProcessName($0.command) }
  }

  private func ttyProcesses(for tty: String) -> [TTYProcessSnapshot] {
    let output = runPS(arguments: ["-t", tty, "-o", "pid=,ppid=,stat=,comm="])
    return
      output
      .split(separator: "\n")
      .compactMap { line in
        let parts =
          line
          .split(maxSplits: 3, omittingEmptySubsequences: true, whereSeparator: \.isWhitespace)
          .map(String.init)
        guard parts.count == 4,
          let pid = Int32(parts[0]),
          let ppid = Int32(parts[1])
        else {
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

  private func isDescendant(_ pid: Int32, of ancestorPID: Int32, parentByPID: [Int32: Int32])
    -> Bool
  {
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

  private func processPriority(for process: TTYProcessSnapshot, rootPID: Int32) -> (Int, Int, Int32)
  {
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
    !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  else {
    return body(nil)
  }

  return value.withCString { ptr in
    body(ptr)
  }
}

private let workspaceInfoTimestampFormatter: ISO8601DateFormatter = {
  let formatter = ISO8601DateFormatter()
  formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
  return formatter
}()

private let knownShells: Set<String> = ["bash", "fish", "sh", "zsh"]

private final class ManagedWorkspace {
  let id: UUID
  let cwd: String
  let launchCommand: WorkspaceLaunchCommand
  let customCommand: String
  let cliArgs: String
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
  var isEphemeral: Bool = false  // #193: IPC-created workspaces — not persisted to sessions.json
  var idleTimer: Timer?

  init(
    id: UUID,
    name: String,
    cwd: String,
    launchCommand: WorkspaceLaunchCommand,
    customCommand: String,
    cliArgs: String = "",
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
    self.cliArgs = cliArgs
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
