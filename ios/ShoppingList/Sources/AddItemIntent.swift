import AppIntents

/// Siri / Shortcuts entry point: "Add milk to Shopping List".
///
/// This is the modern way (iOS 16+, App Intents) to let Siri send a command to
/// *this* app. The intent runs in the background (no app launch needed),
/// appends to the shared store, and speaks a confirmation.
struct AddItemIntent: AppIntent {
    static var title: LocalizedStringResource = "Add to Shopping List"
    static var description = IntentDescription("Adds an item to your shopping list.")

    /// Run without bringing the app to the foreground.
    static var openAppWhenRun: Bool = false

    @Parameter(title: "Item", requestValueDialog: "What should I add?")
    var item: String

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        ShoppingListStore.shared.add(item)
        return .result(dialog: "Added \(item) to your shopping list.")
    }
}

/// Registers spoken phrases with Siri/Spotlight automatically on install — no
/// per-user setup. Phrases must include the app name (`\(.applicationName)`).
/// A target may have exactly one AppShortcutsProvider, so both intents live here.
struct ShoppingListShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: AddItemIntent(),
            phrases: [
                "Add \(\.$item) to \(.applicationName)",
                "Add \(\.$item) to my list in \(.applicationName)"
            ],
            shortTitle: "Add Item",
            systemImageName: "cart.badge.plus"
        )
        AppShortcut(
            intent: SendListToCartIntent(),
            phrases: [
                "Send my list to \(.applicationName)",
                "Send my shopping list to the cart in \(.applicationName)"
            ],
            shortTitle: "Send to Cart",
            systemImageName: "paperplane"
        )
    }
}
