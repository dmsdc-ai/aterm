import Foundation
import Metal
import CoreText
import CoreGraphics
import AppKit  // NSFont for fallback chain (userFixedPitchFont, systemFont)

// MARK: - GlyphKey

/// Cache key: uniquely identifies a rasterized glyph variant.
/// flags encodes style bits (bold=0x1, italic=0x2) so different variants
/// of the same codepoint share no atlas space.
struct GlyphKey: Hashable {
    let fontSize: Float
    let codepoint: UInt32
    let flags: UInt8
}

// MARK: - AtlasRect

/// Position and metrics of a glyph inside the atlas texture.
struct AtlasRect {
    /// Top-left corner in atlas texels.
    let x: Int
    let y: Int
    /// Glyph bitmap dimensions.
    let width: Int
    let height: Int
    /// Horizontal bearing: pixels from glyph origin to left edge of bitmap.
    let bearingX: Int
    /// Vertical bearing: pixels from baseline to top edge of bitmap.
    let bearingY: Int
    /// Horizontal advance for cursor movement.
    let advance: Float
}

// MARK: - GlyphAtlas

/// Workspace-shared Metal glyph atlas. ghostty SharedGrid pattern.
///
/// All TerminalView instances share a single atlas singleton, so glyph data is
/// rasterized once and reused across sessions — matching ghostty's 25-session
/// 40 MB footprint.
///
/// Architecture:
///   • Shelf-bin-packing (row allocator) inside one MTLTexture
///   • Atlas grows 2× (512→1024→2048→4096) via GPU blit when full
///   • CoreText CTFont rasterization — no CPU pixel retention after upload
///   • `modified` counter: renderers skip texture re-bind when counter unchanged
///   • LRU eviction at MAX_GLYPHS=4096 — non-ASCII glyphs evicted first
final class GlyphAtlas {

    // MARK: Singleton

    static let shared = GlyphAtlas()

    // MARK: Constants

    private static let initialSize = 512
    private static let maxAtlasSize = 4096
    private static let maxGlyphs = 4096

    // MARK: Metal

    private let device: MTLDevice
    private let blitQueue: MTLCommandQueue  // dedicated queue for grow blits

    /// Current atlas MTLTexture. Renderers sample this for glyph coverage.
    /// Replace-reference is set atomically under `lock`; renderers should
    /// re-bind whenever `modified` changes.
    private(set) var texture: MTLTexture

    /// Current square atlas dimension in texels.
    private(set) var atlasSize: Int

    // MARK: Modified counter (ghostty atlas.modified pattern)

    /// Incremented each time a glyph is added or the atlas grows.
    /// Renderer caches last-seen value; re-uploads the texture only when changed.
    private(set) var modified: Int = 0

    // MARK: Shelf allocator state

    private var shelves: [Shelf] = []
    private var nextShelfY: Int = 0

    // MARK: Cache

    private var cache: [GlyphKey: AtlasRect] = [:]

    // MARK: LRU (epoch-based, evict oldest non-ASCII)

    private var epochCounter: Int = 0
    private var epochMap: [GlyphKey: Int] = [:]  // key → last-access epoch

    // MARK: Thread safety

    private let lock = NSLock()

    /// Debug logging flag — cached from environment at init.
    private let debugLogEnabled: Bool = ProcessInfo.processInfo.environment["ATERM_DEBUG_LOG"] != nil

    /// Display scale factor for Retina rendering.
    /// Glyphs are rasterized at physical pixel dimensions (logical × scale).
    /// Default 2.0 for Apple Silicon Retina; TerminalView calls updateScale()
    /// with the actual backingScaleFactor on window attach and display change.
    private var contentsScale: CGFloat = 2.0

    // MARK: - Font family (user-configurable via Settings)

    /// Current resolved font family used for rasterization.
    /// Read by GlyphAtlas.makeStyledFont (TerminalView extension) and by
    /// AppDelegate.applySettingsToView() for cell-metric computation.
    /// Single source of truth for the active terminal font name.
    private(set) var resolvedFontFamily: String = "Menlo-Regular"

    /// Dedupe-log state (one-shot warnings per session).
    private var warnedNonMonospaced: Set<String> = []
    private var warnedMissingBold: Set<String> = []
    private var warnedMissingItalic: Set<String> = []
    private var warnedMissingBoldItalic: Set<String> = []
    private var warnedFallbackChain: Set<String> = []

    // MARK: - Init

    private init() {
        guard let dev = MTLCreateSystemDefaultDevice() else {
            fatalError("[GlyphAtlas] No Metal device")
        }
        guard let q = dev.makeCommandQueue() else {
            fatalError("[GlyphAtlas] Cannot create blit command queue")
        }
        device = dev
        blitQueue = q
        atlasSize = GlyphAtlas.initialSize
        texture = GlyphAtlas.makeTexture(device: dev, size: GlyphAtlas.initialSize)
    }

    // MARK: - Public API

    /// Return the AtlasRect for a glyph, rasterizing it if not yet cached.
    ///
    /// - Parameters:
    ///   - key: Cache key (fontSize, codepoint, flags).
    ///   - font: CTFont matching key.fontSize and key.flags. Caller owns lifetime.
    /// - Returns: AtlasRect with pixel position in atlas, or nil if the font
    ///            does not contain the codepoint or the atlas is exhausted.
    func rect(for key: GlyphKey, font: CTFont) -> AtlasRect? {
        lock.lock()
        defer { lock.unlock() }

        if let cached = cache[key] {
            touch(key)
            return cached
        }

        guard let result = rasterize(codepoint: key.codepoint, font: font) else {
            return nil
        }

        // Zero-size glyphs (space, control chars) — cache metrics but no atlas slot
        if result.width == 0 || result.height == 0 {
            let r = AtlasRect(x: 0, y: 0, width: 0, height: 0,
                              bearingX: result.bearingX, bearingY: result.bearingY,
                              advance: result.advance)
            cache[key] = r
            touch(key)
            return r
        }

        // Allocate shelf space (grows atlas if needed)
        guard let (ax, ay) = allocateShelf(width: result.width, height: result.height) else {
            if debugLogEnabled {
                NSLog("%@", "[atlas] shelf alloc failed for cp=0x\(String(key.codepoint, radix: 16)) size=\(result.width)x\(result.height) atlasSize=\(atlasSize)" as NSString)
            }
            return nil  // Atlas at max size and entirely full
        }

        upload(pixels: result.pixels, x: ax, y: ay, width: result.width, height: result.height)
        modified &+= 1

        // DIAGNOSTIC #209: verify atlas texture has correct glyph data after upload.
        // Reads back center pixel from both source bitmap and MTLTexture to confirm
        // the upload wrote the right data to the right location.
        if cache.count <= 10 {
            let midX = ax + result.width / 2
            let midY = ay + result.height / 2
            var texPixel: UInt8 = 0
            texture.getBytes(&texPixel, bytesPerRow: 1,
                             from: MTLRegion(origin: MTLOrigin(x: midX, y: midY, z: 0),
                                            size: MTLSize(width: 1, height: 1, depth: 1)),
                             mipmapLevel: 0)
            let srcIdx = (result.height / 2) * result.width + (result.width / 2)
            let srcPixel = srcIdx < result.pixels.count ? result.pixels[srcIdx] : 0
            let ch = Unicode.Scalar(key.codepoint).map { String($0) } ?? "?"
            NSLog("[ATLAS-VERIFY] U+%04X(%@) atlas(%d,%d,%d,%d) srcCenter=%d texCenter=%d %@",
                  key.codepoint, ch as NSString, ax, ay, result.width, result.height,
                  srcPixel, texPixel,
                  (srcPixel == texPixel ? "MATCH" : "⚠️ MISMATCH") as NSString)
        }

        let r = AtlasRect(x: ax, y: ay, width: result.width, height: result.height,
                          bearingX: result.bearingX, bearingY: result.bearingY,
                          advance: result.advance)
        cache[key] = r
        touch(key)

        if cache.count > GlyphAtlas.maxGlyphs {
            evictOldestNonASCII()
        }

        return r
    }

    /// Invalidate the entire atlas (e.g. font family or DPI change).
    /// Renderers will observe `modified` increment and re-bind the new texture.
    func invalidate() {
        lock.lock()
        defer { lock.unlock() }
        resetState()
    }

    /// Update display scale factor and re-rasterize all glyphs at new resolution.
    /// Called from TerminalView.viewDidChangeBackingProperties() when the window
    /// moves between displays with different backing scale factors.
    func updateScale(_ scale: CGFloat) {
        lock.lock()
        defer { lock.unlock() }
        let clamped = max(scale, 1.0)
        guard clamped != contentsScale else { return }
        contentsScale = clamped
        resetState()
    }

    // MARK: - Font family resolution & hot-reload

    /// Apply a new font family by name. Called from AppDelegate.applySettingsToView
    /// BEFORE cell-metric recomputation so the atlas is cleared and new metrics
    /// are computed from the same resolved font (single source of truth).
    ///
    /// - Q-font-1: hot-reload under existing NSLock, no restart required.
    /// - Q-font-2: fallback chain userFixedPitchFont → Menlo → SFMono → systemFont.
    /// - Q-font-3: non-monospaced fonts accepted with soft warn (not rejected).
    /// - Q-font-5: "System Default" sentinel maps to userFixedPitchFont (chain entry).
    func setFontFamily(_ requested: String) {
        lock.lock()
        defer { lock.unlock() }
        let resolved = resolveFontName(requested)
        guard resolved != resolvedFontFamily else { return }
        resolvedFontFamily = resolved
        resetState()  // clears cache, bumps modified, re-creates texture
    }

    /// Resolve a user-supplied family name to an actual usable font name.
    /// Must be called with `lock` held (mutates `warnedNonMonospaced` / `warnedFallbackChain`).
    private func resolveFontName(_ requested: String) -> String {
        let trimmed = requested.trimmingCharacters(in: .whitespaces)

        // Sentinel: "System Default" → enter fallback chain at tier 1
        if trimmed.isEmpty || trimmed == "System Default" {
            return fallbackChainStart(requestedKey: "System Default")
        }

        // Explicit path: probe + compare PostScript/family names
        let probe = CTFontCreateWithName(trimmed as CFString, 12, nil)
        let postScript = CTFontCopyPostScriptName(probe) as String
        let familyName = CTFontCopyFamilyName(probe) as String
        let normRequested = trimmed.replacingOccurrences(of: " ", with: "").lowercased()
        let normPost = postScript.replacingOccurrences(of: "-", with: "").lowercased()
        let normFamily = familyName.replacingOccurrences(of: " ", with: "").lowercased()
        let accepted =
            postScript.caseInsensitiveCompare(trimmed) == .orderedSame
            || postScript.lowercased().hasPrefix(trimmed.lowercased())
            || familyName.caseInsensitiveCompare(trimmed) == .orderedSame
            || normPost == normRequested
            || normFamily == normRequested

        if accepted {
            warnIfNonMonospaced(font: probe, family: trimmed)
            return trimmed
        }

        // Substitution detected → fall back to chain
        NSLog(
            "[FONT-WARN] requested family '%@' not found (resolved to '%@'), using fallback chain",
            trimmed, postScript)
        return fallbackChainStart(requestedKey: trimmed)
    }

    /// Walk the automatic fallback chain per Q-font-2.
    /// Tiers: (1) userFixedPitchFont, (2) Menlo-Regular, (3) SFMono-Regular, (4) systemFont.
    /// Must be called with `lock` held.
    private func fallbackChainStart(requestedKey: String) -> String {
        let probeSize: CGFloat = 12

        // Tier 1: NSFont.userFixedPitchFont (Q-font-5 entry point)
        if let f = NSFont.userFixedPitchFont(ofSize: probeSize) {
            let name = f.fontName
            logFallbackIfNew(requestedKey: requestedKey, tier: "userFixedPitchFont", resolved: name)
            return name
        }

        // Tier 2: Menlo-Regular — always present on macOS
        let menloProbe = CTFontCreateWithName("Menlo-Regular" as CFString, probeSize, nil)
        let menloPS = CTFontCopyPostScriptName(menloProbe) as String
        if menloPS.caseInsensitiveCompare("Menlo-Regular") == .orderedSame
            || menloPS.lowercased().hasPrefix("menlo")
        {
            logFallbackIfNew(
                requestedKey: requestedKey, tier: "Menlo-Regular", resolved: "Menlo-Regular")
            return "Menlo-Regular"
        }

        // Tier 3: SFMono-Regular (AppleSystemUIFontMonospaced canonical name)
        let sfmProbe = CTFontCreateWithName("SFMono-Regular" as CFString, probeSize, nil)
        let sfmPS = CTFontCopyPostScriptName(sfmProbe) as String
        if sfmPS.lowercased().hasPrefix("sfmono") {
            logFallbackIfNew(
                requestedKey: requestedKey, tier: "SFMono-Regular", resolved: "SFMono-Regular")
            return "SFMono-Regular"
        }

        // Tier 4: systemFont — absolute last resort, always non-nil
        let sys = NSFont.systemFont(ofSize: probeSize)
        logFallbackIfNew(requestedKey: requestedKey, tier: "systemFont", resolved: sys.fontName)
        return sys.fontName
    }

    /// Log one fallback-chain transition per (requestedKey, tier) combination.
    private func logFallbackIfNew(requestedKey: String, tier: String, resolved: String) {
        let key = "\(requestedKey)→\(tier)"
        guard !warnedFallbackChain.contains(key) else { return }
        warnedFallbackChain.insert(key)
        NSLog(
            "[FONT-WARN] fallback chain for '%@' → %@ (%@)", requestedKey, tier, resolved)
    }

    /// Emit a single non-monospaced warning per family. Q-font-3: soft warn, do not reject.
    private func warnIfNonMonospaced(font: CTFont, family: String) {
        let traits = CTFontGetSymbolicTraits(font)
        let isMono = traits.contains(.traitMonoSpace)
        if !isMono, !warnedNonMonospaced.contains(family) {
            warnedNonMonospaced.insert(family)
            NSLog(
                "[FONT-WARN] %@ is not monospaced — terminal grid may misalign", family)
        }
    }

    /// Called from TerminalView.GlyphAtlas extension when a bold / italic /
    /// bold-italic variant is missing. Logs once per (family, variant).
    /// Q-font-4: single-shot warn log per family per variant on first miss.
    func logMissingVariant(family: String, bold: Bool, italic: Bool) {
        lock.lock()
        defer { lock.unlock() }
        let variant: String
        switch (bold, italic) {
        case (true, true): variant = "BoldItalic"
        case (true, false): variant = "Bold"
        case (false, true): variant = "Italic"
        default: return  // Regular never missing
        }
        let alreadyWarned: Bool
        switch variant {
        case "Bold":
            alreadyWarned = warnedMissingBold.contains(family)
            if !alreadyWarned { warnedMissingBold.insert(family) }
        case "Italic":
            alreadyWarned = warnedMissingItalic.contains(family)
            if !alreadyWarned { warnedMissingItalic.insert(family) }
        default:
            alreadyWarned = warnedMissingBoldItalic.contains(family)
            if !alreadyWarned { warnedMissingBoldItalic.insert(family) }
        }
        if !alreadyWarned {
            NSLog(
                "[FONT-WARN] %@ has no %@ variant — using Regular", family, variant)
        }
    }

    // MARK: - Shelf allocator

    private func allocateShelf(width: Int, height: Int) -> (Int, Int)? {
        // Best-fit: smallest shelf whose height ≥ glyph height, with room on X.
        var bestIdx: Int? = nil
        var bestWaste = Int.max
        for (i, shelf) in shelves.enumerated() {
            guard height <= shelf.height else { continue }
            guard shelf.cursorX + width <= atlasSize else { continue }
            let waste = shelf.height - height
            if waste < bestWaste {
                bestWaste = waste
                bestIdx = i
            }
        }
        if let idx = bestIdx {
            let x = shelves[idx].cursorX
            shelves[idx].cursorX += width
            return (x, shelves[idx].y)
        }

        // Open a new shelf if vertical space remains.
        if nextShelfY + height <= atlasSize {
            let y = nextShelfY
            let shelf = Shelf(y: y, height: height)
            shelf.cursorX = width
            shelves.append(shelf)
            nextShelfY += height
            return (0, y)
        }

        // Atlas full — grow if not at limit, then retry once.
        guard atlasSize < GlyphAtlas.maxAtlasSize else { return nil }
        grow()

        if nextShelfY + height <= atlasSize {
            let y = nextShelfY
            let shelf = Shelf(y: y, height: height)
            shelf.cursorX = width
            shelves.append(shelf)
            nextShelfY += height
            return (0, y)
        }

        return nil  // Still no room after grow (shouldn't happen for normal glyphs)
    }

    /// Double the atlas size via GPU blit. Shelves and cache offsets remain valid.
    private func grow() {
        let newSize = min(atlasSize * 2, GlyphAtlas.maxAtlasSize)
        guard newSize > atlasSize else { return }

        NSLog("[ATLAS-GROW] old=%d new=%d glyphs=%d", atlasSize, newSize, cache.count)

        let newTexture = GlyphAtlas.makeTexture(device: device, size: newSize)

        // GPU blit: copy existing glyph data into the larger texture.
        if let cmd = blitQueue.makeCommandBuffer(),
           let blit = cmd.makeBlitCommandEncoder() {
            blit.copy(
                from: texture,
                sourceSlice: 0, sourceLevel: 0,
                sourceOrigin: MTLOrigin(x: 0, y: 0, z: 0),
                sourceSize: MTLSize(width: atlasSize, height: atlasSize, depth: 1),
                to: newTexture,
                destinationSlice: 0, destinationLevel: 0,
                destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0)
            )
            blit.endEncoding()
            cmd.commit()
            cmd.waitUntilCompleted()
        }

        atlasSize = newSize
        texture = newTexture
        modified &+= 1  // Renderer must re-bind the new texture object
    }

    // MARK: - CoreText rasterization

    private struct RasterResult {
        let pixels: [UInt8]  // Single-channel grayscale (r8Unorm), top-down
        let width: Int
        let height: Int
        let bearingX: Int   // pixels from glyph origin to left edge
        let bearingY: Int   // pixels from baseline to top edge
        let advance: Float
    }

    private func rasterize(codepoint: UInt32, font: CTFont) -> RasterResult? {
        guard let scalar = Unicode.Scalar(codepoint) else {
            if debugLogEnabled { NSLog("%@", "[atlas] invalid scalar for cp=0x\(String(codepoint, radix: 16))" as NSString) }
            return nil
        }

        let str = String(Character(scalar))
        var utf16 = Array(str.utf16)  // 1 UniChar (BMP) or 2 UniChars (surrogate pair)
        var glyphs = [CGGlyph](repeating: 0, count: utf16.count)

        // Try primary font, then CoreText font fallback for CJK/special chars
        var renderFont = font
        let found = CTFontGetGlyphsForCharacters(font, &utf16, &glyphs, utf16.count)
        if !found || glyphs[0] == 0 {
            let fallback = CTFontCreateForString(font, str as CFString,
                                                  CFRange(location: 0, length: str.utf16.count))
            var fallbackGlyphs = [CGGlyph](repeating: 0, count: utf16.count)
            let found2 = CTFontGetGlyphsForCharacters(fallback, &utf16, &fallbackGlyphs, utf16.count)
            guard found2, fallbackGlyphs[0] != 0 else {
                if debugLogEnabled {
                    let fontName = CTFontCopyPostScriptName(font) as String
                    NSLog("%@", "[atlas] glyph not found (with fallback): cp=0x\(String(codepoint, radix: 16)) char='\(str)' font=\(fontName)" as NSString)
                }
                return nil
            }
            glyphs = fallbackGlyphs
            renderFont = fallback
        }

        var g = glyphs[0]

        // Advance from ORIGINAL font (logical points — used for cell positioning)
        var advance = CGSize.zero
        CTFontGetAdvancesForGlyphs(renderFont, .default, &g, &advance, 1)

        // Retina: ghostty pattern — scale the FONT, not the context CTM.
        // CTFontCreateCopyWithAttributes at fontSize × contentsScale produces a font
        // whose bounds and rendering are naturally in physical pixels.
        // This avoids double-scaling from ctx.scaleBy + physical pixel context.
        let scale = contentsScale
        let scaledFont: CTFont
        if scale != 1.0 {
            let scaledSize = CTFontGetSize(renderFont) * scale
            scaledFont = CTFontCreateCopyWithAttributes(renderFont, scaledSize, nil, nil)
        } else {
            scaledFont = renderFont
        }

        // Bounding rect from scaled font — naturally in physical pixel units
        let bounds = CTFontGetBoundingRectsForGlyphs(scaledFont, .default, &g, nil, 1)

        let bearingX = Int(floor(bounds.minX))

        // Zero-size check (space, zero-width chars)
        let rawW = Int(ceil(bounds.width))
        let rawH = Int(ceil(bounds.height))
        guard rawW > 0, rawH > 0 else {
            return RasterResult(pixels: [], width: 0, height: 0,
                                bearingX: bearingX, bearingY: 0,
                                advance: Float(advance.width))
        }

        // +2px padding per axis for antialiasing fringe
        let w = rawW + 2
        let h = rawH + 2

        // ghostty pattern: bearingY = px_y + px_height (distance from cell bottom to glyph top)
        // px_y = floor(bounds.minY) — glyph bottom offset from baseline (physical px)
        // px_height = h — rasterized bitmap height (incl. +2px AA padding)
        let bearingY = Int(floor(bounds.minY)) + h
        let rgbaBytesPerRow = w * 4

        // Rasterize to RGBA (CoreText requires color context), then extract R channel
        var rgbaPixels = [UInt8](repeating: 0, count: h * rgbaBytesPerRow)

        rgbaPixels.withUnsafeMutableBytes { buf in
            guard let ctx = CGContext(
                data: buf.baseAddress,
                width: w, height: h,   // physical pixel dimensions (from scaled font bounds)
                bitsPerComponent: 8,
                bytesPerRow: rgbaBytesPerRow,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return }

            // NO scaleBy — scaled font renders directly at physical pixel resolution.
            // White glyph — shader applies terminal foreground colour; alpha = coverage.
            ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))

            // Glyph origin in physical pixel coordinates (bounds are from scaled font):
            //   x: 1px left pad − left bearing  → leftmost glyph pixel lands at x=1
            //   y: 1px bottom pad − descent      → bottom of glyph lands at y=1
            let ox = CGFloat(1) - bounds.minX
            let oy = CGFloat(1) - bounds.minY
            var pos = CGPoint(x: ox, y: oy)
            CTFontDrawGlyphs(scaledFont, &g, &pos, 1, ctx)
        }

        // Extract R channel → single-channel r8Unorm (ghostty pattern).
        // White premultiplied: R = G = B = A = coverage.
        var grayscale = [UInt8](repeating: 0, count: w * h)
        for row in 0..<h {
            for col in 0..<w {
                grayscale[row * w + col] = rgbaPixels[row * rgbaBytesPerRow + col * 4]
            }
        }

        return RasterResult(pixels: grayscale, width: w, height: h,
                            bearingX: bearingX, bearingY: bearingY,
                            advance: Float(advance.width))
    }

    // MARK: - CPU→GPU upload

    private func upload(pixels: [UInt8], x: Int, y: Int, width: Int, height: Int) {
        let region = MTLRegion(
            origin: MTLOrigin(x: x, y: y, z: 0),
            size: MTLSize(width: width, height: height, depth: 1)
        )
        pixels.withUnsafeBytes { ptr in
            texture.replace(region: region, mipmapLevel: 0,
                            withBytes: ptr.baseAddress!, bytesPerRow: width)
        }
    }

    // MARK: - LRU

    private func touch(_ key: GlyphKey) {
        epochCounter &+= 1
        epochMap[key] = epochCounter
    }

    /// Remove the oldest non-ASCII glyph to stay under MAX_GLYPHS.
    /// ASCII glyphs (codepoint ≤ 127) are kept permanently — they are accessed
    /// on every frame and cheap to protect.
    private func evictOldestNonASCII() {
        var evictKey: GlyphKey? = nil
        var oldestEpoch = Int.max

        for key in cache.keys where key.codepoint > 127 {
            let epoch = epochMap[key] ?? 0
            if epoch < oldestEpoch {
                oldestEpoch = epoch
                evictKey = key
            }
        }

        guard let k = evictKey else { return }
        cache.removeValue(forKey: k)
        epochMap.removeValue(forKey: k)
        // Note: the atlas texels are NOT cleared — eviction only removes the
        // cache entry. If the glyph is needed again it will be re-rasterized
        // into a freshly allocated slot (the old texels are abandoned in place,
        // which is fine because shelves are never compacted).
    }

    // MARK: - Reset

    private func resetState() {
        cache.removeAll(keepingCapacity: true)
        epochMap.removeAll(keepingCapacity: true)
        epochCounter = 0
        shelves.removeAll()
        nextShelfY = 0
        atlasSize = GlyphAtlas.initialSize
        texture = GlyphAtlas.makeTexture(device: device, size: GlyphAtlas.initialSize)
        modified &+= 1
    }

    // MARK: - Texture factory

    private static func makeTexture(device: MTLDevice, size: Int) -> MTLTexture {
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .r8Unorm,
            width: size, height: size,
            mipmapped: false
        )
        desc.usage = .shaderRead
        // .shared = Apple Silicon unified memory — zero-copy between CPU and GPU.
        // aterm-darwin-arm64 only, so this is always valid.
        desc.storageMode = .shared
        guard let tex = device.makeTexture(descriptor: desc) else {
            fatalError("[GlyphAtlas] Cannot allocate \(size)×\(size) r8Unorm atlas texture")
        }
        return tex
    }
}

// MARK: - Shelf (row allocator)

private final class Shelf {
    let y: Int       // Top-left Y of this shelf row
    let height: Int  // Row height = tallest glyph placed (fixed at shelf creation)
    var cursorX: Int // Next available X position

    init(y: Int, height: Int) {
        self.y = y
        self.height = height
        self.cursorX = 0
    }
}
