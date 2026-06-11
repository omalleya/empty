import Foundation

/// One line on the shopping list.
struct ShoppingItem: Identifiable, Codable, Equatable {
    let id: UUID
    var name: String
    var isChecked: Bool

    init(id: UUID = UUID(), name: String, isChecked: Bool = false) {
        self.id = id
        self.name = name
        self.isChecked = isChecked
    }
}
