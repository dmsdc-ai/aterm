import Foundation

enum AtermLocalization {
    /// Task #246: forced English default. Korean string literals at call sites
    /// are preserved for future multi-lang revival — re-wire this getter to
    /// read OS locale or a user preference key when that returns.
    static var languageCode: String { "en" }

    static var isKorean: Bool { false }

    static func text(ko: String, en: String) -> String { en }
}
