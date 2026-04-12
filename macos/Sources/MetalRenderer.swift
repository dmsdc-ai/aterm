import Metal
import simd
import QuartzCore
import os.log

// MARK: - GlyphAtlasProtocol

/// Protocol for glyph atlas — concrete implementation in GlyphAtlas.swift.
/// MetalRenderer depends only on this protocol so the atlas can be swapped
/// or stubbed for testing.
protocol GlyphAtlasProtocol: AnyObject {
    /// The grayscale atlas texture (r8Unorm, for regular text glyphs).
    var grayscaleTexture: MTLTexture? { get }
    /// The color atlas texture (bgra8Unorm, for emoji/color glyphs).
    var colorTexture: MTLTexture? { get }

    /// Rasterize a glyph and return its atlas region. Returns nil if atlas is full.
    func rasterize(codepoint: UInt32, fontSize: Float, bold: Bool, italic: Bool) -> GlyphRegion?

    /// Check if a glyph is already cached.
    func lookup(codepoint: UInt32, fontSize: Float, bold: Bool, italic: Bool) -> GlyphRegion?

    /// Mark frame boundary for LRU tracking.
    func beginFrame()

    /// Evict stale glyphs if needed.
    func evictIfNeeded()
}

// MARK: - GlyphRegion

/// Position and metrics of a glyph inside the atlas texture.
struct GlyphRegion {
    var atlasX: UInt32        // pixel X in atlas
    var atlasY: UInt32        // pixel Y in atlas
    var width: UInt32         // glyph pixel width
    var height: UInt32        // glyph pixel height
    var bearingX: Int16       // horizontal bearing
    var bearingY: Int16       // vertical bearing (from baseline)
    var atlasType: UInt8      // 0 = grayscale, 1 = color
}

// MARK: - CellData (32 bytes — matches CellText in Metal)

/// Per-glyph instance data uploaded to the cell_text pipeline.
/// Layout must match `CellText` in Shaders.metal exactly.
struct CellData {
    var glyphPos: (UInt32, UInt32)                   // 8 bytes — atlas pixel position
    var glyphSize: (UInt32, UInt32)                   // 8 bytes — glyph pixel size
    var bearings: (Int16, Int16)                      // 4 bytes — glyph bearings
    var gridPos: (UInt16, UInt16)                     // 4 bytes — grid column, row
    var fgColor: (UInt8, UInt8, UInt8, UInt8)         // 4 bytes RGBA
    var atlasType: UInt8                              // 1 byte — 0=grayscale, 1=color
    var cellFlags: UInt8                              // 1 byte — bit flags
    var _pad: (UInt8, UInt8)                          // 2 bytes padding
    // Total: 32 bytes
}

// MARK: - Uniforms (144 bytes — matches Metal side)

/// Uniform buffer shared across all pipeline passes.
/// Byte layout must match the Metal `Uniforms` struct exactly.
///
/// ```
/// offset   0: float4x4 projectionMatrix  (64 bytes)
/// offset  64: float2   screenSize         (8 bytes)
/// offset  72: float2   cellSize           (8 bytes)
/// offset  80: uint2    gridSize           (8 bytes)
/// offset  88: float2   _pad0              (8 bytes, align float4 at 96)
/// offset  96: float4   gridPadding        (16 bytes)
/// offset 112: float2   cursorPos          (8 bytes)
/// offset 120: uchar4   cursorColor        (4 bytes)
/// offset 124: uchar4   bgColor            (4 bytes)
/// offset 128: uint     flags              (4 bytes)
/// offset 132: float    scale              (4 bytes, display contentsScale)
/// offset 136: float2   _pad1              (8 bytes, pad to 144)
/// ```
struct Uniforms {
    var projectionMatrix: simd_float4x4 = matrix_identity_float4x4 // 64
    var screenSize: SIMD2<Float> = .zero                            // 8
    var cellSize: SIMD2<Float> = .zero                              // 8
    var gridSize: SIMD2<UInt32> = .zero                             // 8
    var _pad0: SIMD2<Float> = .zero                                 // 8  (align)
    var gridPadding: SIMD4<Float> = .zero                           // 16
    var cursorPos: SIMD2<Float> = .zero                             // 8
    var cursorColor: (UInt8, UInt8, UInt8, UInt8) = (0, 0, 0, 0)    // 4
    var bgColor: (UInt8, UInt8, UInt8, UInt8) = (0, 0, 0, 0)        // 4
    var flags: UInt32 = 0                                            // 4
    var scale: Float = 2.0                                           // 4  (display contentsScale for Retina glyph sizing)
    var minContrast: Float = 1.0                                     // 4  (WCAG 2.0 min contrast ratio, 1.0=disabled)
    var _pad1: Float = 0                                             // 4  (pad to 144)
    // Total: 144 bytes
}

// MARK: - CursorRect (matches Metal side)

/// Cursor geometry and appearance for the cursor pipeline pass.
struct CursorRect {
    var pos: SIMD2<Float>                             // 8 bytes
    var size: SIMD2<Float>                            // 8 bytes
    var color: (UInt8, UInt8, UInt8, UInt8)            // 4 bytes
    var style: UInt8                                   // 0=block, 1=underline, 2=bar
    var _pad: (UInt8, UInt8, UInt8)                    // 3 bytes
    // Total: 24 bytes
}

// MARK: - SelectionRange (matches Metal side)

/// Selection highlight range for the selection overlay pipeline.
struct SelectionRange {
    var start: (UInt16, UInt16)                        // col, row — 4 bytes
    var end: (UInt16, UInt16)                          // col, row — 4 bytes
    var color: (UInt8, UInt8, UInt8, UInt8)            // RGBA — 4 bytes
    // Total: 12 bytes
}

// MARK: - FrameState (triple buffering)

/// Per-frame Metal buffer set. Three instances rotate via semaphore
/// to allow CPU preparation of frame N+1 while GPU renders frame N.
struct FrameState {
    var uniformsBuffer: MTLBuffer
    var cellsBgBuffer: MTLBuffer       // uchar4 per cell
    var cellsTextBuffer: MTLBuffer     // CellData per glyph instance
    var cellsBgCount: Int = 0
    var cellsTextCount: Int = 0
}

// MARK: - MetalRendererError

enum MetalRendererError: Error, CustomStringConvertible {
    case noCommandQueue
    case noLibrary(String)
    case noFunction(String)
    case pipelineCreation(String)

    var description: String {
        switch self {
        case .noCommandQueue:
            return "Failed to create Metal command queue"
        case .noLibrary(let detail):
            return "Failed to load Metal shader library: \(detail)"
        case .noFunction(let name):
            return "Metal function '\(name)' not found in library"
        case .pipelineCreation(let detail):
            return "Failed to create render pipeline: \(detail)"
        }
    }
}

// MARK: - MetalRenderer

/// GPU renderer for the aterm terminal emulator.
///
/// Architecture modeled on ghostty's Metal renderer:
/// - 5 render pipeline passes per frame (bg_color, cell_bg, cell_text, cursor, selection)
/// - Triple buffering with semaphore-gated frame rotation
/// - Orthographic projection with top-left origin
/// - All data uploaded via shared-mode MTLBuffers (Apple Silicon unified memory)
final class MetalRenderer {

    // MARK: - Constants

    /// Initial capacity for uniform buffer (single Uniforms struct).
    private static let uniformsBufferSize = 256

    /// Initial cell background buffer: 200 cols * 50 rows * 4 bytes (RGBA).
    private static let initialCellBgSize = 200 * 50 * 4

    /// Initial cell text buffer: 200 cols * 50 rows * 32 bytes (CellData).
    private static let initialCellTextSize = 200 * 50 * MemoryLayout<CellData>.stride

    private static let log = OSLog(subsystem: "com.aigentry.aterm", category: "MetalRenderer")

    // MARK: - Metal objects

    private let device: MTLDevice
    private let commandQueue: MTLCommandQueue
    private let library: MTLLibrary

    // 5 pipeline states — one per render pass
    private let bgColorPipeline: MTLRenderPipelineState
    private let cellBgPipeline: MTLRenderPipelineState
    private let cellTextPipeline: MTLRenderPipelineState
    private let cursorPipeline: MTLRenderPipelineState
    private let selectionPipeline: MTLRenderPipelineState

    // MARK: - Triple buffering

    private var frames: [FrameState]
    private var frameIndex: Int = 0
    private let maxFramesInFlight = 3
    private let frameSemaphore: DispatchSemaphore

    // MARK: - Atlas (injected)

    /// Glyph atlas — concrete implementation lives in GlyphAtlas.swift.
    /// Weak reference: renderer does not own the atlas lifetime.
    weak var glyphAtlas: (any GlyphAtlasProtocol)?

    // MARK: - Configurable state

    /// Cell dimensions in pixels (width, height). Default: 10.8 x 26.0
    /// for an 18px font at 0.6 advance ratio with 26px line height.
    var cellSize: SIMD2<Float> = SIMD2<Float>(10.8, 21.0)

    /// Terminal background color (Default scheme = xterm black).
    var bgColor: (UInt8, UInt8, UInt8, UInt8) = (0x28, 0x2C, 0x34, 0xFF)

    /// Cursor color (Tokyo Night foreground default).
    var cursorColor: (UInt8, UInt8, UInt8, UInt8) = (0xC0, 0xCA, 0xF5, 0xFF)

    /// Display scale factor for Retina. Atlas glyphs are rasterized at physical
    /// pixels; the shader divides by this to get logical quad size.
    var contentsScale: Float = 2.0

    /// When true, suppress rendering (DEC private mode 2026).
    var isSynchronizedOutput: Bool = false

    // MARK: - Init

    /// Create a MetalRenderer for the given device.
    ///
    /// Compiles all 5 render pipelines from the default Metal library.
    /// Throws if any pipeline fails to compile.
    init(device: MTLDevice) throws {
        self.device = device

        guard let queue = device.makeCommandQueue() else {
            throw MetalRendererError.noCommandQueue
        }
        self.commandQueue = queue
        queue.label = "com.aigentry.aterm.render"

        // Load shader library — Shaders.metal compiled into default library
        guard let lib = device.makeDefaultLibrary() else {
            throw MetalRendererError.noLibrary("makeDefaultLibrary() returned nil")
        }
        self.library = lib

        // DIAGNOSTIC #209: verify CellData layout matches Metal CellText (32 bytes)
        do {
            let size = MemoryLayout<CellData>.size
            let stride = MemoryLayout<CellData>.stride
            let align = MemoryLayout<CellData>.alignment
            NSLog("[METAL-LAYOUT] CellData size=%d stride=%d align=%d", size, stride, align)
            NSLog("[METAL-LAYOUT] field offsets: glyphPos=%d glyphSize=%d bearings=%d gridPos=%d fgColor=%d atlasType=%d cellFlags=%d _pad=%d",
                  MemoryLayout<CellData>.offset(of: \CellData.glyphPos) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.glyphSize) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.bearings) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.gridPos) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.fgColor) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.atlasType) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData.cellFlags) ?? -1,
                  MemoryLayout<CellData>.offset(of: \CellData._pad) ?? -1)
            // Expected: size=32 stride=32 align=4
            // Expected offsets: 0, 8, 16, 20, 24, 28, 29, 30
            // Metal CellText (device addr space): uint2(0), uint2(8), short2(16), ushort2(20), uchar4(24), uchar(28), uchar(29), uchar2(30)
            if size != 32 || stride != 32 {
                NSLog("[METAL-LAYOUT] ⚠️ MISMATCH — CellData is NOT 32 bytes! Metal CellText expects 32.")
            }
        }

        // Build all 5 pipelines
        self.bgColorPipeline = try MetalRenderer.makeBgColorPipeline(device: device, library: lib)
        self.cellBgPipeline = try MetalRenderer.makeCellBgPipeline(device: device, library: lib)
        self.cellTextPipeline = try MetalRenderer.makeCellTextPipeline(device: device, library: lib)
        self.cursorPipeline = try MetalRenderer.makeCursorPipeline(device: device, library: lib)
        self.selectionPipeline = try MetalRenderer.makeSelectionPipeline(device: device, library: lib)

        // Triple buffer semaphore
        self.frameSemaphore = DispatchSemaphore(value: maxFramesInFlight)

        // Allocate 3 frame states
        var frameArray: [FrameState] = []
        frameArray.reserveCapacity(maxFramesInFlight)
        for i in 0..<maxFramesInFlight {
            let uniformsBuf = device.makeBuffer(
                length: MetalRenderer.uniformsBufferSize,
                options: .storageModeShared
            )!
            uniformsBuf.label = "uniforms-\(i)"

            let cellsBgBuf = device.makeBuffer(
                length: MetalRenderer.initialCellBgSize,
                options: .storageModeShared
            )!
            cellsBgBuf.label = "cellsBg-\(i)"

            let cellsTextBuf = device.makeBuffer(
                length: MetalRenderer.initialCellTextSize,
                options: .storageModeShared
            )!
            cellsTextBuf.label = "cellsText-\(i)"

            frameArray.append(FrameState(
                uniformsBuffer: uniformsBuf,
                cellsBgBuffer: cellsBgBuf,
                cellsTextBuffer: cellsTextBuf
            ))
        }
        self.frames = frameArray
    }

    // MARK: - Pipeline Creation

    /// Pass 1: fullscreen background clear — vertex-only fullscreen triangle.
    /// No blending needed (opaque background).
    private static func makeBgColorPipeline(
        device: MTLDevice, library: MTLLibrary
    ) throws -> MTLRenderPipelineState {
        guard let vertexFn = library.makeFunction(name: "bg_color_vertex") else {
            throw MetalRendererError.noFunction("bg_color_vertex")
        }
        guard let fragmentFn = library.makeFunction(name: "bg_color_fragment") else {
            throw MetalRendererError.noFunction("bg_color_fragment")
        }

        let desc = MTLRenderPipelineDescriptor()
        desc.label = "bg_color"
        desc.vertexFunction = vertexFn
        desc.fragmentFunction = fragmentFn
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        // No blending — opaque background fill
        desc.colorAttachments[0].isBlendingEnabled = false

        return try device.makeRenderPipelineState(descriptor: desc)
    }

    /// Pass 2: per-cell background colors.
    /// Premultiplied alpha blending with discard for transparent cells.
    private static func makeCellBgPipeline(
        device: MTLDevice, library: MTLLibrary
    ) throws -> MTLRenderPipelineState {
        guard let vertexFn = library.makeFunction(name: "cell_bg_vertex") else {
            throw MetalRendererError.noFunction("cell_bg_vertex")
        }
        guard let fragmentFn = library.makeFunction(name: "cell_bg_fragment") else {
            throw MetalRendererError.noFunction("cell_bg_fragment")
        }

        let desc = MTLRenderPipelineDescriptor()
        desc.label = "cell_bg"
        desc.vertexFunction = vertexFn
        desc.fragmentFunction = fragmentFn
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        // Premultiplied alpha: src=one, dst=oneMinusSourceAlpha
        desc.colorAttachments[0].isBlendingEnabled = true
        desc.colorAttachments[0].rgbBlendOperation = .add
        desc.colorAttachments[0].alphaBlendOperation = .add
        desc.colorAttachments[0].sourceRGBBlendFactor = .one
        desc.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
        desc.colorAttachments[0].sourceAlphaBlendFactor = .one
        desc.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha

        return try device.makeRenderPipelineState(descriptor: desc)
    }

    /// Pass 3: instanced glyph rendering.
    /// Premultiplied alpha blending. CellData read from buffer by instance ID.
    private static func makeCellTextPipeline(
        device: MTLDevice, library: MTLLibrary
    ) throws -> MTLRenderPipelineState {
        guard let vertexFn = library.makeFunction(name: "cell_text_vertex") else {
            throw MetalRendererError.noFunction("cell_text_vertex")
        }
        guard let fragmentFn = library.makeFunction(name: "cell_text_fragment") else {
            throw MetalRendererError.noFunction("cell_text_fragment")
        }

        let desc = MTLRenderPipelineDescriptor()
        desc.label = "cell_text"
        desc.vertexFunction = vertexFn
        desc.fragmentFunction = fragmentFn
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        // Premultiplied alpha: src=one, dst=oneMinusSourceAlpha
        desc.colorAttachments[0].isBlendingEnabled = true
        desc.colorAttachments[0].rgbBlendOperation = .add
        desc.colorAttachments[0].alphaBlendOperation = .add
        desc.colorAttachments[0].sourceRGBBlendFactor = .one
        desc.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
        desc.colorAttachments[0].sourceAlphaBlendFactor = .one
        desc.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha

        return try device.makeRenderPipelineState(descriptor: desc)
    }

    /// Pass 4: cursor overlay.
    /// Standard alpha blending for cursor with configurable opacity.
    private static func makeCursorPipeline(
        device: MTLDevice, library: MTLLibrary
    ) throws -> MTLRenderPipelineState {
        guard let vertexFn = library.makeFunction(name: "cursor_vertex") else {
            throw MetalRendererError.noFunction("cursor_vertex")
        }
        guard let fragmentFn = library.makeFunction(name: "cursor_fragment") else {
            throw MetalRendererError.noFunction("cursor_fragment")
        }

        let desc = MTLRenderPipelineDescriptor()
        desc.label = "cursor"
        desc.vertexFunction = vertexFn
        desc.fragmentFunction = fragmentFn
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        // Standard alpha: src=sourceAlpha, dst=oneMinusSourceAlpha
        desc.colorAttachments[0].isBlendingEnabled = true
        desc.colorAttachments[0].rgbBlendOperation = .add
        desc.colorAttachments[0].alphaBlendOperation = .add
        desc.colorAttachments[0].sourceRGBBlendFactor = .sourceAlpha
        desc.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
        desc.colorAttachments[0].sourceAlphaBlendFactor = .sourceAlpha
        desc.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha

        return try device.makeRenderPipelineState(descriptor: desc)
    }

    /// Pass 5: selection highlight overlay.
    /// Standard alpha blending for translucent selection rectangles.
    private static func makeSelectionPipeline(
        device: MTLDevice, library: MTLLibrary
    ) throws -> MTLRenderPipelineState {
        guard let vertexFn = library.makeFunction(name: "selection_vertex") else {
            throw MetalRendererError.noFunction("selection_vertex")
        }
        guard let fragmentFn = library.makeFunction(name: "selection_fragment") else {
            throw MetalRendererError.noFunction("selection_fragment")
        }

        let desc = MTLRenderPipelineDescriptor()
        desc.label = "selection"
        desc.vertexFunction = vertexFn
        desc.fragmentFunction = fragmentFn
        desc.colorAttachments[0].pixelFormat = .bgra8Unorm_srgb
        // Standard alpha: src=sourceAlpha, dst=oneMinusSourceAlpha
        desc.colorAttachments[0].isBlendingEnabled = true
        desc.colorAttachments[0].rgbBlendOperation = .add
        desc.colorAttachments[0].alphaBlendOperation = .add
        desc.colorAttachments[0].sourceRGBBlendFactor = .sourceAlpha
        desc.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
        desc.colorAttachments[0].sourceAlphaBlendFactor = .sourceAlpha
        desc.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha

        return try device.makeRenderPipelineState(descriptor: desc)
    }

    // MARK: - Draw Frame

    /// Render one complete frame to the given drawable.
    ///
    /// Executes all 5 pipeline passes in order:
    /// 1. bg_color — fullscreen terminal background
    /// 2. cell_bg — per-cell background colors
    /// 3. cell_text — instanced glyph rendering
    /// 4. cursor — cursor overlay
    /// 5. selection — selection highlight overlay
    ///
    /// This method does not throw. On error it logs and skips the frame
    /// to avoid stalling the display link.
    func drawFrame(
        cells: [CellData],
        cellsBg: [UInt8],
        gridSize: (cols: Int, rows: Int),
        screenSize: (width: Float, height: Float),
        gridPadding: (top: Float, right: Float, bottom: Float, left: Float),
        cursorRect: CursorRect?,
        selectionRanges: [SelectionRange],
        drawable: CAMetalDrawable
    ) {
        // DEC private mode 2026: suppress rendering during synchronized output
        if isSynchronizedOutput { return }

        // Wait for an available frame slot (triple buffering)
        frameSemaphore.wait()

        let frameIdx = frameIndex % maxFramesInFlight
        frameIndex &+= 1

        // Get mutable copy of frame state for buffer growth
        var frame = frames[frameIdx]

        // --- Build uniforms ---
        var uniforms = Uniforms()
        uniforms.projectionMatrix = orthographicProjection(
            width: screenSize.width,
            height: screenSize.height
        )
        uniforms.screenSize = SIMD2<Float>(screenSize.width, screenSize.height)
        uniforms.cellSize = cellSize
        uniforms.gridSize = SIMD2<UInt32>(UInt32(gridSize.cols), UInt32(gridSize.rows))
        uniforms.gridPadding = SIMD4<Float>(
            gridPadding.top, gridPadding.right,
            gridPadding.bottom, gridPadding.left
        )
        if let cursor = cursorRect {
            uniforms.cursorPos = cursor.pos
            uniforms.cursorColor = cursorColor
        }
        uniforms.bgColor = bgColor
        // Flags: bit 0 = has_selection, bit 1 = cursor_visible
        var flags: UInt32 = 0
        if !selectionRanges.isEmpty { flags |= 0x1 }
        if cursorRect != nil { flags |= 0x2 }
        uniforms.flags = flags
        uniforms.scale = contentsScale

        // --- Upload uniforms ---
        let uniformsSize = MemoryLayout<Uniforms>.size
        ensureBufferCapacity(&frame.uniformsBuffer, needed: uniformsSize, label: "uniforms-\(frameIdx)")
        memcpy(frame.uniformsBuffer.contents(), &uniforms, uniformsSize)

        // --- Upload cell backgrounds ---
        let cellBgSize = cellsBg.count
        if cellBgSize > 0 {
            ensureBufferCapacity(&frame.cellsBgBuffer, needed: cellBgSize, label: "cellsBg-\(frameIdx)")
            cellsBg.withUnsafeBytes { ptr in
                frame.cellsBgBuffer.contents().copyMemory(from: ptr.baseAddress!, byteCount: cellBgSize)
            }
        }
        frame.cellsBgCount = gridSize.cols * gridSize.rows

        // --- Upload cell text data ---
        let cellTextSize = cells.count * MemoryLayout<CellData>.stride
        if cellTextSize > 0 {
            ensureBufferCapacity(&frame.cellsTextBuffer, needed: cellTextSize, label: "cellsText-\(frameIdx)")
            cells.withUnsafeBytes { ptr in
                frame.cellsTextBuffer.contents().copyMemory(from: ptr.baseAddress!, byteCount: cellTextSize)
            }
        }
        frame.cellsTextCount = cells.count

        // Write back (struct is value type)
        frames[frameIdx] = frame

        // --- Create command buffer ---
        guard let commandBuffer = commandQueue.makeCommandBuffer() else {
            os_log(.error, log: MetalRenderer.log, "Failed to create command buffer — skipping frame")
            frameSemaphore.signal()
            return
        }
        commandBuffer.label = "aterm-frame"

        // Signal semaphore when GPU completes this frame
        commandBuffer.addCompletedHandler { [weak self] _ in
            self?.frameSemaphore.signal()
        }

        // --- Render pass descriptor ---
        func sRGBtoLinear(_ v: Double) -> Double {
            v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
        }
        let rpd = MTLRenderPassDescriptor()
        rpd.colorAttachments[0].texture = drawable.texture
        rpd.colorAttachments[0].loadAction = .clear
        rpd.colorAttachments[0].storeAction = .store
        rpd.colorAttachments[0].clearColor = MTLClearColor(
            red:   sRGBtoLinear(Double(bgColor.0) / 255.0),
            green: sRGBtoLinear(Double(bgColor.1) / 255.0),
            blue:  sRGBtoLinear(Double(bgColor.2) / 255.0),
            alpha: 1.0
        )

        guard let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: rpd) else {
            os_log(.error, log: MetalRenderer.log, "Failed to create render encoder — skipping frame")
            frameSemaphore.signal()
            return
        }
        encoder.label = "aterm-render"

        // --- Pass 1: bg_color (fullscreen background) ---
        // Fullscreen triangle — vertex shader generates 3 vertices procedurally.
        // Uniforms at buffer(0) provide bgColor for the fragment shader.
        encoder.setRenderPipelineState(bgColorPipeline)
        encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 0)
        encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)

        // --- Pass 2: cell_bg (per-cell background colors) ---
        // Each cell's bg color is a packed uchar4 in cellsBgBuffer.
        // Vertex shader generates a quad per cell using gridSize from uniforms.
        // Fragment shader reads the cell color and discards transparent cells.
        let totalCells = gridSize.cols * gridSize.rows
        if totalCells > 0 && cellBgSize >= totalCells * 4 {
            encoder.setRenderPipelineState(cellBgPipeline)
            encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            encoder.setVertexBuffer(frame.cellsBgBuffer, offset: 0, index: 1)
            encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            encoder.setFragmentBuffer(frame.cellsBgBuffer, offset: 0, index: 1)
            // One fullscreen triangle; shader discards pixels outside grid cells
            // OR: instanced draw with one quad per cell row
            encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        }

        // --- Pass 3a: block cursor BEHIND text (style==0) ---
        // Block cursor drawn before text so text renders on top.
        if var cursor = cursorRect, (flags & 0x2) != 0, cursor.style == 0 {
            encoder.setRenderPipelineState(cursorPipeline)
            encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            let cursorSize = MemoryLayout<CursorRect>.stride
            guard let cursorBuf = device.makeBuffer(
                bytes: &cursor,
                length: cursorSize,
                options: .storageModeShared
            ) else {
                os_log(.error, log: MetalRenderer.log, "Failed to allocate cursor buffer")
                encoder.endEncoding()
                commandBuffer.present(drawable)
                commandBuffer.commit()
                return
            }
            cursorBuf.label = "cursor-data"
            encoder.setVertexBuffer(cursorBuf, offset: 0, index: 1)
            encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        }

        // --- Pass 3b: cell_text (instanced glyph rendering) ---
        // CellData array at buffer(0), uniforms at buffer(1).
        // Vertex shader reads CellData per instance (iid) to position each glyph quad.
        // Fragment shader samples the atlas texture to get glyph coverage.
        if !cells.isEmpty {
            encoder.setRenderPipelineState(cellTextPipeline)
            encoder.setVertexBuffer(frame.cellsTextBuffer, offset: 0, index: 0)
            encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 1)
            // Bind atlas textures for fragment shader
            if let grayscale = glyphAtlas?.grayscaleTexture {
                encoder.setFragmentTexture(grayscale, index: 0)
            }
            if let color = glyphAtlas?.colorTexture {
                encoder.setFragmentTexture(color, index: 1)
            }
            encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            // 4 vertices per quad (triangle strip), one instance per glyph
            encoder.drawPrimitives(
                type: .triangleStrip,
                vertexStart: 0,
                vertexCount: 4,
                instanceCount: cells.count
            )
        }

        // --- Pass 4: bar/underline cursor ON TOP of text (style!=0) ---
        if var cursor = cursorRect, (flags & 0x2) != 0, cursor.style != 0 {
            encoder.setRenderPipelineState(cursorPipeline)
            encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            let cursorSize = MemoryLayout<CursorRect>.stride
            guard let cursorBuf = device.makeBuffer(
                bytes: &cursor,
                length: cursorSize,
                options: .storageModeShared
            ) else {
                os_log(.error, log: MetalRenderer.log, "Failed to allocate cursor buffer")
                encoder.endEncoding()
                commandBuffer.present(drawable)
                commandBuffer.commit()
                return
            }
            cursorBuf.label = "cursor-data"
            encoder.setVertexBuffer(cursorBuf, offset: 0, index: 1)
            encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        }

        // --- Pass 5: selection overlay ---
        // One instanced quad per selection range.
        if !selectionRanges.isEmpty {
            encoder.setRenderPipelineState(selectionPipeline)
            encoder.setVertexBuffer(frame.uniformsBuffer, offset: 0, index: 1)
            let selSize = selectionRanges.count * MemoryLayout<SelectionRange>.stride
            var ranges = selectionRanges
            guard let selBuf = device.makeBuffer(
                bytes: &ranges,
                length: selSize,
                options: .storageModeShared
            ) else {
                os_log(.error, log: MetalRenderer.log, "Failed to allocate selection buffer")
                encoder.endEncoding()
                commandBuffer.present(drawable)
                commandBuffer.commit()
                return
            }
            selBuf.label = "selection-data"
            encoder.setVertexBuffer(selBuf, offset: 0, index: 0)
            encoder.setFragmentBuffer(frame.uniformsBuffer, offset: 0, index: 0)
            encoder.drawPrimitives(
                type: .triangleStrip,
                vertexStart: 0,
                vertexCount: 4,
                instanceCount: selectionRanges.count
            )
        }

        // --- Finalize ---
        encoder.endEncoding()
        commandBuffer.present(drawable)
        commandBuffer.commit()
    }

    // MARK: - Orthographic Projection

    /// Build an orthographic projection matrix mapping screen coordinates
    /// to Metal clip space.
    ///
    /// Input: (0,0) at top-left, (width,height) at bottom-right (AppKit flipped).
    /// Output: Metal NDC (-1,-1) to (1,1), with Y flipped for top-left origin.
    ///
    /// Matrix (column-major):
    /// ```
    ///  2/w   0    0   0
    ///   0  -2/h   0   0
    ///   0    0    1   0
    ///  -1    1    0   1
    /// ```
    private func orthographicProjection(width: Float, height: Float) -> simd_float4x4 {
        guard width > 0 && height > 0 else { return matrix_identity_float4x4 }
        return simd_float4x4(
            SIMD4<Float>(2.0 / width,  0,             0, 0),
            SIMD4<Float>(0,           -2.0 / height,  0, 0),
            SIMD4<Float>(0,            0,             1, 0),
            SIMD4<Float>(-1,           1,             0, 1)
        )
    }

    // MARK: - Buffer Management

    /// Grow a Metal buffer if its current length is insufficient.
    ///
    /// Allocates a new buffer at 2x the needed size (or at least the needed size)
    /// to amortize future growth. The old buffer is released by ARC.
    private func ensureBufferCapacity(_ buffer: inout MTLBuffer, needed: Int, label: String) {
        guard buffer.length < needed else { return }
        let newSize = max(needed, buffer.length * 2)
        guard let newBuffer = device.makeBuffer(length: newSize, options: .storageModeShared) else {
            os_log(.error, log: MetalRenderer.log, "Failed to grow buffer '%{public}s' to %d bytes", label, newSize)
            return
        }
        newBuffer.label = label
        buffer = newBuffer
    }

    // MARK: - Utility

    /// Wait for all in-flight frames to complete. Call before deallocation
    /// or when the rendering surface is invalidated.
    func waitForAllFrames() {
        for _ in 0..<maxFramesInFlight {
            frameSemaphore.wait()
        }
        for _ in 0..<maxFramesInFlight {
            frameSemaphore.signal()
        }
    }

    /// Update the background color from RGBA components.
    func setBgColor(r: UInt8, g: UInt8, b: UInt8, a: UInt8 = 0xFF) {
        NSLog("[METAL-SETBG] setBgColor called with RGBA=(%d,%d,%d,%d) from=%@", Int(r), Int(g), Int(b), Int(a), Thread.callStackSymbols[1])
        bgColor = (r, g, b, a)
    }

    /// Update the cursor color from RGBA components.
    func setCursorColor(r: UInt8, g: UInt8, b: UInt8, a: UInt8 = 0xFF) {
        cursorColor = (r, g, b, a)
    }

    /// Update cell dimensions. Called when font size or line height changes.
    func setCellSize(width: Float, height: Float) {
        cellSize = SIMD2<Float>(width, height)
    }
}
