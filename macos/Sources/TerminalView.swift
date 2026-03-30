import AppKit
import QuartzCore

class TerminalView: NSView, NSTextInputClient {
    private(set) var core: OpaquePointer?  // AtermCore*
    var corePointer: OpaquePointer? { core }
    var initialWorkingDirectory: String = NSHomeDirectory()
    private var displayLink: CVDisplayLink?
    private var markedText = NSMutableAttributedString()
    private var inputContext_: NSTextInputContext?
    private var hasInitializedCore = false
    private var shellSpawned = false

    // Ghostty pattern: set to non-nil during keyDown to accumulate insertText contents
    private var keyTextAccumulator: [String]?

    // MARK: - Init

    override init(frame: NSRect) {
        super.init(frame: frame)
        commonInit()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        commonInit()
    }

    private func commonInit() {
        wantsLayer = true
        let metalLayer = CAMetalLayer()
        metalLayer.device = MTLCreateSystemDefaultDevice()
        metalLayer.pixelFormat = .bgra8Unorm_srgb
        metalLayer.framebufferOnly = true
        metalLayer.contentsScale = NSScreen.main?.backingScaleFactor ?? 2.0
        layer = metalLayer

        // Create core
        core = aterm_core_new()

        // Custom NSTextInputContext for CAMetalLayer-based view
        inputContext_ = NSTextInputContext(client: self)
    }

    override var inputContext: NSTextInputContext? {
        return inputContext_
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard !hasInitializedCore else { return }
        guard let window = self.window, let core = core else { return }
        hasInitializedCore = true

        // Init GPU with this view's pointer
        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        let size = self.convertToBacking(bounds).size
        let w = UInt32(size.width)
        let h = UInt32(size.height)

        let scale = Float(window.backingScaleFactor)
        let result = aterm_core_init_gpu(core, viewPtr, w, h, scale)
        if result != 0 {
            NSLog("[aterm] GPU init failed: %d", result)
            return
        }

        // Set dirty callback — wakes the display link
        let ud = Unmanaged.passUnretained(self).toOpaque()
        aterm_core_set_dirty_callback(core, { userdata in
            guard let userdata = userdata else { return }
            let view = Unmanaged<TerminalView>.fromOpaque(userdata).takeUnretainedValue()
            DispatchQueue.main.async {
                view.needsDisplay = true
            }
        }, ud)

        // Start display link for rendering
        startDisplayLink()

        window.makeFirstResponder(self)

        DispatchQueue.main.async { [weak self] in
            self?.spawnShellIfNeeded()
        }
    }

    deinit {
        stopDisplayLink()
        if let core = core {
            aterm_core_free(core)
        }
    }

    // MARK: - Responder

    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        inputContext_?.activate()
        return super.becomeFirstResponder()
    }

    // MARK: - Display Link

    private func startDisplayLink() {
        guard displayLink == nil else { return }

        var link: CVDisplayLink?
        CVDisplayLinkCreateWithActiveCGDisplays(&link)
        guard let link = link else { return }

        let ud = Unmanaged.passUnretained(self).toOpaque()
        CVDisplayLinkSetOutputCallback(link, { (_, _, _, _, _, userdata) -> CVReturn in
            guard let userdata = userdata else { return kCVReturnSuccess }
            let view = Unmanaged<TerminalView>.fromOpaque(userdata).takeUnretainedValue()
            DispatchQueue.main.async {
                view.renderFrame()
            }
            return kCVReturnSuccess
        }, ud)

        CVDisplayLinkStart(link)
        displayLink = link
    }

    private func stopDisplayLink() {
        if let link = displayLink {
            CVDisplayLinkStop(link)
            displayLink = nil
        }
    }

    private func renderFrame() {
        guard let core = core else { return }
        // Only render if dirty
        if aterm_core_take_dirty(core) != 0 || needsDisplay {
            aterm_core_render(core)
            needsDisplay = false
        }
    }

    // MARK: - Resize

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        guard let core = core else { return }

        let backingSize = convertToBacking(NSRect(origin: .zero, size: newSize)).size
        let w = UInt32(backingSize.width)
        let h = UInt32(backingSize.height)

        (layer as? CAMetalLayer)?.drawableSize = backingSize
        aterm_core_resize(core, w, h)
        spawnShellIfNeeded()
        aterm_core_render(core)
    }

    // MARK: - Keyboard Input

    override func keyDown(with event: NSEvent) {
        guard let core = core else { return }
        NSLog("[TV] keyDown keyCode=%d chars=%@ inputContext=%@", event.keyCode, event.characters ?? "nil", String(describing: self.inputContext))

        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        // Handle macOS terminal navigation shortcuts directly so they do not get
        // swallowed by AppKit text navigation before reaching the PTY.
        if flags.contains(.option) && !flags.contains(.command) {
            switch event.keyCode {
            case 123: // Option+Left
                aterm_core_write_pty(core, "\u{1b}b", 2)
                return
            case 124: // Option+Right
                aterm_core_write_pty(core, "\u{1b}f", 2)
                return
            default:
                break
            }
        }

        // Handle Ctrl+key combinations directly (bypass IME)
        if flags.contains(.control), let chars = event.charactersIgnoringModifiers {
            if let scalar = chars.unicodeScalars.first {
                let code = scalar.value
                if code >= 0x61 && code <= 0x7A { // a-z
                    let ctrl = UInt8(code - 0x60)
                    let byte = [ctrl]
                    byte.withUnsafeBufferPointer { buf in
                        aterm_core_write_pty(core, buf.baseAddress!, 1)
                    }
                    return
                }
            }
        }

        // Ghostty pattern: accumulator collects insertText results during interpretKeyEvents.
        // This is CRITICAL for Korean input — interpretKeyEvents triggers NSTextInputClient
        // methods (insertText, setMarkedText, doCommand) which we process after.
        let markedTextBefore = markedText.length > 0
        keyTextAccumulator = []
        defer { keyTextAccumulator = nil }

        NSLog("[TV] calling interpretKeyEvents")
        self.interpretKeyEvents([event])

        // Process accumulated text from insertText calls
        if let acc = keyTextAccumulator, !acc.isEmpty {
            NSLog("[TV] accumulated %d text(s)", acc.count)
            for text in acc {
                text.withCString { ptr in
                    aterm_core_write_pty(core, ptr, text.utf8.count)
                }
            }
        } else if !markedTextBefore && markedText.length == 0 {
            // No text accumulated and no marked text — might be a key event
            // that interpretKeyEvents handled via doCommand (special keys).
            // doCommand already handled it, nothing to do here.
            NSLog("[TV] no text accumulated (handled by doCommand or IME)")
        }
    }

    // Called by NSTextInputClient when text is committed (Ghostty pattern)
    func insertText(_ string: Any, replacementRange: NSRange) {
        NSLog("[TV] insertText: %@, hasAccumulator=%@", String(describing: string), keyTextAccumulator != nil ? "true" : "false")

        // We must have an associated event
        guard NSApp.currentEvent != nil else { return }

        var chars = ""
        switch string {
        case let v as NSAttributedString: chars = v.string
        case let v as String: chars = v
        default: return
        }

        // If insertText is called, preedit is over (Ghostty pattern)
        unmarkText()

        // If we have an accumulator, we're inside keyDown → interpretKeyEvents.
        // Accumulate and return — keyDown will process after interpretKeyEvents completes.
        if keyTextAccumulator != nil {
            keyTextAccumulator!.append(chars)
            return
        }

        // No accumulator = called outside keyDown (e.g., paste, external event).
        // Send directly to PTY.
        guard let core = core, !chars.isEmpty else { return }
        chars.withCString { ptr in
            aterm_core_write_pty(core, ptr, chars.utf8.count)
        }
    }

    // MARK: - NSTextInputClient (IME)

    func hasMarkedText() -> Bool {
        NSLog("[TV] hasMarkedText: %d", markedText.length)
        return markedText.length > 0
    }

    func markedRange() -> NSRange {
        guard markedText.length > 0 else { return NSRange() }
        return NSRange(0...(markedText.length - 1))
    }

    func selectedRange() -> NSRange {
        NSLog("[TV] selectedRange -> {0, 0}")
        return NSRange(location: 0, length: 0)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        NSLog("[TV] setMarkedText: %@ selectedRange=(%d,%d)", String(describing: string), selectedRange.location, selectedRange.length)
        switch string {
        case let v as NSAttributedString:
            markedText = NSMutableAttributedString(attributedString: v)
        case let v as String:
            markedText = NSMutableAttributedString(string: v)
        default:
            return
        }

        // Update window title with preedit text for visual feedback
        if markedText.length > 0 {
            window?.title = "aterm v3 [\(markedText.string)]"
        } else {
            window?.title = "aterm v3"
        }
    }

    func unmarkText() {
        markedText.mutableString.setString("")
        window?.title = "aterm v3"
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        return []
    }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        return nil
    }

    func characterIndex(for point: NSPoint) -> Int {
        return 0
    }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        // Return cursor position in screen coordinates for IME popup
        guard let window = self.window else {
            return NSRect(x: frame.origin.x, y: frame.origin.y, width: 0, height: 0)
        }
        // Place at bottom-left of the view for now (can be refined with cursor position later)
        let viewRect = NSRect(x: 0, y: 0, width: 0, height: 20)
        let winRect = convert(viewRect, to: nil)
        return window.convertToScreen(winRect)
    }

    // MARK: - Mouse Selection

    /// Convert view pixel coordinates to terminal cell (col, line)
    private func pixelToCell(_ point: NSPoint) -> (col: UInt32, line: Int32) {
        guard let core = core else { return (0, 0) }
        var cols: UInt16 = 0
        var rows: UInt16 = 0
        let backingSize = convertToBacking(bounds).size
        aterm_core_grid_size(core, Float(backingSize.width), Float(backingSize.height), &cols, &rows)

        let backingPoint = convertToBacking(point)
        // Cell size in backing pixels
        let cellW = backingSize.width / CGFloat(cols)
        let cellH = backingSize.height / CGFloat(rows)

        let col = UInt32(max(0, min(Int(backingPoint.x / cellW), Int(cols) - 1)))
        // NSView Y is bottom-up, terminal Y is top-down
        let flippedY = backingSize.height - backingPoint.y
        let line = Int32(max(0, min(Int(flippedY / cellH), Int(rows) - 1)))

        return (col, line)
    }

    override func mouseDown(with event: NSEvent) {
        guard let core = core else { return }
        let loc = convert(event.locationInWindow, from: nil)
        let cell = pixelToCell(loc)
        let side: UInt8 = 0 // Left side

        // Clear previous selection, start new
        aterm_core_selection_clear(core)
        aterm_core_selection_start(core, cell.col, cell.line, side)
        aterm_core_render(core)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let core = core else { return }
        let loc = convert(event.locationInWindow, from: nil)
        let cell = pixelToCell(loc)
        let side: UInt8 = 1 // Right side for drag

        aterm_core_selection_update(core, cell.col, cell.line, side)
        aterm_core_render(core)
    }

    override func mouseUp(with event: NSEvent) {
        // Selection complete — keep it visible until next click or text input
    }

    // MARK: - Scroll

    override func scrollWheel(with event: NSEvent) {
        guard let core = core else { return }
        // Accumulate scroll delta — positive = scroll up (show history), negative = scroll down
        let delta: Int32
        if event.hasPreciseScrollingDeltas {
            // Trackpad: smooth scrolling, accumulate fractional lines
            delta = Int32(-event.scrollingDeltaY / 3.0)
        } else {
            // Mouse wheel: discrete steps
            delta = Int32(-event.scrollingDeltaY)
        }
        if delta != 0 {
            aterm_core_scroll(core, delta)
            aterm_core_render(core)
        }
    }

    // MARK: - Special Keys (arrows, function keys)

    override func keyUp(with event: NSEvent) {
        // no-op, but needed to prevent beep
    }

    override func flagsChanged(with event: NSEvent) {
        // Handle modifier key changes if needed
    }

    private func spawnShellIfNeeded() {
        guard !shellSpawned, let core = core else { return }

        let backingSize = convertToBacking(bounds).size
        guard backingSize.width > 0, backingSize.height > 0 else { return }

        var cols: UInt16 = 0
        var rows: UInt16 = 0
        aterm_core_grid_size(core, Float(backingSize.width), Float(backingSize.height), &cols, &rows)
        guard cols > 2, rows > 1 else { return }

        let cwd = initialWorkingDirectory.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? NSHomeDirectory()
            : initialWorkingDirectory
        _ = cwd.withCString { cwd in
            aterm_core_spawn_shell(core, cwd, cols, rows)
        }
        shellSpawned = true
    }

    // Capture Cmd+key that bypass keyDown
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.type == .keyDown else { return false }
        guard let core = core else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        guard flags.contains(.command) else { return false }

        switch event.keyCode {
        case 8: // Cmd+C → copy selection, or SIGINT if no selection
            if let textPtr = aterm_core_selection_text(core) {
                let text = String(cString: textPtr)
                aterm_core_free_string(textPtr)
                if !text.isEmpty {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                    aterm_core_selection_clear(core)
                    aterm_core_render(core)
                    return true
                }
            }
            // No selection → send SIGINT
            aterm_core_write_pty(core, "\u{03}", 1)
            return true
        case 9: // Cmd+V → paste from clipboard
            if let text = NSPasteboard.general.string(forType: .string) {
                text.withCString { ptr in
                    aterm_core_write_pty(core, ptr, text.utf8.count)
                }
            }
            return true
        case 51: // Cmd+Backspace → kill line (Ctrl+U)
            aterm_core_write_pty(core, "\u{15}", 1)
            return true
        case 123: // Cmd+Left → line start
            aterm_core_write_pty(core, "\u{01}", 1)
            return true
        case 124: // Cmd+Right → line end
            aterm_core_write_pty(core, "\u{05}", 1)
            return true
        default:
            return false
        }
    }

    // doCommand handles special keys routed by interpretKeyEvents
    override func doCommand(by selector: Selector) {
        NSLog("[TV] doCommand: %@", NSStringFromSelector(selector))
        guard let core = core else { return }

        switch selector {
        case #selector(insertNewline(_:)):
            // If inside interpretKeyEvents (IME may have just committed text),
            // defer Enter so committed text is sent to PTY first.
            if keyTextAccumulator != nil {
                keyTextAccumulator!.append("\r")
            } else {
                aterm_core_named_key(core, UInt32(ATERM_KEY_ENTER))
            }
        case #selector(deleteBackward(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_BACKSPACE))
        case #selector(deleteWordBackward(_:)): // Option+Backspace → ESC DEL (word delete)
            aterm_core_write_pty(core, "\u{1b}\u{7f}", 2)
        case #selector(deleteForward(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_DELETE))
        case #selector(insertTab(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_TAB))
        case #selector(cancelOperation(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_ESCAPE))
        case #selector(moveUp(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_ARROW_UP))
        case #selector(moveDown(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_ARROW_DOWN))
        case #selector(moveRight(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_ARROW_RIGHT))
        case #selector(moveLeft(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_ARROW_LEFT))
        case #selector(moveWordLeft(_:)):
            aterm_core_write_pty(core, "\u{1b}b", 2)
        case #selector(moveWordRight(_:)):
            aterm_core_write_pty(core, "\u{1b}f", 2)
        case #selector(moveToBeginningOfLine(_:)):
            aterm_core_write_pty(core, "\u{01}", 1)
        case #selector(moveToEndOfLine(_:)):
            aterm_core_write_pty(core, "\u{05}", 1)
        case #selector(moveToBeginningOfDocument(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_HOME))
        case #selector(moveToEndOfDocument(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_END))
        case #selector(scrollPageUp(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_PAGE_UP))
        case #selector(scrollPageDown(_:)):
            aterm_core_named_key(core, UInt32(ATERM_KEY_PAGE_DOWN))
        default:
            // no-op — prevents NSBeep for unhandled selectors
            break
        }
    }
}
