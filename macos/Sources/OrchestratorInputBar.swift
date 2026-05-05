import AppKit

/// Dedicated input bar for orchestrator (isSystem) workspaces.
///
/// v3.1 (hybrid: v2 edge-to-edge layout + v3 typography):
///   - Transparent container (sidebar bg shows through) — NO card elevation
///   - cornerRadius = 0 (flat rectangle)
///   - Edge-to-edge horizontally (no horizontal margin from bar edges)
///   - 16pt vertical + 20pt horizontal interior padding (typography unchanged)
///   - minBarHeight = 48pt (matches v2 via barTop=0, barBottom=0, containerContentHeight=48)
///   - 1px hairline top separator RESTORED (α=0.35) — divides bar from terminal area above
///   - Border: α=0 absolute (borderWidth=0) — no card outline
///   - `chevron.right` 11pt REGULAR weight, α=0.65 resting → 1.0 focused (prompt tint shift only)
///   - Placeholder: regular (non-italic), α=0.55 muted
///   - Focus animation: 200ms ease-out — prompt tint only (NO background change since transparent)
///   - Dynamic height: 120ms ease-out growth to 200pt max on multi-line (unchanged)
///   - 3px thin custom scroller, always visible
///
/// Preserves 100% of v2/#240 functionality: history ↑/↓, Ctrl+C/D/L passthrough,
/// `/` command dropdown, Tab completion, Shift+Enter multi-line, ESC toggle, IME,
/// cursor vertical centering formula (constants shifted, shape preserved).
final class OrchestratorInputBar: NSView, NSTextViewDelegate {
    private let separatorView = NSView()
    private let containerView = NSView()
    private let promptLabel = NSImageView()
    private let scrollView = NSScrollView()
    private let textView = OrchestratorTextView()
    private let history = OrchestratorHistory()
    private let commandDropdown = CommandDropdown()

    /// Called with the entered text (without trailing \r) when user presses Enter.
    var onSubmit: ((String) -> Void)?
    /// Called when user presses ESC to toggle focus back to the terminal.
    var onEscape: (() -> Void)?
    /// Called with a raw byte (e.g. 0x03 = Ctrl+C, 0x04 = Ctrl+D, 0x0C = Ctrl+L).
    var onCtrlKey: ((UInt8) -> Void)?
    /// Called whenever the bar's preferred height changes (for container relayout).
    var onHeightChange: ((CGFloat) -> Void)?

    private var isFocused = false
    private var heightConstraint: NSLayoutConstraint!

    // MARK: - Line metrics (computed once from actual font)

    private static let inputFont = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    /// CJK-aware line height — takes the max of Latin monospaced defaultLineHeight and
    /// the actual CJK fallback layout height. P0 fix for v3.1 cursor top-alignment:
    /// `NSLayoutManager.defaultLineHeight(for: monospacedSystemFont)` returns ~14-16pt
    /// which is Latin-only metrics. When Korean/CJK text is laid out, the font
    /// substitution system swaps in Hiragino Sans (or similar) whose actual line
    /// height is ~20-22pt. If we size the bar/padding from Latin metrics alone, CJK
    /// lines overflow the text area and the cursor appears top-aligned.
    ///
    /// Fix: pre-measure both Latin and CJK by laying out representative glyphs in a
    /// dummy NSLayoutManager + NSTextContainer, take the larger. Downstream math
    /// (`containerContentHeight`, `textContainerInset.height`, `minBarHeight`) uses
    /// this value uniformly so both scripts render centered.
    private static let lineHeight: CGFloat = {
        let font = inputFont
        // Latin metrics — from font directly via NSLayoutManager default
        let latinHeight: CGFloat = {
            let lm = NSLayoutManager()
            return ceil(lm.defaultLineHeight(for: font))
        }()
        // CJK metrics — lay out a Korean glyph to trigger font fallback and measure
        // the actual used rect height (which reflects the substitute font's metrics).
        let cjkHeight: CGFloat = {
            let storage = NSTextStorage(
                string: "한글", attributes: [.font: font])
            let container = NSTextContainer(size: NSSize(width: 10000, height: 10000))
            container.lineFragmentPadding = 0
            let lm = NSLayoutManager()
            storage.addLayoutManager(lm)
            lm.addTextContainer(container)
            lm.ensureLayout(for: container)
            return ceil(lm.usedRect(for: container).height)
        }()
        return max(latinHeight, cjkHeight)
    }()

    // v3.1 — hybrid geometry (v2 edge-to-edge + v3 typography constants)
    /// Container interior vertical padding (top AND bottom) — applied via `textContainerInset.height`.
    /// Formula preserved from #240: `textContainerInset.height = verticalPadding` → text centers
    /// at `container.top + verticalPadding + lineHeight/2`. Constant is 16 (v3).
    private static let verticalPadding: CGFloat = 16
    /// Container interior horizontal padding — leading offset from bar edge for text/prompt.
    private static let horizontalPadding: CGFloat = 20
    /// Flat rectangle — no rounded corners (v3.1 reverted from v3's 16pt card radius).
    private static let cornerRadius: CGFloat = 0
    /// Container height for single line (text line + symmetric 16pt padding).
    private static let containerContentHeight: CGFloat = lineHeight + verticalPadding * 2
    /// Edge-to-edge — no outer horizontal margin (v3.1 reverted from v3's 8pt card margin).
    private static let barHorizontalMargin: CGFloat = 0
    /// No outer vertical bar margins — container fills bar vertically to keep minBarHeight = 48.
    private static let barTopMargin: CGFloat = 0
    private static let barBottomMargin: CGFloat = 0
    /// Top hairline separator alpha — divides transparent bar from terminal area above.
    private static let separatorAlpha: CGFloat = 0.35

    static let minBarHeight: CGFloat =
        containerContentHeight + barTopMargin + barBottomMargin
    //  = (lineHeight ~16) + 32 + 0 + 0 = 48pt — matches v2
    private static let maxBarHeight: CGFloat = 200

    override init(frame: NSRect) {
        super.init(frame: frame)
        setup()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setup()
    }

    private func setup() {
        wantsLayer = true
        translatesAutoresizingMaskIntoConstraints = false

        heightConstraint = heightAnchor.constraint(equalToConstant: Self.minBarHeight)
        heightConstraint.priority = .required
        heightConstraint.isActive = true

        // v3.1: 1px hairline top separator RESTORED — divides transparent bar from terminal above.
        separatorView.wantsLayer = true
        separatorView.translatesAutoresizingMaskIntoConstraints = false
        separatorView.layer?.backgroundColor =
            AtermTheme.inputBarBorder.withAlphaComponent(Self.separatorAlpha).cgColor
        addSubview(separatorView)

        // v3.1: transparent container, flat rectangle (no radius), no border.
        containerView.wantsLayer = true
        containerView.translatesAutoresizingMaskIntoConstraints = false
        containerView.layer?.cornerRadius = Self.cornerRadius
        containerView.layer?.masksToBounds = false
        addSubview(containerView)

        applyContainerStyle(focused: false)

        // Prompt glyph: chevron.right SF Symbol, 11pt REGULAR weight (v3 Direction E — was semibold),
        // α=0.65 resting muted tint → full accent on focus (Q5 approved).
        promptLabel.translatesAutoresizingMaskIntoConstraints = false
        promptLabel.imageScaling = .scaleProportionallyDown
        promptLabel.contentTintColor = AtermTheme.inputBarPrompt.withAlphaComponent(0.65)
        if let symbol = NSImage(
            systemSymbolName: "chevron.right", accessibilityDescription: "prompt")
        {
            let config = NSImage.SymbolConfiguration(pointSize: 11, weight: .regular)
            promptLabel.image = symbol.withSymbolConfiguration(config)
        }
        containerView.addSubview(promptLabel)

        // Custom 3px thin scroller, always visible (Q4 decision: orchestrator bar is
        // always in use, scroll affordance must be persistent).
        let thin = ThinScroller()
        thin.scrollerStyle = .legacy
        scrollView.verticalScroller = thin

        scrollView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = false  // always visible
        scrollView.scrollerStyle = .legacy
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder

        // NSTextView — textContainerInset computed for VERTICAL CENTERING.
        // Root cause fix for the cursor top-alignment bug: instead of hardcoded
        // 6pt insets + misaligned scroll padding, we derive the top/bottom padding
        // from the actual NSFont line height so the caret sits at (containerH - lineH) / 2.
        textView.isEditable = true
        textView.isSelectable = true
        textView.isRichText = false
        textView.allowsUndo = true
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.font = Self.inputFont
        textView.textColor = AtermTheme.textPrimary
        // NOTE: insertionPointColor intentionally NOT set — use NSTextView default
        // (Q1 decision: drop amber signature cursor for this view).
        textView.textContainerInset = NSSize(width: 0, height: Self.verticalPadding)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.smartInsertDeleteEnabled = false
        textView.delegate = self

        // Force consistent line height across Latin + CJK so both center identically.
        // Without this, Latin (16pt line) would top-align within the CJK-sized (22pt)
        // text area, causing asymmetric centering. With forced min/max, NSLayoutManager
        // adds extra leading to Latin lines to match CJK height.
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.minimumLineHeight = Self.lineHeight
        paragraphStyle.maximumLineHeight = Self.lineHeight
        textView.defaultParagraphStyle = paragraphStyle
        textView.typingAttributes = [
            .font: Self.inputFont,
            .paragraphStyle: paragraphStyle,
            .foregroundColor: AtermTheme.textPrimary,
        ]

        // Placeholder: regular (non-italic), α=0.55 muted (v3 Direction E tone match).
        // Text unchanged from v2 — #246 English default preserved.
        textView.placeholderText = AtermLocalization.text(
            ko: "명령어 입력...",
            en: "Type a command...")
        textView.placeholderAttributes = [
            .foregroundColor: AtermTheme.inputBarPlaceholder.withAlphaComponent(0.55),
            .font: Self.inputFont,
        ]

        scrollView.documentView = textView
        containerView.addSubview(scrollView)

        // Prompt is pinned to the first-line center: container.top + padding + lineH/2.
        // When the bar grows to multi-line, the first line stays at y = padding from
        // container top, so the prompt stays aligned with line 1 regardless of height.
        let promptCenterOffset = Self.verticalPadding + Self.lineHeight / 2

        NSLayoutConstraint.activate([
            // v3.1: Top hairline separator — 1px, edge-to-edge at bar top.
            separatorView.leadingAnchor.constraint(equalTo: leadingAnchor),
            separatorView.trailingAnchor.constraint(equalTo: trailingAnchor),
            separatorView.topAnchor.constraint(equalTo: topAnchor),
            separatorView.heightAnchor.constraint(equalToConstant: 1),

            // Container: edge-to-edge horizontally (v3.1 barHorizontalMargin=0),
            // fills bar vertically (v3.1 barTop/Bottom=0 preserved from v3).
            containerView.leadingAnchor.constraint(
                equalTo: leadingAnchor, constant: Self.barHorizontalMargin),
            containerView.trailingAnchor.constraint(
                equalTo: trailingAnchor, constant: -Self.barHorizontalMargin),
            containerView.topAnchor.constraint(equalTo: topAnchor, constant: Self.barTopMargin),
            containerView.bottomAnchor.constraint(
                equalTo: bottomAnchor, constant: -Self.barBottomMargin),

            // Prompt: 20pt from card's leading edge, vertically centered on first line.
            // Formula preserved from #240: centerY = container.top + verticalPadding + lineHeight/2
            // (constants shifted from 9 to 16, formula shape identical).
            promptLabel.leadingAnchor.constraint(
                equalTo: containerView.leadingAnchor, constant: Self.horizontalPadding),
            promptLabel.centerYAnchor.constraint(
                equalTo: containerView.topAnchor, constant: promptCenterOffset),
            promptLabel.widthAnchor.constraint(equalToConstant: 12),
            promptLabel.heightAnchor.constraint(equalToConstant: 12),

            // Scroll view: flush with container vertically (textContainerInset.height=16
            // handles vertical padding), 8pt gap from prompt glyph trailing, 20pt trailing margin.
            scrollView.leadingAnchor.constraint(
                equalTo: promptLabel.trailingAnchor, constant: 8),
            scrollView.trailingAnchor.constraint(
                equalTo: containerView.trailingAnchor, constant: -Self.horizontalPadding),
            scrollView.topAnchor.constraint(equalTo: containerView.topAnchor),
            scrollView.bottomAnchor.constraint(equalTo: containerView.bottomAnchor),
        ])

        // Wire text view callbacks
        textView.onEnter = { [weak self] in self?.handleEnter() }
        textView.onShiftEnter = { [weak self] in self?.handleShiftEnter() }
        textView.onEscape = { [weak self] in self?.handleEscape() }
        textView.onArrowUp = { [weak self] in self?.handleArrowUp() }
        textView.onArrowDown = { [weak self] in self?.handleArrowDown() }
        textView.onTab = { [weak self] in self?.handleTab() }
        textView.onCtrlKey = { [weak self] byte in self?.onCtrlKey?(byte) }
        textView.onFocusChange = { [weak self] focused in
            self?.updateFocusState(focused)
        }

        // Command dropdown
        commandDropdown.onSelect = { [weak self] cmd in
            self?.applyCommand(cmd)
        }

    }

    // MARK: - Height management

    private func recomputeHeight() {
        textView.layoutManager?.ensureLayout(for: textView.textContainer!)
        let used = textView.layoutManager?.usedRect(for: textView.textContainer!).size.height
            ?? Self.lineHeight
        // Container content height = used text area + symmetric vertical padding (textContainerInset).
        let contentHeight = max(Self.lineHeight, used) + Self.verticalPadding * 2
        // Total bar height = container + top margin + bottom margin (separator overlays top margin).
        let target = min(
            Self.maxBarHeight,
            max(Self.minBarHeight, contentHeight + Self.barTopMargin + Self.barBottomMargin))
        if abs(heightConstraint.constant - target) > 0.5 {
            NSAnimationContext.runAnimationGroup({ ctx in
                ctx.duration = 0.12
                ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
                ctx.allowsImplicitAnimation = true
                heightConstraint.animator().constant = target
            })
            onHeightChange?(target)
        }
    }

    // MARK: - Key handlers

    private func handleEnter() {
        // Dropdown takes priority — checked BEFORE the empty-text guard so that
        // if the user deleted back to empty while the dropdown was still showing,
        // Enter still commits the highlighted selection instead of silently no-op'ing.
        //
        // Note: this path only fires when OrchestratorTextView is first responder.
        // When NSPopover promotes its own window to key (stealing first responder),
        // DropdownTableView.keyDown handles Enter at the table level instead.
        // Both paths converge on applyCommand() → setText + hide().
        if commandDropdown.isShown {
            if let cmd = commandDropdown.confirmCurrent() {
                applyCommand(cmd)
                return
            }
            commandDropdown.hide()
            // Fall through to submit existing text (preserves prior behavior when
            // dropdown was shown but had no selection).
        }

        let text = textView.string
        let stripped = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !stripped.isEmpty else { return }

        history.add(text)
        onSubmit?(text)
        clearText()
    }

    private func handleShiftEnter() {
        textView.insertText("\n", replacementRange: textView.selectedRange())
    }

    private func handleEscape() {
        if commandDropdown.isShown {
            commandDropdown.hide()
            return
        }
        onEscape?()
    }

    private func handleArrowUp() {
        if commandDropdown.isShown {
            commandDropdown.selectPrevious()
            return
        }
        // Only navigate history if single-line (no newlines present)
        guard !textView.string.contains("\n") else {
            textView.moveUp(nil)
            return
        }
        if let prev = history.previous(currentDraft: textView.string) {
            setText(prev)
            moveCursorToEnd()
        }
    }

    private func handleArrowDown() {
        if commandDropdown.isShown {
            commandDropdown.selectNext()
            return
        }
        guard !textView.string.contains("\n") else {
            textView.moveDown(nil)
            return
        }
        if let nxt = history.next() {
            setText(nxt)
            moveCursorToEnd()
        }
    }

    private func handleTab() {
        if commandDropdown.isShown, let cmd = commandDropdown.confirmCurrent() {
            applyCommand(cmd)
            return
        }
        // Local autocomplete: if text starts with / and matches a single command → complete it
        let text = textView.string
        if text.hasPrefix("/"), !text.contains(" "), !text.contains("\n") {
            let matches = OrchestratorCommands.filter(
                OrchestratorCommands.commands(forCLI: commandDropdown.currentCLI),
                prefix: text)
            if matches.count == 1 {
                setText(matches[0].name + " ")
                moveCursorToEnd()
                commandDropdown.hide()
                return
            }
        }
        // Default: insert tab literal
        textView.insertText("\t", replacementRange: textView.selectedRange())
    }

    // MARK: - Command dropdown

    private func applyCommand(_ cmd: CLICommand) {
        // Replace current input with the selected command name + trailing space
        setText(cmd.name + " ")
        moveCursorToEnd()
        commandDropdown.hide()
    }

    private func maybeShowCommandDropdown() {
        let text = textView.string
        // Only trigger when first non-empty token starts with /
        let firstLine = text.split(separator: "\n").first.map(String.init) ?? text
        if firstLine.hasPrefix("/"), !firstLine.contains(" ") {
            commandDropdown.show(anchor: self, prefix: firstLine)
        } else {
            commandDropdown.hide()
        }
    }

    // MARK: - Text utilities

    private func setText(_ text: String) {
        textView.string = text
        textView.didChangeText()
        recomputeHeight()
        textView.needsDisplay = true
    }

    private func clearText() {
        textView.string = ""
        textView.didChangeText()
        history.resetNavigation()
        recomputeHeight()
        commandDropdown.hide()
    }

    private func moveCursorToEnd() {
        let end = (textView.string as NSString).length
        textView.setSelectedRange(NSRange(location: end, length: 0))
    }

    // MARK: - NSTextViewDelegate

    func textDidChange(_ notification: Notification) {
        recomputeHeight()
        maybeShowCommandDropdown()
        textView.needsDisplay = true
    }

    // MARK: - Focus state

    private func updateFocusState(_ focused: Bool) {
        isFocused = focused
        // v3.1: focus animation = prompt tint shift ONLY (200ms ease-out).
        // Background is transparent so no bg change needed. Border is α=0 so no border change.
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.20
            ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
            ctx.allowsImplicitAnimation = true
            promptLabel.contentTintColor = focused
                ? AtermTheme.inputBarPromptFocus
                : AtermTheme.inputBarPrompt.withAlphaComponent(0.65)
        }
    }

    /// v3.1: transparent container, flat rectangle (no radius), no border. Focus state
    /// is indicated by the prompt glyph tint change only (handled in updateFocusState).
    private func applyContainerStyle(focused: Bool) {
        guard let layer = containerView.layer else { return }
        layer.cornerRadius = Self.cornerRadius  // 0
        // Transparent — sidebar bg shows through naturally, no card elevation
        layer.backgroundColor = NSColor.clear.cgColor
        // NO border
        layer.borderWidth = 0
        layer.borderColor = nil
        // Shadow explicitly disabled
        layer.shadowColor = nil
        layer.shadowOpacity = 0
        layer.shadowRadius = 0
    }

    // MARK: - Public API

    func focus() {
        window?.makeFirstResponder(textView)
    }

    var hasFocus: Bool {
        window?.firstResponder === textView
    }

    func setCLI(_ cli: String) {
        commandDropdown.setCLI(cli)
    }

    // MARK: - Theme

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        separatorView.layer?.backgroundColor =
            AtermTheme.inputBarBorder.withAlphaComponent(Self.separatorAlpha).cgColor
        applyContainerStyle(focused: isFocused)
        promptLabel.contentTintColor = isFocused
            ? AtermTheme.inputBarPromptFocus
            : AtermTheme.inputBarPrompt.withAlphaComponent(0.65)
        textView.textColor = AtermTheme.textPrimary
    }
}

// MARK: - OrchestratorTextView (custom NSTextView subclass)

final class OrchestratorTextView: NSTextView {
    var onEnter: (() -> Void)?
    var onShiftEnter: (() -> Void)?
    var onEscape: (() -> Void)?
    var onArrowUp: (() -> Void)?
    var onArrowDown: (() -> Void)?
    var onTab: (() -> Void)?
    var onCtrlKey: ((UInt8) -> Void)?
    var onFocusChange: ((Bool) -> Void)?

    var placeholderText: String = ""
    var placeholderAttributes: [NSAttributedString.Key: Any] = [:]

    override var acceptsFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocusChange?(true) }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { onFocusChange?(false) }
        return ok
    }

    override func keyDown(with event: NSEvent) {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        // Ctrl+C / Ctrl+D / Ctrl+L → route to PTY
        if flags.contains(.control), !flags.contains(.command), !flags.contains(.option) {
            if let chars = event.charactersIgnoringModifiers?.lowercased() {
                switch chars {
                case "c":
                    onCtrlKey?(0x03)
                    return
                case "d":
                    onCtrlKey?(0x04)
                    return
                case "l":
                    onCtrlKey?(0x0C)
                    return
                default:
                    break
                }
            }
        }

        switch event.keyCode {
        case 36, 76:  // Return / numpad Enter
            if hasMarkedText() {
                // Korean IME composing — let the system finalize composition first.
                // After finalization, insertNewline: fires and handles submit.
                super.keyDown(with: event)
                return
            }
            if flags.contains(.shift) {
                onShiftEnter?()
            } else {
                onEnter?()
            }
            return
        case 53:  // ESC
            onEscape?()
            return
        case 126:  // Up
            if !flags.contains(.shift) && !flags.contains(.option) && !flags.contains(.command) {
                onArrowUp?()
                return
            }
        case 125:  // Down
            if !flags.contains(.shift) && !flags.contains(.option) && !flags.contains(.command) {
                onArrowDown?()
                return
            }
        case 48:  // Tab
            if !flags.contains(.shift) {
                onTab?()
                return
            }
        default:
            break
        }

        super.keyDown(with: event)
    }

    /// Catch Enter routed through key binding system (after IME composition finalization).
    /// When Korean IME has marked text, keyDown delegates to super which triggers
    /// interpretKeyEvents → insertText (finalize) → insertNewline. Without this override,
    /// NSTextView's default insertNewline inserts a literal newline character.
    override func insertNewline(_ sender: Any?) {
        onEnter?()
    }

    // Placeholder drawing. Aligned to current `textContainerInset` so the
    // placeholder sits exactly where the first-line baseline will land when the
    // user types — critical for the new vertically-centered cursor geometry.
    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty, !placeholderText.isEmpty else { return }
        let attributed = NSAttributedString(
            string: placeholderText, attributes: placeholderAttributes)
        let origin = NSPoint(
            x: textContainerInset.width + (textContainer?.lineFragmentPadding ?? 5),
            y: textContainerInset.height)
        attributed.draw(at: origin)
    }
}

// MARK: - ThinScroller (custom 3px always-visible scroller)

/// Minimal 3px-wide scroller for multi-line overflow in the orchestrator input bar.
/// Knob renders as a rounded pill in muted text color; track is transparent.
/// Used with `scrollerStyle = .legacy` + `autohidesScrollers = false` for persistent visibility.
final class ThinScroller: NSScroller {
    override class var isCompatibleWithOverlayScrollers: Bool { true }

    override class func scrollerWidth(
        for controlSize: NSControl.ControlSize, scrollerStyle: NSScroller.Style
    ) -> CGFloat {
        return 3
    }

    override func drawKnob() {
        let slot = rect(for: .knob)
        guard slot.height > 0 else { return }
        let knob = slot.insetBy(dx: 0, dy: 2)
        let path = NSBezierPath(roundedRect: knob, xRadius: 1.5, yRadius: 1.5)
        AtermTheme.textMuted.withAlphaComponent(0.4).setFill()
        path.fill()
    }

    override func drawKnobSlot(in slotRect: NSRect, highlight: Bool) {
        // Transparent track — draw nothing.
    }
}

// MARK: - NSFont italic helper

extension NSFont {
    func withTraits(_ traits: NSFontTraitMask) -> NSFont {
        let fm = NSFontManager.shared
        return fm.convert(self, toHaveTrait: traits)
    }
}
