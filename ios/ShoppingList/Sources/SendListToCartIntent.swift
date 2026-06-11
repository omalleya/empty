import AppIntents

/// Siri / Shortcuts: "Send my list to Shopping List".
///
/// Grabs the current items and hands them to the cart backend (which resolves
/// them to products and adds them to the store cart). Runs in the background.
struct SendListToCartIntent: AppIntent {
    static var title: LocalizedStringResource = "Send List to Cart"
    static var description = IntentDescription("Sends your shopping list to your store cart.")
    static var openAppWhenRun: Bool = false

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let names = ShoppingListStore.shared.items.map(\.name)
        guard !names.isEmpty else {
            return .result(dialog: "Your shopping list is empty.")
        }

        let backend: CartBackend = HTTPCartBackend()
        do {
            let result = try await backend.send(items: names)
            return .result(dialog: IntentDialog(stringLiteral: result.message))
        } catch let error as CartBackendError {
            return .result(dialog: IntentDialog(stringLiteral: error.localizedDescription))
        }
    }
}
