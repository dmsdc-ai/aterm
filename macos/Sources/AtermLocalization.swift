import Foundation

enum AtermLocalization {
    static var languageCode: String {
        let preferred = Locale.preferredLanguages.first?.lowercased() ?? ""
        return preferred.hasPrefix("ko") ? "ko" : "en"
    }

    static var isKorean: Bool {
        languageCode == "ko"
    }

    static func text(ko: String, en: String) -> String {
        isKorean ? ko : en
    }
}
