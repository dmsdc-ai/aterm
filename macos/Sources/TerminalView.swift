import AppKit
import QuartzCore
import Metal
import CoreText

// MARK: - GlyphAtlas + GlyphAtlasProtocol conformance

extension GlyphAtlas: GlyphAtlasProtocol {
    var grayscaleTexture: MTLTexture? { texture }
    var colorTexture: MTLTexture? { nil }

    func rasterize(codepoint: UInt32, fontSize: Float, bold: Bool, italic: Bool) -> GlyphRegion? {
        var flags: UInt8 = 0
        if bold { flags |= 1 }
        if italic { flags |= 2 }
        let key = GlyphKey(fontSize: fontSize, codepoint: codepoint, flags: flags)
        let font = GlyphAtlas.makeStyledFont(size: CGFloat(fontSize), bold: bold, italic: italic)
        guard let ar = rect(for: key, font: font) else { return nil }
        guard ar.width > 0 && ar.height > 0 else { return nil }
        return GlyphRegion(
            atlasX: UInt32(ar.x), atlasY: UInt32(ar.y),
            width: UInt32(ar.width), height: UInt32(ar.height),
            bearingX: Int16(ar.bearingX), bearingY: Int16(ar.bearingY),
            atlasType: 0
        )
    }

    func lookup(codepoint: UInt32, fontSize: Float, bold: Bool, italic: Bool) -> GlyphRegion? {
        rasterize(codepoint: codepoint, fontSize: fontSize, bold: bold, italic: italic)
    }

    func beginFrame() { /* LRU tracking managed internally by GlyphAtlas */ }
    func evictIfNeeded() { /* LRU eviction managed internally by GlyphAtlas */ }

    static func makeStyledFont(size: CGFloat, bold: Bool, italic: Bool) -> CTFont {
        // Single source of truth: GlyphAtlas.shared.resolvedFontFamily
        // (set by AppDelegate.applySettingsToView via GlyphAtlas.setFontFamily).
        let family = GlyphAtlas.shared.resolvedFontFamily
        let base = CTFontCreateWithName(family as CFString, size, nil)
        var traits: CTFontSymbolicTraits = []
        if bold { traits.insert(.boldTrait) }
        if italic { traits.insert(.italicTrait) }
        if traits.isEmpty { return base }
        // Q-font-4: if variant missing, log once per family per variant and fall back to base
        guard let styled = CTFontCreateCopyWithSymbolicTraits(base, size, nil, traits, traits) else {
            GlyphAtlas.shared.logMissingVariant(family: family, bold: bold, italic: italic)
            return base
        }
        return styled
    }
}

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
    var onGPUInitFailed: (() -> Void)?
    /// #240: When set, ESC toggles focus to the orchestrator input bar instead of sending to PTY.
    var onEscapeToInputBar: (() -> Void)?
    var workspaceName: String = "main"
    var spawnCommand: String?  // nil = default shell, "claude" = run claude directly
    var initialWorkingDirectory: String = NSHomeDirectory()
    private var displayLink: CVDisplayLink?
    private var markedText = NSMutableAttributedString()
    /// Cursor style driven by IME state: 0=block, 3=underline. Updated on main thread.
    var imeCursorStyle: UInt8 = 0
    private var inputContext_: NSTextInputContext?
    private var hasInitializedCore = false
    private var shellSpawned = false
    private var backgroundSpawnAllowed = false
    private var lastAppliedThemeMode: AtermThemeMode?
    private var isDraggingSelection = false
    private var dragStartGridPoint: (col: UInt32, row: Int32)?

    // Ghostty pattern: set to non-nil during keyDown to accumulate insertText contents
    private var keyTextAccumulator: [String]?

    /// Dirty flag — UI handlers set this, CVDisplayLink renders on next vsync.
    var needsRender = false
    /// Cursor style from settings: "block", "bar", "underline"
    var cursorStyleSetting: String = "block"

    // Resize dedup: skip resize+render if pixel dimensions unchanged (cmux/Ghostty pattern)
    private var lastDrawableSize: CGSize = .zero
    private var hasObservedLayoutBounds = false

    // Scroll: accumulate fractional trackpad deltas before converting to lines
    private var scrollAccumulator: CGFloat = 0.0

    // Auto-scroll during drag selection near edges
    private var autoScrollTimer: Timer?
    private var autoScrollDirection: Int32 = 0  // +1 = up (scroll back), -1 = down
    private var lastDragEvent: NSEvent?

    // Metal renderer for Swift-side rendering (no wgpu dependency)
    private var metalRenderer: MetalRenderer?
    private let renderLock = NSLock()

    // Pre-allocated FFI buffer — avoid per-frame malloc (ghostty pattern)
    private var cellFFIBuffer = [CellDataFFI](repeating: CellDataFFI(), count: 300 * 80)

    // Pre-allocated per-frame buffers — reuse across frames to avoid 7.5GB alloc churn
    private var cellsBg = [UInt8]()
    private var cellsText = [CellData]()

    // Dirty mask from Rust — per-row u8 (1=dirty, 0=clean). Pre-allocated for max rows.
    private var dirtyRowsBuffer = [UInt8](repeating: 1, count: 500)

    // Row-level shaped result cache — skip per-cell atlas lookups for unchanged rows.
    // Key: row index. Value: FNV-1a hash of row's FFI data + cached CellData + bg bytes.
    private struct RowCacheEntry {
        let hash: UInt64
        let cells: [CellData]
        let bg: [UInt8]  // cols * 4 bytes
    }
    private var rowShapeCache: [UInt16: RowCacheEntry] = [:]
    private var lastCacheCols: UInt16 = 0
    private var lastCacheFontSize: Float = 0

    // Font size for atlas rasterization (matches Rust NO_WGPU_FONT_SIZE)
    var currentFontSize: Float = 18.0

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
        self.focusRingType = .none
        wantsLayer = true
        let metalLayer = CAMetalLayer()
        metalLayer.device = MTLCreateSystemDefaultDevice()
        metalLayer.pixelFormat = .bgra8Unorm_srgb
        metalLayer.colorspace = CGColorSpace(name: CGColorSpace.sRGB)!
        metalLayer.framebufferOnly = true
        metalLayer.isOpaque = true
        metalLayer.masksToBounds = true
        metalLayer.contentsScale = NSScreen.main?.backingScaleFactor ?? 2.0
        metalLayer.maximumDrawableCount = 2
        layer = metalLayer
        syncMetalLayerBacking()

        // Init Metal renderer (Swift-side, no wgpu dependency)
        if let metalDevice = metalLayer.device {
            do {
                let renderer = try MetalRenderer(device: metalDevice)
                renderer.glyphAtlas = GlyphAtlas.shared
                self.metalRenderer = renderer
            } catch {
                NSLog("[aterm] MetalRenderer init failed: %@", "\(error)")
            }
        }

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
            DispatchQueue.main.async { [weak self] in
                self?.onGPUInitFailed?()
            }
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
            // Mark dirty — CVDisplayLink renders on next vsync
            view.needsRender = true
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
        metalRenderer?.waitForAllFrames()
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
        guard needsRender else { return }
        needsRender = false
        renderMetalFrame()
    }

    // MARK: - Block Drawing Characters (#228)

    /// Returns sub-rects for block drawing characters (U+2580-U+259F) as cell-relative fractions.
    /// Each tuple: (x_start, y_start, width, height) in 0.0...1.0.
    /// Returns nil for non-block characters.
    private static func blockDrawingRects(_ cp: UInt32) -> [(Float, Float, Float, Float)]? {
        switch cp {
        // Lower block elements (U+2581-U+2587): bottom N/8
        case 0x2581: return [(0, 0.875, 1, 0.125)]
        case 0x2582: return [(0, 0.75,  1, 0.25)]
        case 0x2583: return [(0, 0.625, 1, 0.375)]
        case 0x2584: return [(0, 0.5,   1, 0.5)]
        case 0x2585: return [(0, 0.375, 1, 0.625)]
        case 0x2586: return [(0, 0.25,  1, 0.75)]
        case 0x2587: return [(0, 0.125, 1, 0.875)]
        // Full block + upper half
        case 0x2580: return [(0, 0, 1, 0.5)]
        case 0x2588: return [(0, 0, 1, 1)]
        // Left block elements (U+2589-U+258F): left N/8
        case 0x2589: return [(0, 0, 0.875, 1)]
        case 0x258A: return [(0, 0, 0.75,  1)]
        case 0x258B: return [(0, 0, 0.625, 1)]
        case 0x258C: return [(0, 0, 0.5,   1)]
        case 0x258D: return [(0, 0, 0.375, 1)]
        case 0x258E: return [(0, 0, 0.25,  1)]
        case 0x258F: return [(0, 0, 0.125, 1)]
        // Right half
        case 0x2590: return [(0.5, 0, 0.5, 1)]
        // Shade characters — full cell, alpha handled by caller
        case 0x2591: return [(0, 0, 1, 1)]
        case 0x2592: return [(0, 0, 1, 1)]
        case 0x2593: return [(0, 0, 1, 1)]
        // Upper/right 1/8
        case 0x2594: return [(0, 0, 1, 0.125)]
        case 0x2595: return [(0.875, 0, 0.125, 1)]
        // Quadrant elements (U+2596-U+259F)
        case 0x2596: return [(0, 0.5, 0.5, 0.5)]                                    // ▖ lower left
        case 0x2597: return [(0.5, 0.5, 0.5, 0.5)]                                  // ▗ lower right
        case 0x2598: return [(0, 0, 0.5, 0.5)]                                      // ▘ upper left
        case 0x2599: return [(0, 0, 0.5, 1), (0.5, 0.5, 0.5, 0.5)]                 // ▙ left half + LR
        case 0x259A: return [(0, 0, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5)]               // ▚ UL + LR
        case 0x259B: return [(0, 0, 1, 0.5), (0, 0.5, 0.5, 0.5)]                   // ▛ upper half + LL
        case 0x259C: return [(0, 0, 1, 0.5), (0.5, 0.5, 0.5, 0.5)]                 // ▜ upper half + LR
        case 0x259D: return [(0.5, 0, 0.5, 0.5)]                                    // ▝ upper right
        case 0x259E: return [(0.5, 0, 0.5, 0.5), (0, 0.5, 0.5, 0.5)]               // ▞ UR + LL
        case 0x259F: return [(0.5, 0, 0.5, 0.5), (0, 0.5, 1, 0.5)]                 // ▟ UR + lower half
        default: return nil
        }
    }

    // MARK: - Metal Render Path (#209/#213)

    /// Direct Metal render path — bypasses wgpu entirely.
    /// CVDisplayLink / dirty-callback → get_render_data (Rust FFI, lock-free after copy)
    /// → rasterize glyphs via GlyphAtlas → MetalRenderer.drawFrame()
    func renderMetalFrame() {
        guard renderLock.try() else { return }  // skip frame if already rendering
        defer { renderLock.unlock() }
        autoreleasepool {
        guard hasInitializedCore,
              let core = core,
              let renderer = metalRenderer,
              let metalLayer = self.layer as? CAMetalLayer,
              let drawable = metalLayer.nextDrawable()
        else { return }

        // STEP 1: Fetch cell data from Rust (FairMutex lock → copy → unlock)
        var count: UInt32 = 0
        var cols: UInt16 = 0
        var rows: UInt16 = 0
        let maxCells = UInt32(cellFFIBuffer.count)
        // Ensure dirty mask buffer is large enough
        if dirtyRowsBuffer.count < 500 {
            dirtyRowsBuffer = [UInt8](repeating: 1, count: 500)
        }

        let result = cellFFIBuffer.withUnsafeMutableBufferPointer { cellPtr in
            dirtyRowsBuffer.withUnsafeMutableBufferPointer { dirtyPtr in
                aterm_core_get_render_data(core, cellPtr.baseAddress!, maxCells, &count, &cols, &rows, dirtyPtr.baseAddress!)
            }
        }
        guard result == 0 else { return }

        let atlas = GlyphAtlas.shared

        // STEP 2: Convert CellDataFFI → CellData + build cellsBg flat array
        let totalCells = Int(cols) * Int(rows)
        let bgSize = totalCells * 4
        if cellsBg.count != bgSize {
            // Grid size changed — full realloc, all rows dirty
            cellsBg = [UInt8](repeating: 0, count: bgSize)
        }
        // Don't memset clean rows — preserve previous frame data (#231).
        cellsText.removeAll(keepingCapacity: true)

        atlas.beginFrame()

        // Invalidate row shape cache on font/grid change
        if cols != lastCacheCols || currentFontSize != lastCacheFontSize {
            rowShapeCache.removeAll(keepingCapacity: true)
            lastCacheCols = cols
            lastCacheFontSize = currentFontSize
        }

        // Row-level dirty tracking (#231): Rust emits only dirty rows' cells.
        // Clean rows → use cached cellsBg + cellsText from previous frame.
        // Dirty rows → process FFI cells sequentially, update cache.
        let colCount = Int(cols)
        let rowCount = Int(rows)
        var ffiIdx = 0

        for rowIdx in 0..<rowCount {
            // Clean row — use cached data, skip atlas lookups entirely
            if dirtyRowsBuffer[rowIdx] == 0 {
                if let cached = rowShapeCache[UInt16(rowIdx)] {
                    let bgDst = rowIdx * colCount * 4
                    let bgLen = colCount * 4
                    if bgDst + bgLen <= cellsBg.count, cached.bg.count == bgLen {
                        for j in 0..<bgLen { cellsBg[bgDst + j] = cached.bg[j] }
                    }
                    cellsText.append(contentsOf: cached.cells)
                }
                continue
            }

            // Dirty row — process cells from FFI buffer (Rust emits only dirty rows)
            var rowCells: [CellData] = []
            let bgLen = colCount * 4
            var rowBg = [UInt8](repeating: 0, count: bgLen)

            for colIdx in 0..<colCount {
                guard ffiIdx < Int(count) else { break }
                let ffi = cellFFIBuffer[ffiIdx]
                ffiIdx += 1

                // Fill background
                let bgOff = colIdx * 4
                rowBg[bgOff]     = ffi.bg_r
                rowBg[bgOff + 1] = ffi.bg_g
                rowBg[bgOff + 2] = ffi.bg_b
                rowBg[bgOff + 3] = ffi.bg_a

                let mainBgIdx = (rowIdx * colCount + colIdx) * 4
                if mainBgIdx + 3 < cellsBg.count {
                    cellsBg[mainBgIdx]     = ffi.bg_r
                    cellsBg[mainBgIdx + 1] = ffi.bg_g
                    cellsBg[mainBgIdx + 2] = ffi.bg_b
                    cellsBg[mainBgIdx + 3] = ffi.bg_a
                }

                // Skip space / NUL / NBSP
                guard ffi.character > 0x20, ffi.character != 0xA0 else { continue }

                // Block Drawing Characters (U+2580-U+259F) — pixel-perfect rects
                if let rects = Self.blockDrawingRects(ffi.character) {
                    var alpha = ffi.fg_a
                    if ffi.character == 0x2591 { alpha = UInt8(Float(ffi.fg_a) * 0.25) }
                    else if ffi.character == 0x2592 { alpha = UInt8(Float(ffi.fg_a) * 0.50) }
                    else if ffi.character == 0x2593 { alpha = UInt8(Float(ffi.fg_a) * 0.75) }
                    for rect in rects {
                        rowCells.append(CellData(
                            glyphPos:  (0, 0),
                            glyphSize: (UInt32(rect.2 * 256), UInt32(rect.3 * 256)),
                            bearings:  (Int16(rect.0 * 256), Int16(rect.1 * 256)),
                            gridPos:   (ffi.col, ffi.row),
                            fgColor:   (ffi.fg_r, ffi.fg_g, ffi.fg_b, alpha),
                            atlasType: 2,
                            cellFlags: ffi.flags,
                            _pad:      (0, 0)
                        ))
                    }
                    continue
                }

                let bold   = (ffi.flags & 1) != 0
                let italic = (ffi.flags & 2) != 0
                guard let region = atlas.rasterize(
                    codepoint: ffi.character,
                    fontSize: currentFontSize,
                    bold: bold,
                    italic: italic
                ) else {
                    continue
                }

                rowCells.append(CellData(
                    glyphPos:  (region.atlasX, region.atlasY),
                    glyphSize: (region.width, region.height),
                    bearings:  (region.bearingX, region.bearingY),
                    gridPos:   (ffi.col, ffi.row),
                    fgColor:   (ffi.fg_r, ffi.fg_g, ffi.fg_b, ffi.fg_a),
                    atlasType: region.atlasType,
                    cellFlags: ffi.flags,
                    _pad:      (0, 0)
                ))
            }

            // Store in cache for future clean-row hits
            rowShapeCache[UInt16(rowIdx)] = RowCacheEntry(hash: 0, cells: rowCells, bg: rowBg)
            cellsText.append(contentsOf: rowCells)
        }

        atlas.evictIfNeeded()

        // STEP 3: Get geometry from Rust
        var cellW: Float = 0, cellH: Float = 0
        aterm_core_cell_size(core, &cellW, &cellH)
        renderer.setCellSize(width: cellW, height: cellH)

        var padX: Float = 0, padY: Float = 0
        let backingSize = convertToBacking(bounds).size
        aterm_core_grid_padding(core, Float(backingSize.width), Float(backingSize.height), &padX, &padY)

        var cursorX: Float = 0, cursorY: Float = 0
        aterm_core_cursor_position(core, Float(backingSize.width), Float(backingSize.height), &cursorX, &cursorY)

        // Detect preedit from markedText directly (more reliable than imeCursorStyle flag)
        let isPreedit = markedText.length > 0

        // Suppress INVERSE cursor cell bg during preedit (Rust renders cursor as inverse cell)
        // Clear ALL preedit cells — Korean chars are 2 cells wide
        if isPreedit {
            let cursorCol = Int((cursorX - padX) / cellW)
            let cursorRow = Int((cursorY - padY) / cellH)
            let preeditCellCount = markedText.string.unicodeScalars.count * 2  // wide chars = 2 cells
            for ci in 0..<max(1, preeditCellCount) {
                let idx = (cursorRow * Int(cols) + cursorCol + ci) * 4
                if idx >= 0 && idx + 3 < cellsBg.count {
                    cellsBg[idx + 3] = 0  // transparent — hide inverse cursor block
                }
            }
        }

        // STEP 3.5: Preedit (IME composition) — render inline at cursor position (Ghostty pattern)
        // When user types Korean (ㅎ→하→한), each intermediate state renders at cursor cell.
        // Preedit chars get underline decoration (bit 2 = 0x04). Normal cursor hidden during preedit.
        if isPreedit {
            let preeditStr = markedText.string
            let cursorCol = UInt16((cursorX - padX) / cellW)
            let cursorRow = UInt16((cursorY - padY) / cellH)
            var preeditOffset: UInt16 = 0
            for scalar in preeditStr.unicodeScalars {
                let cp = scalar.value
                guard cp > 0x20 else { preeditOffset += 1; continue }
                if let region = atlas.rasterize(
                    codepoint: cp,
                    fontSize: currentFontSize,
                    bold: false,
                    italic: false
                ) {
                    cellsText.append(CellData(
                        glyphPos:  (region.atlasX, region.atlasY),
                        glyphSize: (region.width, region.height),
                        bearings:  (region.bearingX, region.bearingY),
                        gridPos:   (cursorCol + preeditOffset, cursorRow),
                        fgColor:   renderer.cursorColor,
                        atlasType: region.atlasType,
                        cellFlags: 0x04,  // underline decoration
                        _pad:      (0, 0)
                    ))
                }
                preeditOffset += 1
            }
        }

        // Build cursor rect — style from settings, nil during preedit
        let cursor: CursorRect?
        if isPreedit {
            cursor = nil
        } else {
            let cPos: SIMD2<Float>
            let cSize: SIMD2<Float>
            let cStyle: UInt8
            switch cursorStyleSetting {
            case "bar":
                cStyle = 2
                cPos = SIMD2<Float>(cursorX, cursorY)
                cSize = SIMD2<Float>(2.0 * Float(renderer.contentsScale), cellH)
            case "underline":
                cStyle = 3
                cPos = SIMD2<Float>(cursorX, cursorY + cellH - 2.0)
                cSize = SIMD2<Float>(cellW, 2.0)
            default: // "block"
                cStyle = 0
                cPos = SIMD2<Float>(cursorX, cursorY)
                cSize = SIMD2<Float>(cellW, cellH)
            }
            cursor = CursorRect(
                pos: cPos, size: cSize,
                color: renderer.cursorColor,
                style: cStyle,
                _pad: (0, 0, 0)
            )
        }

        // STEP 3.7: Selection ranges from Rust
        var selRanges: [SelectionRange] = []
        var selCount: UInt32 = 0
        let maxSel = UInt32(rows)
        var selBuffer = [SelectionRangeFFI](repeating: SelectionRangeFFI(), count: Int(maxSel))
        aterm_core_selection_ranges(core, &selBuffer, maxSel, &selCount)
        if selCount > 0 {
            selRanges.reserveCapacity(Int(selCount))
            for i in 0..<Int(selCount) {
                let s = selBuffer[i]
                selRanges.append(SelectionRange(
                    start: (s.start_col, s.start_row),
                    end: (s.end_col, s.end_row),
                    color: (s.r, s.g, s.b, s.a)
                ))
            }
        }

        // STEP 4: GPU draw — fully lock-free from here
        renderer.drawFrame(
            cells: cellsText,
            cellsBg: cellsBg,
            gridSize: (cols: Int(cols), rows: Int(rows)),
            screenSize: (width: Float(backingSize.width), height: Float(backingSize.height)),
            gridPadding: (top: padY, right: padX, bottom: padY, left: padX),
            cursorRect: cursor,
            selectionRanges: selRanges,
            drawable: drawable
        )
        } // autoreleasepool
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
                    needsRender = true
                }
                return
            case 125: // Cmd+Option+Down → scroll to next prompt
                let markCount = aterm_core_prompt_mark_count(core)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] Cmd+Option+Down pressed, marks=%d", markCount) }
                let scrolled = aterm_core_scroll_to_prompt(core, 1)
                if ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil { NSLog("[scroll-to-prompt] scroll_to_prompt(1) returned %d", scrolled) }
                if scrolled != 0 {
                    needsRender = true
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
        imeCursorStyle = 0  // block after commit
        if let core = core { aterm_core_set_preedit_active(core, false) }
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
            imeCursorStyle = 1  // preedit active
            if let core = core { aterm_core_set_preedit_active(core, true) }
            window?.title = "aterm v3 [\(markedText.string)]"
        } else {
            imeCursorStyle = 0  // preedit done
            if let core = core { aterm_core_set_preedit_active(core, false) }
            window?.title = "aterm v3"
        }
        renderMetalFrame()  // immediate render — preedit cannot wait for vsync
    }

    func unmarkText() {
        markedText.mutableString.setString("")
        imeCursorStyle = 0  // block when done
        if let core = core { aterm_core_set_preedit_active(core, false) }
        window?.title = "aterm v3"
        renderMetalFrame()  // immediate render
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

        // Convert to backing pixels — bypass convertToBacking (CAMetalLayer transform contaminates all overloads)
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2.0
        let x = Float(location.x * scale)
        let y = Float(location.y * scale)
        // Use consistent scale for backing size (not convertToBacking which is CAMetalLayer-contaminated)
        let backingW = Float(bounds.width * scale)
        let backingH = Float(bounds.height * scale)

        var cw: Float = 0
        var ch: Float = 0
        aterm_core_cell_size(core, &cw, &ch)

        guard cw > 0 && ch > 0 else { return nil }

        // Get actual grid padding from renderer — must use same backing size as mouse coords
        var padX: Float = 0
        var padY: Float = 0
        aterm_core_grid_padding(core, backingW, backingH, &padX, &padY)

        // Clamp to grid
        let col = max(0, Int32((x - padX) / cw))
        let row = max(0, Int32((y - padY) / ch))

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
        if let pt = dragStartGridPoint {
            NSLog("[selection-debug] mouseDown → col=%d row=%d", pt.col, pt.row)
        }
        needsRender = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let core = core else { return }

        // Start selection on first drag from the mouseDown point
        if !isDraggingSelection, let start = dragStartGridPoint {
            aterm_core_selection_start(core, start.col, start.row, 0)
            isDraggingSelection = true
        }

        if isDraggingSelection, let pt = gridPoint(for: event) {
            NSLog("[selection-debug] mouseDragged → col=%d row=%d", pt.col, pt.row)
            aterm_core_selection_update(core, pt.col, pt.row, 0)
            needsRender = true
        }

        // Auto-scroll when dragging near edges
        lastDragEvent = event
        let location = convert(event.locationInWindow, from: nil)
        let edgeThreshold: CGFloat = 5.0

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
        autoScrollTimer = Timer.scheduledTimer(withTimeInterval: 0.015, repeats: true) { [weak self] _ in
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
        // Calculate scroll speed proportional to distance from edge (Alacritty: delta / 20px step)
        var scrollLines = autoScrollDirection  // base: 1 line
        if let event = lastDragEvent {
            let location = convert(event.locationInWindow, from: nil)
            let scrollStep: CGFloat = 20.0  // Alacritty SELECTION_SCROLLING_STEP
            if autoScrollDirection > 0 {
                // Scrolling up — mouse is near top (y < 5)
                let overshoot = max(0, 5.0 - location.y)
                scrollLines = max(1, Int32(overshoot / scrollStep) + 1)
            } else {
                // Scrolling down — mouse is near bottom
                let overshoot = max(0, location.y - (bounds.height - 5.0))
                scrollLines = -max(1, Int32(overshoot / scrollStep) + 1)
            }
        }
        aterm_core_scroll(core, scrollLines)
        // Update selection endpoint to match the edge row
        if let event = lastDragEvent, let pt = gridPoint(for: event) {
            aterm_core_selection_update(core, pt.col, pt.row, 0)
        }
        needsRender = true
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

        needsRender = true
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
                view.needsRender = true
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
            syncRendererBgColor()
            return
        }

        lastAppliedThemeMode = mode
        if let metalLayer = layer as? CAMetalLayer {
            metalLayer.backgroundColor = AtermTheme.terminalBackground
                .atermResolvedCGColor(with: effectiveAppearance)
            syncRendererBgColor()
        }
        if let core {
            aterm_core_set_theme_mode(core, mode.rawValue)
            if hasInitializedCore {
                needsRender = true
            }
        }
    }

    private func syncRendererBgColor() {
        let c = AtermTheme.terminalBackground
        var r: UInt8 = 0x1A; var g: UInt8 = 0x1B; var b: UInt8 = 0x26
        effectiveAppearance.performAsCurrentDrawingAppearance {
            if let srgb = c.usingColorSpace(.sRGB) {
                r = UInt8(max(0, min(255, srgb.redComponent * 255)))
                g = UInt8(max(0, min(255, srgb.greenComponent * 255)))
                b = UInt8(max(0, min(255, srgb.blueComponent * 255)))
            }
        }
        metalRenderer?.setBgColor(r: r, g: g, b: b)
    }

    /// Update MetalRenderer bg color from the Rust color scheme palette.
    func applySchemeBackground(_ scheme: UInt8) {
        var r: UInt8 = 0; var g: UInt8 = 0; var b: UInt8 = 0
        aterm_core_scheme_bg_color(scheme, &r, &g, &b)
        metalRenderer?.setBgColor(r: r, g: g, b: b)
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
                    needsRender = true
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
            if let handler = onEscapeToInputBar {
                handler()
                return
            }
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
                needsRender = true
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
                needsRender = true
            } else {
                aterm_core_named_key(core, UInt32(ATERM_KEY_PAGE_DOWN))
            }
        default:
            // no-op — prevents NSBeep for unhandled selectors
            break
        }
    }

    // MARK: - Context Menu

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()

        // Copy — only if there's a selection
        if let core = core, let textPtr = aterm_core_selection_text(core) {
            let text = String(cString: textPtr)
            aterm_core_free_string(textPtr)
            if !text.isEmpty {
                let copyItem = NSMenuItem(title: "Copy", action: #selector(contextCopy(_:)), keyEquivalent: "c")
                copyItem.keyEquivalentModifierMask = .command
                menu.addItem(copyItem)
            }
        }

        // Paste
        let pasteItem = NSMenuItem(title: "Paste", action: #selector(contextPaste(_:)), keyEquivalent: "v")
        pasteItem.keyEquivalentModifierMask = .command
        menu.addItem(pasteItem)

        menu.addItem(NSMenuItem.separator())

        // Select All
        let selectAllItem = NSMenuItem(title: "Select All", action: #selector(contextSelectAll(_:)), keyEquivalent: "a")
        selectAllItem.keyEquivalentModifierMask = .command
        menu.addItem(selectAllItem)

        return menu
    }

    @objc private func contextCopy(_ sender: Any?) {
        guard let core = core else { return }
        if let textPtr = aterm_core_selection_text(core) {
            let text = String(cString: textPtr)
            aterm_core_free_string(textPtr)
            if !text.isEmpty {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
                aterm_core_selection_clear(core)
                needsRender = true
            }
        }
    }

    @objc private func contextPaste(_ sender: Any?) {
        guard let core = core else { return }
        if let text = NSPasteboard.general.string(forType: .string) {
            text.withCString { ptr in
                aterm_core_write_pty(core, ptr, text.utf8.count)
            }
        }
    }

    @objc private func contextSelectAll(_ sender: Any?) {
        guard let core = core else { return }
        aterm_core_select_all(core)
        needsRender = true
    }
}
