import AppKit

/// A CLI slash-command hint shown in the command palette.
struct CLICommand {
    let name: String         // e.g. "/clear"
    let description: String  // e.g. "Clear conversation history"
}

/// Static command definitions per CLI.
enum OrchestratorCommands {
    static func commands(forCLI cli: String) -> [CLICommand] {
        switch cli.lowercased() {
        case "claude":
            return [
                CLICommand(name: "/help", description: "Show available commands"),
                CLICommand(name: "/clear", description: "Clear conversation history"),
                CLICommand(name: "/compact", description: "Compact conversation context"),
                CLICommand(name: "/model", description: "Change Claude model"),
                CLICommand(name: "/status", description: "Show session status"),
                CLICommand(name: "/cost", description: "Show token usage & cost"),
                CLICommand(name: "/review", description: "Request code review"),
                CLICommand(name: "/init", description: "Initialize project context"),
                CLICommand(name: "/quit", description: "Exit Claude"),
            ]
        case "gemini":
            return [
                CLICommand(name: "/help", description: "Show available commands"),
                CLICommand(name: "/clear", description: "Clear conversation history"),
                CLICommand(name: "/stats", description: "Show session statistics"),
                CLICommand(name: "/tools", description: "List available tools"),
                CLICommand(name: "/quit", description: "Exit Gemini"),
            ]
        case "codex":
            return [
                CLICommand(name: "/help", description: "Show available commands"),
                CLICommand(name: "/clear", description: "Clear conversation history"),
                CLICommand(name: "/new", description: "Start a new session"),
                CLICommand(name: "/resume", description: "Resume previous session"),
                CLICommand(name: "/quit", description: "Exit Codex"),
            ]
        default:
            return []
        }
    }

    static func filter(_ commands: [CLICommand], prefix: String) -> [CLICommand] {
        guard prefix.hasPrefix("/") else { return [] }
        let needle = prefix.lowercased()
        if needle == "/" { return commands }
        return commands.filter { $0.name.lowercased().hasPrefix(needle) }
    }
}

/// NSTableView subclass that refuses first responder status so NSPopover cannot
/// promote its own window to key. This is the PRIMARY fix for focus theft:
///
/// Root cause: `NSTableView.acceptsFirstResponder` defaults to true when
/// `numberOfRows > 0`. When NSPopover shows a table with rows, AppKit promotes
/// the table to first responder inside the popover's window, which in turn
/// promotes the popover's window to key. All keyboard events then flow to the
/// NSTableView — including character keys (filter-as-you-type broken), Tab
/// (autocomplete broken), and arrow keys (native handling bypasses our
/// handleArrowUp/Down path). Only Enter was rescued by the prior `keyDown`
/// override fix.
///
/// Fix: return `false` from `acceptsFirstResponder`. The table cannot become
/// first responder; combined with `becomesKeyOnlyIfNeeded = true` on the
/// popover's window (set in `popoverDidShow` observer), the popover window
/// never becomes key. Keyboard events continue flowing to OrchestratorTextView
/// in the parent window, which owns the filter/Tab/character/arrow input logic.
///
/// Mouse clicks still work via target-action pattern (tableClicked) regardless
/// of first responder state.
///
/// The `keyDown` override below remains as defensive belt-and-suspenders — if
/// a future macOS version or edge case somehow makes this table first responder
/// anyway (e.g., programmatic `makeFirstResponder`), Return/Tab/ESC still route
/// correctly instead of being silently dropped.
final class DropdownTableView: NSTableView {
    var onConfirmKey: (() -> Void)?
    var onCancelKey: (() -> Void)?

    /// Refuse first responder — PRIMARY focus-theft fix.
    override var acceptsFirstResponder: Bool { false }

    override func keyDown(with event: NSEvent) {
        // Defensive — should not fire in the normal fix path since this table
        // is never first responder. Retained for macOS edge cases and safety.
        switch event.keyCode {
        case 36, 76:  // Return / Numpad Enter → confirm current selection
            onConfirmKey?()
            return
        case 48:  // Tab → same as Enter for dropdown (matches handleTab semantics)
            onConfirmKey?()
            return
        case 53:  // ESC → cancel dropdown
            onCancelKey?()
            return
        default:
            break
        }
        super.keyDown(with: event)
    }
}

/// NSPopover-hosted dropdown showing filtered slash commands.
final class CommandDropdown: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private let popover = NSPopover()
    private let scroll = NSScrollView()
    private let tableView = DropdownTableView()
    private var filtered: [CLICommand] = []
    private(set) var currentCLI: String = ""
    /// Weak ref to the parent window captured at show() time — used in
    /// popoverDidShow to restore key status to the parent so keyboard events
    /// continue flowing to OrchestratorTextView instead of the popover.
    private weak var anchorWindow: NSWindow?

    /// Called when user selects a command (via click or Tab/Enter from the input bar).
    var onSelect: ((CLICommand) -> Void)?

    override init() {
        super.init()
        configurePopover()
        configureTable()
    }

    private func configurePopover() {
        popover.behavior = .transient
        popover.animates = false
    }

    /// Restore key status to the parent window after the popover has been shown.
    /// Called synchronously right after `popover.show(...)` so the popover's
    /// internal window (typically NSPanel) can have `becomesKeyOnlyIfNeeded`
    /// set and the parent window can be forced back to key — which keeps
    /// OrchestratorTextView receiving keyboard events (filter-as-you-type,
    /// Tab autocomplete, character input, arrow navigation via our handlers).
    ///
    /// NOTE: previously this was wired via NSPopover.didShowNotification observer
    /// + #selector in `configurePopover`. Inlined here to avoid any side effects
    /// from NotificationCenter observation at CommandDropdown init time (which
    /// runs during OrchestratorInputBar init before the view hierarchy exists).
    private func configurePopoverWindowAfterShow() {
        if let popoverWindow = popover.contentViewController?.view.window {
            if let panel = popoverWindow as? NSPanel {
                panel.becomesKeyOnlyIfNeeded = true
            }
        }
        anchorWindow?.makeKey()
    }

    private func configureTable() {
        tableView.headerView = nil
        tableView.rowSizeStyle = .default
        tableView.rowHeight = 30
        tableView.backgroundColor = AtermTheme.panelBackground
        tableView.selectionHighlightStyle = .regular
        tableView.intercellSpacing = NSSize(width: 0, height: 2)
        tableView.gridStyleMask = []
        tableView.allowsEmptySelection = false
        tableView.allowsMultipleSelection = false
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.action = #selector(tableClicked(_:))

        // Route Return/Tab/ESC through the table subclass — handles the case
        // where NSPopover has stolen first responder from OrchestratorTextView.
        tableView.onConfirmKey = { [weak self] in
            guard let self = self else { return }
            let cmd = self.confirmCurrent()
            self.hide()
            if let cmd = cmd {
                self.onSelect?(cmd)
            }
        }
        tableView.onCancelKey = { [weak self] in
            self?.hide()
        }

        let nameColumn = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("name"))
        nameColumn.width = 110
        nameColumn.minWidth = 110
        tableView.addTableColumn(nameColumn)

        let descColumn = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("desc"))
        descColumn.width = 260
        descColumn.minWidth = 200
        tableView.addTableColumn(descColumn)

        scroll.documentView = tableView
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let host = NSViewController()
        let container = NSView()
        container.wantsLayer = true
        container.layer?.backgroundColor = AtermTheme.panelBackground.cgColor
        container.addSubview(scroll)
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: container.topAnchor, constant: 4),
            scroll.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -4),
            scroll.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 4),
            scroll.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -4),
        ])
        host.view = container
        popover.contentViewController = host
    }

    // MARK: - API

    var isShown: Bool { popover.isShown }

    func setCLI(_ cli: String) {
        currentCLI = cli
    }

    func show(anchor: NSView, prefix: String) {
        let all = OrchestratorCommands.commands(forCLI: currentCLI)
        filtered = OrchestratorCommands.filter(all, prefix: prefix)
        if filtered.isEmpty {
            hide()
            return
        }
        // Capture anchor's window so we can restore key status to it after
        // popover.show() — prevents focus theft that breaks filter-as-you-type /
        // Tab autocomplete / character input.
        anchorWindow = anchor.window
        let rows = min(filtered.count, 8)
        let width: CGFloat = 380
        let height = CGFloat(rows) * (tableView.rowHeight + 2) + 8
        popover.contentSize = NSSize(width: width, height: height)
        tableView.reloadData()
        tableView.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)

        if !popover.isShown {
            popover.show(
                relativeTo: anchor.bounds,
                of: anchor,
                preferredEdge: .maxY
            )
            // Configure popover window + restore parent key status synchronously
            // after show. Inlined (was previously wired via notification observer
            // in configurePopover — removed to eliminate init-time side effects).
            configurePopoverWindowAfterShow()
        }
    }

    func hide() {
        if popover.isShown {
            popover.performClose(nil)
        }
    }

    func selectNext() {
        guard !filtered.isEmpty else { return }
        let row = tableView.selectedRow
        let next = (row + 1) % filtered.count
        tableView.selectRowIndexes(IndexSet(integer: next), byExtendingSelection: false)
        tableView.scrollRowToVisible(next)
    }

    func selectPrevious() {
        guard !filtered.isEmpty else { return }
        let row = tableView.selectedRow
        let prev = row <= 0 ? filtered.count - 1 : row - 1
        tableView.selectRowIndexes(IndexSet(integer: prev), byExtendingSelection: false)
        tableView.scrollRowToVisible(prev)
    }

    func confirmCurrent() -> CLICommand? {
        let row = tableView.selectedRow
        guard row >= 0, row < filtered.count else { return nil }
        return filtered[row]
    }

    // MARK: - NSTableViewDataSource

    func numberOfRows(in tableView: NSTableView) -> Int {
        filtered.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int)
        -> NSView?
    {
        guard row < filtered.count else { return nil }
        let cmd = filtered[row]
        let id = tableColumn?.identifier.rawValue ?? ""
        let text: String = (id == "name") ? cmd.name : cmd.description
        let color: NSColor = (id == "name") ? AtermTheme.orchestrator : AtermTheme.textSecondary

        let cell = NSTableCellView()
        let label = NSTextField(labelWithString: text)
        label.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        label.textColor = color
        label.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 8),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -4),
            label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    // MARK: - Actions

    @objc private func tableClicked(_ sender: NSTableView) {
        let row = sender.clickedRow
        guard row >= 0, row < filtered.count else { return }
        let cmd = filtered[row]
        hide()
        onSelect?(cmd)
    }
}
