import Foundation

/// Local history for orchestrator input bar with JSON persistence.
/// Mimics shell history (up/down arrow navigation, dedupe, max size).
final class OrchestratorHistory {
    private var items: [String] = []
    private var navIndex: Int = 0   // current nav position (items.count = at "new" line)
    private var draft: String = ""  // buffered draft when navigating
    private let maxItems = 500
    private let url: URL

    init() {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let dir = home.appendingPathComponent(".aigentry/data", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        url = dir.appendingPathComponent("orch-history.json")
        load()
        navIndex = items.count
    }

    /// Append a new entry. Removes duplicates (moves existing to end).
    func add(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        items.removeAll { $0 == trimmed }
        items.append(trimmed)
        if items.count > maxItems {
            items.removeFirst(items.count - maxItems)
        }
        navIndex = items.count
        draft = ""
        save()
    }

    /// ↑ — returns previous item, or nil if at top. `currentDraft` is preserved so ↓ can restore it.
    func previous(currentDraft: String) -> String? {
        guard !items.isEmpty else { return nil }
        if navIndex == items.count {
            draft = currentDraft  // buffer the in-progress draft
        }
        if navIndex > 0 {
            navIndex -= 1
            return items[navIndex]
        }
        return items[navIndex]
    }

    /// ↓ — returns next item, or empty/draft string at bottom. Nil if no history.
    func next() -> String? {
        guard !items.isEmpty else { return nil }
        if navIndex < items.count {
            navIndex += 1
        }
        if navIndex == items.count {
            return draft
        }
        return items[navIndex]
    }

    /// Reset navigation pointer to "new line" state.
    func resetNavigation() {
        navIndex = items.count
        draft = ""
    }

    var isNavigating: Bool { navIndex < items.count }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode([String].self, from: data)
        else { return }
        items = decoded
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(items) else { return }
        try? data.write(to: url, options: .atomic)
    }
}
