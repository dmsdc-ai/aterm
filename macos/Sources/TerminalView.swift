import AppKit
import QuartzCore

class TerminalView: NSView, NSTextInputClient {
    private(set) var core: OpaquePointer?  // AtermCore*
    var corePointer: OpaquePointer? { core }
    var didSpawnShell: Bool { shellSpawned }
    var isPtyAlive: Bool {
        guard shellSpawned, let core = core else { return false }
        return aterm_core_workspace_is_alive(core) != 0
    }
    var onActivity: (() -> Void)?
    var onShellSpawned: ((Int32) -> Void)?
    var workspaceName: String = "main"
    var spawnCommand: String?  // nil = default shell, "claude" = run claude directly
    var initialWorkingDirectory: String = NSHomeDirectory()
    private var displayLink: CVDisplayLink?
    private var markedText = NSMutableAttributedString()
    private var inputContext_: NSTextInputContext?
    private var hasInitializedCore = false
    private var shellSpawned = false
    private var backgroundSpawnAllowed = false
    private var lastAppliedThemeMode: AtermThemeMode?
    private var isDraggingSelection = false
    private var dragStartGridPoint: (col: UInt32, row: Int32)?

    // Ghostty pattern: set to non-nil during keyDown to accumulate insertText contents
    private var keyTextAccumulator: [String]?

    // Resize dedup: skip resize+render if pixel dimensions unchanged (cmux/Ghostty pattern)
    private var lastDrawableSize: CGSize = .zero
    private var hasObservedLayoutBounds = false

    // Scroll: accumulate fractional trackpad deltas before converting to lines
    private var scrollAccumulator: CGFloat = 0.0

    // Auto-scroll during drag selection near edges
    private var autoScrollTimer: Timer?
    private var autoScrollDirection: Int32 = 0  // +1 = up (scroll back), -1 = down
    private var lastDragEvent: NSEvent?

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
        metalLayer.masksToBounds = true
        layer = metalLayer
        syncMetalLayerBacking()

        // Create core
        core = aterm_core_new()

        // Custom NSTextInputContext for CAMetalLayer-based view
        inputContext_ = NSTextInputContext(client: self)
        applyTheme()
    }

    override var inputContext: NSTextInputContext? {
        return inputContext_
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        syncMetalLayerBacking()
        applyTheme()
        guard !hasInitializedCore else { return }
        guard self.window != nil, let core = core else { return }

        // Init GPU with this view's pointer
        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        let size = self.convertToBacking(bounds).size
        // Defer init if bounds not yet laid out — setFrameSize will retry
        guard size.width > 0 && size.height > 0 else { return }
        let w = UInt32(size.width)
        let h = UInt32(size.height)

        let scale = Float(currentBackingScaleFactor())
        let result = aterm_core_init_gpu(core, viewPtr, w, h, scale)
        if result != 0 {
            NSLog("[aterm] GPU init failed: %d", result)
            return // hasInitializedCore stays false — allows retry when view becomes visible
        }

        hasInitializedCore = true
        lastDrawableSize = size

        // Force PTY resize — layout() may have run before core was initialized,
        // updating lastDrawableSize without calling aterm_core_resize.
        aterm_core_resize(core, w, h)

        // Set dirty callback — immediate render + activity notification (Fix #153).
        // Ghostty pattern: IO wakeup IMMEDIATELY triggers renderer, bypassing
        // CVDisplayLink's ~8ms average latency. try_render is thread-safe and
        // skips if another thread is already rendering.
        let ud = Unmanaged.passUnretained(self).toOpaque()
        aterm_core_set_dirty_callback(core, { userdata in
            guard let userdata = userdata else { return }
            let view = Unmanaged<TerminalView>.fromOpaque(userdata).takeUnretainedValue()
            // Immediate render from PTY thread — 0ms latency vs 8.3ms avg CVDisplayLink
            aterm_core_try_render(view.core)
            DispatchQueue.main.async {
                view.onActivity?()
            }
        }, ud)

        // Start display link for rendering
        startDisplayLink()

        window?.makeFirstResponder(self)

        spawnShellIfNeeded()
    }

    deinit {
        if let core = core {
            aterm_core_stop(core)
        }
        stopDisplayLink()
        if let core = core {
            aterm_core_free(core)
        }
    }

    // MARK: - Responder

    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }

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
            // CVDisplayLink serves as fallback renderer for animations/cursor blink.
            // Primary input-driven render happens in dirty callback (Fix #153).
            // try_render is thread-safe and skips if PTY callback already rendered.
            view.renderFrame()
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
        // try_render atomically: check dirty + acquire render lock + render.
        // Skips if PTY dirty callback already rendered this frame (Fix #153).
        aterm_core_try_render(core)
    }

    // MARK: - Resize

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        guard let core = core else { return }

        // Retry deferred GPU init if viewDidMoveToWindow skipped due to zero bounds
        if !hasInitializedCore && self.window != nil {
            viewDidMoveToWindow()
            guard hasInitializedCore else { return }
        }

        let backingSize = convertToBacking(NSRect(origin: .zero, size: newSize)).size
        guard backingSize != lastDrawableSize else { return }
        lastDrawableSize = backingSize

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        syncMetalLayerBacking()
        let w = UInt32(backingSize.width)
        let h = UInt32(backingSize.height)
        aterm_core_resize(core, w, h)
        CATransaction.commit()

        // No synchronous render — Rust resize sets dirty flag, CVDisplayLink handles render
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        guard hasInitializedCore, let core = core else { return }

        let backingSize = convertToBacking(bounds).size
        guard backingSize != lastDrawableSize else { return }
        lastDrawableSize = backingSize

        CATransaction.begin()
        CATransaction.setDisableActions(true)
        syncMetalLayerBacking()
        CATransaction.commit()

        let w = UInt32(backingSize.width)
        let h = UInt32(backingSize.height)
        aterm_core_resize(core, w, h)
        // No synchronous render — CVDisplayLink handles it via dirty flag
    }

    override func layout() {
        super.layout()
        syncMetalLayerBacking()

        // Unified dedup (Fix 2): shares lastDrawableSize with setFrameSize()
        // to prevent duplicate FFI calls. Fixes first session bottom clipping
        // where PTY spawns with provisional bounds before AppKit layout settles.
        let newSize = convertToBacking(bounds).size
        if newSize.width > 0 && newSize.height > 0 {
            hasObservedLayoutBounds = true
        }
        if newSize != lastDrawableSize && newSize.width > 0 && newSize.height > 0 {
            lastDrawableSize = newSize
            if let core = core {
                aterm_core_resize(core, UInt32(newSize.width), UInt32(newSize.height))
            }
        }

        spawnShellIfNeeded()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyTheme()
    }

    // MARK: - Keyboard Input

    override func keyDown(with event: NSEvent) {
        guard let core = core else { return }
        #if DEBUG
        NSLog("[TV] keyDown keyCode=%d chars=%@ inputContext=%@", event.keyCode, event.characters ?? "nil", String(describing: self.inputContext))
        #endif

        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        // Cmd+Option+Up/Down: scroll to previous/next shell prompt (OSC 133)
        if flags.contains(.command) && flags.contains(.option) {
            switch event.keyCode {
            case 126: // Cmd+Option+Up → scroll to previous prompt
                let markCount = aterm_core_prompt_mark_count(core)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] Cmd+Option+Up pressed, marks=%d", markCount) }
                let scrolled = aterm_core_scroll_to_prompt(core, -1)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] scroll_to_prompt(-1) returned %d", scrolled) }
                if scrolled != 0 {
                    aterm_core_render(core)
                }
                return
            case 125: // Cmd+Option+Down → scroll to next prompt
                let markCount = aterm_core_prompt_mark_count(core)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] Cmd+Option+Down pressed, marks=%d", markCount) }
                let scrolled = aterm_core_scroll_to_prompt(core, 1)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] scroll_to_prompt(1) returned %d", scrolled) }
                if scrolled != 0 {
                    aterm_core_render(core)
                }
                return
            default:
                break
            }
        }

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

        #if DEBUG
        NSLog("[TV] calling interpretKeyEvents")
        #endif
        self.interpretKeyEvents([event])

        // Process accumulated text from insertText calls
        if let acc = keyTextAccumulator, !acc.isEmpty {
            #if DEBUG
            NSLog("[TV] accumulated %d text(s)", acc.count)
            #endif
            for text in acc {
                text.withCString { ptr in
                    aterm_core_write_pty(core, ptr, text.utf8.count)
                }
            }
        } else if !markedTextBefore && markedText.length == 0 {
            // No text accumulated and no marked text — might be a key event
            // that interpretKeyEvents handled via doCommand (special keys).
            // doCommand already handled it, nothing to do here.
            #if DEBUG
            NSLog("[TV] no text accumulated (handled by doCommand or IME)")
            #endif
        }
    }

    // Called by NSTextInputClient when text is committed (Ghostty pattern)
    func insertText(_ string: Any, replacementRange: NSRange) {
        #if DEBUG
        NSLog("[TV] insertText: %@, hasAccumulator=%@", String(describing: string), keyTextAccumulator != nil ? "true" : "false")
        #endif

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
        #if DEBUG
        NSLog("[TV] hasMarkedText: %d", markedText.length)
        #endif
        return markedText.length > 0
    }

    func markedRange() -> NSRange {
        guard markedText.length > 0 else { return NSRange() }
        return NSRange(0...(markedText.length - 1))
    }

    func selectedRange() -> NSRange {
        #if DEBUG
        NSLog("[TV] selectedRange -> {0, 0}")
        #endif
        return NSRange(location: 0, length: 0)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        #if DEBUG
        NSLog("[TV] setMarkedText: %@ selectedRange=(%d,%d)", String(describing: string), selectedRange.location, selectedRange.length)
        #endif
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

    private func gridPoint(for event: NSEvent) -> (col: UInt32, row: Int32)? {
        guard let core = core else { return nil }
        let location = convert(event.locationInWindow, from: nil)
        
        // Convert to backing pixels
        let backingPoint = convertToBacking(NSRect(origin: location, size: .zero)).origin
        let backingSize = convertToBacking(bounds).size
        
        let x = Float(backingPoint.x)
        let y = Float(backingPoint.y)
        
        var cw: Float = 0
        var ch: Float = 0
        aterm_core_cell_size(core, &cw, &ch)
        
        guard cw > 0 && ch > 0 else { return nil }
        
        // Clamp to grid
        let col = max(0, Int32((x - 4.0) / cw))
        let row = max(0, Int32((y - 4.0) / ch))
        
        return (col: UInt32(col), row: row)
    }

    override func mouseDown(with event: NSEvent) {
        guard let core = core else { return }

        // Reset focus
        window?.makeFirstResponder(self)

        // Click to deselect: clear any existing selection immediately
        aterm_core_selection_clear(core)
        isDraggingSelection = false
        dragStartGridPoint = gridPoint(for: event)
        aterm_core_render(core)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let core = core else { return }

        // Start selection on first drag from the mouseDown point
        if !isDraggingSelection, let start = dragStartGridPoint {
            aterm_core_selection_start(core, start.col, start.row, 0)
            isDraggingSelection = true
        }

        if isDraggingSelection, let pt = gridPoint(for: event) {
            aterm_core_selection_update(core, pt.col, pt.row, 0)
            aterm_core_render(core)
        }

        // Auto-scroll when dragging near edges
        lastDragEvent = event
        let location = convert(event.locationInWindow, from: nil)
        let edgeThreshold: CGFloat = 20.0

        if location.y < edgeThreshold {
            // Near top edge (isFlipped: y=0 is top) → scroll back (up)
            startAutoScroll(direction: 1)
        } else if location.y > bounds.height - edgeThreshold {
            // Near bottom edge → scroll down (forward)
            startAutoScroll(direction: -1)
        } else {
            stopAutoScroll()
        }
    }

    override func mouseUp(with event: NSEvent) {
        isDraggingSelection = false
        dragStartGridPoint = nil
        stopAutoScroll()
    }

    private func startAutoScroll(direction: Int32) {
        if autoScrollDirection == direction, autoScrollTimer != nil { return }
        stopAutoScroll()
        autoScrollDirection = direction
        autoScrollTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.performAutoScroll()
        }
    }

    private func stopAutoScroll() {
        autoScrollTimer?.invalidate()
        autoScrollTimer = nil
        autoScrollDirection = 0
        lastDragEvent = nil
    }

    private func performAutoScroll() {
        guard let core = core, isDraggingSelection else {
            stopAutoScroll()
            return
        }
        aterm_core_scroll(core, autoScrollDirection)
        // Update selection endpoint to match the edge row
        if let event = lastDragEvent, let pt = gridPoint(for: event) {
            aterm_core_selection_update(core, pt.col, pt.row, 0)
        }
        aterm_core_render(core)
    }

    // MARK: - Scroll

    override func scrollWheel(with event: NSEvent) {
        guard let core = core else { return }

        if event.hasPreciseScrollingDeltas {
            // Trackpad: accumulate pixel deltas, convert to terminal lines
            scrollAccumulator += event.scrollingDeltaY
            let pixelsPerLine: CGFloat = 3.0
            let lines = Int32(scrollAccumulator / pixelsPerLine)
            if lines != 0 {
                aterm_core_scroll(core, lines)
                scrollAccumulator -= CGFloat(lines) * pixelsPerLine
            }
        } else {
            // Mouse wheel: discrete steps, 3 lines per notch
            let delta = Int32(event.scrollingDeltaY * 3.0)
            if delta != 0 {
                aterm_core_scroll(core, delta)
            }
        }

        // Reset accumulator at gesture boundary
        if event.phase == .ended || event.phase == .cancelled {
            scrollAccumulator = 0.0
        }

        aterm_core_render(core)
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
        guard hasInitializedCore, hasObservedLayoutBounds, window != nil, (!isHidden || backgroundSpawnAllowed) else { return }

        let backingSize = convertToBacking(bounds).size
        guard backingSize.width > 0, backingSize.height > 0 else { return }

        var cols: UInt16 = 0
        var rows: UInt16 = 0
        aterm_core_grid_size(core, Float(backingSize.width), Float(backingSize.height), &cols, &rows)
        guard cols > 2, rows > 1 else { return }

        let cwd = initialWorkingDirectory.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? NSHomeDirectory()
            : initialWorkingDirectory
        let result = workspaceName.withCString { name in
            cwd.withCString { cwd in
                if let cmd = spawnCommand {
                    return cmd.withCString { cmdPtr in
                        aterm_core_spawn_shell(core, name, cwd, cmdPtr, cols, rows)
                    }
                } else {
                    return aterm_core_spawn_shell(core, name, cwd, nil, cols, rows)
                }
            }
        }
        if result == 0 {
            shellSpawned = true
        } else {
            NSLog("[aterm] shell spawn failed for '%@': %d", workspaceName, result)
        }
        onShellSpawned?(result)
    }

    /// Retry GPU init + shell spawn for views that were hidden during initial setup.
    /// Called by AppDelegate.selectWorkspace when a previously-hidden view becomes visible.
    func retrySpawnIfNeeded() {
        guard !shellSpawned else { return }

        if !hasInitializedCore {
            guard self.window != nil, let core = core else { return }
            syncMetalLayerBacking()

            let viewPtr = Unmanaged.passUnretained(self).toOpaque()
            let size = self.convertToBacking(bounds).size
            guard size.width > 0 && size.height > 0 else { return }
            let w = UInt32(size.width)
            let h = UInt32(size.height)
            let scale = Float(currentBackingScaleFactor())
            let result = aterm_core_init_gpu(core, viewPtr, w, h, scale)
            if result != 0 {
                NSLog("[aterm] GPU init retry failed: %d", result)
                return
            }

            hasInitializedCore = true
            lastDrawableSize = size
            aterm_core_resize(core, w, h)

            let ud = Unmanaged.passUnretained(self).toOpaque()
            aterm_core_set_dirty_callback(core, { userdata in
                guard let userdata = userdata else { return }
                let view = Unmanaged<TerminalView>.fromOpaque(userdata).takeUnretainedValue()
                aterm_core_try_render(view.core)
                DispatchQueue.main.async {
                    view.onActivity?()
                }
            }, ud)

            startDisplayLink()
        }

        needsLayout = true
        layoutSubtreeIfNeeded()
        spawnShellIfNeeded()
    }

    /// Pre-spawn PTY for background workspaces to eliminate delay on first switch (#177).
    /// Bypasses the isHidden guard in spawnShellIfNeeded().
    func preSpawnInBackground() {
        guard !shellSpawned else { return }
        backgroundSpawnAllowed = true
        defer { backgroundSpawnAllowed = false }
        retrySpawnIfNeeded()
    }

    private func currentBackingScaleFactor() -> CGFloat {
        return window?.backingScaleFactor
            ?? window?.screen?.backingScaleFactor
            ?? NSScreen.main?.backingScaleFactor
            ?? 2.0
    }

    private func applyTheme() {
        let mode = AtermTheme.mode(for: effectiveAppearance)
        if lastAppliedThemeMode == mode, let metalLayer = layer as? CAMetalLayer {
            metalLayer.backgroundColor = AtermTheme.terminalBackground
                .atermResolvedCGColor(with: effectiveAppearance)
            return
        }

        lastAppliedThemeMode = mode
        if let metalLayer = layer as? CAMetalLayer {
            metalLayer.backgroundColor = AtermTheme.terminalBackground
                .atermResolvedCGColor(with: effectiveAppearance)
        }
        if let core {
            aterm_core_set_theme_mode(core, mode.rawValue)
            if hasInitializedCore {
                aterm_core_render(core)
            }
        }
    }

    private func syncMetalLayerBacking() {
        guard let metalLayer = layer as? CAMetalLayer else { return }
        metalLayer.contentsScale = currentBackingScaleFactor()
        metalLayer.drawableSize = convertToBacking(bounds).size
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
            let pb = NSPasteboard.general

            // Image in clipboard: don't intercept — let CLI read clipboard directly
            if pb.canReadObject(forClasses: [NSImage.self], options: nil),
               pb.string(forType: .string) == nil {
                return true
            }

            // Text paste
            if let text = pb.string(forType: .string) {
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
        #if DEBUG
        NSLog("[TV] doCommand: %@", NSStringFromSelector(selector))
        #endif
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
            if NSApp.currentEvent?.modifierFlags.contains(.shift) == true {
                var cols: UInt16 = 0
                var rows: UInt16 = 0
                let backingSize = convertToBacking(bounds).size
                aterm_core_grid_size(core, Float(backingSize.width), Float(backingSize.height), &cols, &rows)
                aterm_core_scroll(core, Int32(max(rows, 2) - 1))
                aterm_core_render(core)
            } else {
                aterm_core_named_key(core, UInt32(ATERM_KEY_PAGE_UP))
            }
        case #selector(scrollPageDown(_:)):
            if NSApp.currentEvent?.modifierFlags.contains(.shift) == true {
                var cols: UInt16 = 0
                var rows: UInt16 = 0
                let backingSize = convertToBacking(bounds).size
                aterm_core_grid_size(core, Float(backingSize.width), Float(backingSize.height), &cols, &rows)
                aterm_core_scroll(core, -Int32(max(rows, 2) - 1))
                aterm_core_render(core)
            } else {
                aterm_core_named_key(core, UInt32(ATERM_KEY_PAGE_DOWN))
            }
        default:
            // no-op — prevents NSBeep for unhandled selectors
            break
        }
    }
}
