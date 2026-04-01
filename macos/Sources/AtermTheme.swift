import AppKit
import SwiftUI

enum AtermThemeMode: UInt8 {
    case dark = 0
    case light = 1

    static func resolve(from appearance: NSAppearance?) -> Self {
        let bestMatch = appearance?.bestMatch(from: [.darkAqua, .aqua])
            ?? NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua])
        return bestMatch == .darkAqua ? .dark : .light
    }
}

enum AtermTheme {
    static let windowBackground = NSColor.atermDynamic(
        light: .atermHex(0xFAF6F0),
        dark: .atermHex(0x000000)
    )
    static let terminalBackground = NSColor.atermDynamic(
        light: .atermHex(0xFAF6F0),
        dark: .atermHex(0x000000)
    )
    static let terminalForeground = NSColor.atermDynamic(
        light: .atermHex(0x24292F),
        dark: .atermHex(0xE0E0E0)
    )
    static let sidebarBackground = NSColor.atermDynamic(
        light: .atermHex(0xF0EBE3),
        dark: .atermHex(0x0A0A0A)
    )
    static let sidebarHeaderBackground = NSColor.atermDynamic(
        light: .atermHex(0xE8E0D4),
        dark: .atermHex(0x0A0A0A)
    )
    static let panelBackground = NSColor.atermDynamic(
        light: .atermHex(0xFFFDFC),
        dark: .atermHex(0x0A0A0A)
    )
    static let panelInsetBackground = NSColor.atermDynamic(
        light: .atermHex(0xF7F2EC),
        dark: .atermHex(0x111111)
    )
    static let border = NSColor.atermDynamic(
        light: .atermHex(0xD8CEC1),
        dark: .atermHex(0x1A1A1A)
    )
    static let textPrimary = NSColor.atermDynamic(
        light: .atermHex(0x24292F),
        dark: .atermHex(0xE0E0E0)
    )
    static let textSecondary = NSColor.atermDynamic(
        light: .atermHex(0x57606A),
        dark: .atermHex(0x8B949E)
    )
    static let textMuted = NSColor.atermDynamic(
        light: .atermHex(0x6E7781),
        dark: .atermHex(0x6E7681)
    )
    static let accent = NSColor.atermDynamic(
        light: .atermHex(0xB45309),
        dark: .atermHex(0xD97706)
    )
    static let accentStrong = NSColor.atermDynamic(
        light: .atermHex(0xD97706),
        dark: .atermHex(0xF59E0B)
    )
    static let accentSubtle = NSColor.atermDynamic(
        light: .atermHex(0xD97706, alpha: 0.10),
        dark: .atermHex(0xD97706, alpha: 0.14)
    )
    static let accentBorder = NSColor.atermDynamic(
        light: .atermHex(0xD97706, alpha: 0.28),
        dark: .atermHex(0xD97706, alpha: 0.34)
    )
    static let statusSuccess = NSColor.atermDynamic(
        light: .atermHex(0x3DA85E),
        dark: .atermHex(0x56D364)
    )
    static let statusWarning = NSColor.atermDynamic(
        light: .atermHex(0xC4952E),
        dark: .atermHex(0xD4A574)
    )
    static let statusDanger = NSColor.atermDynamic(
        light: .atermHex(0xD45555),
        dark: .atermHex(0xFF7B72)
    )
    static let info = NSColor.atermDynamic(
        light: .atermHex(0x0E7490),
        dark: .atermHex(0x22D3EE)
    )
    static let gemini = NSColor.atermDynamic(
        light: .atermHex(0x6D28D9),
        dark: .atermHex(0xA78BFA)
    )
    static let secondaryRowBackground = NSColor.atermDynamic(
        light: .atermHex(0xFFFFFF, alpha: 0.82),
        dark: .atermHex(0xFFFFFF, alpha: 0.035)
    )
    static let selectedRowBackground = NSColor.atermDynamic(
        light: .atermHex(0xD97706, alpha: 0.12),
        dark: .atermHex(0xD97706, alpha: 0.14)
    )
    static let selectedRowStroke = NSColor.atermDynamic(
        light: .atermHex(0xD97706, alpha: 0.34),
        dark: .atermHex(0xD97706, alpha: 0.38)
    )

    static func mode(for appearance: NSAppearance?) -> AtermThemeMode {
        AtermThemeMode.resolve(from: appearance)
    }
}

extension NSColor {
    static func atermDynamic(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { appearance in
            AtermThemeMode.resolve(from: appearance) == .dark ? dark : light
        }
    }

    static func atermHex(_ hex: UInt32, alpha: CGFloat = 1.0) -> NSColor {
        NSColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255.0,
            green: CGFloat((hex >> 8) & 0xFF) / 255.0,
            blue: CGFloat(hex & 0xFF) / 255.0,
            alpha: alpha
        )
    }

    func atermResolvedCGColor(with appearance: NSAppearance) -> CGColor {
        var resolved = (self.usingColorSpace(.sRGB) ?? self).cgColor
        appearance.performAsCurrentDrawingAppearance {
            resolved = (self.usingColorSpace(.sRGB) ?? self).cgColor
        }
        return resolved
    }
}
