import Foundation

/// Source of truth for the list. Persists to UserDefaults as JSON so the list
/// survives relaunches. A single shared instance is used by both the UI and the
/// Siri / App Intent path so a voice-added item shows up in the app.
@MainActor
final class ShoppingListStore: ObservableObject {
    static let shared = ShoppingListStore()

    @Published private(set) var items: [ShoppingItem] = []

    private let defaults: UserDefaults
    private let key = "shopping_items"

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        load()
    }

    func add(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        items.append(ShoppingItem(name: trimmed))
        save()
    }

    func remove(at offsets: IndexSet) {
        items.remove(atOffsets: offsets)
        save()
    }

    func remove(_ item: ShoppingItem) {
        items.removeAll { $0.id == item.id }
        save()
    }

    func toggle(_ item: ShoppingItem) {
        guard let i = items.firstIndex(where: { $0.id == item.id }) else { return }
        items[i].isChecked.toggle()
        save()
    }

    func clearChecked() {
        items.removeAll { $0.isChecked }
        save()
    }

    // MARK: - Persistence

    private func load() {
        guard
            let data = defaults.data(forKey: key),
            let decoded = try? JSONDecoder().decode([ShoppingItem].self, from: data)
        else { return }
        items = decoded
    }

    private func save() {
        if let data = try? JSONEncoder().encode(items) {
            defaults.set(data, forKey: key)
        }
    }
}
