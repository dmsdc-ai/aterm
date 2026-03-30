import AppKit
import SwiftUI

class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var terminalView: TerminalView!
    var busClient: TeleptyBusClient!
    var splitView: NSSplitView!

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
        let sidebarView = SessionSidebarView(busClient: busClient)
        let sidebarHost = NSHostingView(rootView: sidebarView)
        sidebarHost.frame = NSRect(x: 0, y: 0, width: 240, height: rect.height)

        // Create terminal view
        let terminalRect = NSRect(x: 0, y: 0, width: rect.width - 240, height: rect.height)
        terminalView = TerminalView(frame: terminalRect)

        // Create split view
        splitView = NSSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        splitView.frame = rect
        splitView.autoresizingMask = [.width, .height]

        splitView.addSubview(sidebarHost)
        splitView.addSubview(terminalView)

        // Set sidebar constraints
        splitView.setHoldingPriority(.defaultLow, forSubviewAt: 0)
        splitView.setHoldingPriority(.defaultHigh, forSubviewAt: 1)
        sidebarHost.widthAnchor.constraint(greaterThanOrEqualToConstant: 180).isActive = true
        sidebarHost.widthAnchor.constraint(lessThanOrEqualToConstant: 400).isActive = true
        splitView.setPosition(240, ofDividerAt: 0)

        // Wire bus client to aterm-core for workspace polling
        busClient.setCore(terminalView.corePointer)

        window.contentView = splitView
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(terminalView)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }
}
