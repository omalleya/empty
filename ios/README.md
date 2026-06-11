# Shopping List — iOS app

A small SwiftUI shopping list: add, remove (swipe), check off, clear checked,
and **dictate** items by voice. Includes a working **Siri / App Intent** so you
can say "Add milk to Shopping List" without opening the app.

Requires Xcode 15+ and iOS 17+ (App Intents need 16+; the UI uses a couple of
iOS 17 conveniences like `ContentUnavailableView`).

## Open it in Xcode

The repo ships source + an [XcodeGen](https://github.com/yonaskolb/XcodeGen)
spec rather than a checked-in `.xcodeproj` (pbxproj files are noisy and
merge-hostile).

```bash
brew install xcodegen
cd ios/ShoppingList
xcodegen generate
open ShoppingList.xcodeproj
```

Then pick a simulator (or your device) and Run.

**No XcodeGen?** Make a new Xcode project (iOS App, SwiftUI, name it
`ShoppingList`), delete its generated `ContentView`/`App` files, drag the files
in `Sources/` into the project, and add the two privacy strings from
`project.yml` (`NSMicrophoneUsageDescription`,
`NSSpeechRecognitionUsageDescription`) to the target's Info tab.

> Dictation and Siri don't work on the Simulator's microphone reliably — test
> voice features on a real device.

## Files

| File | Purpose |
|------|---------|
| `Sources/ShoppingListApp.swift` | `@main` app entry |
| `Sources/ContentView.swift` | the list UI + dictation entry bar |
| `Sources/ShoppingItem.swift` | the model (`Codable`) |
| `Sources/ShoppingListStore.swift` | state + JSON persistence (shared with Siri) |
| `Sources/SpeechRecognizer.swift` | live speech-to-text (Speech + AVAudioEngine) |
| `Sources/AddItemIntent.swift` | "add item" Siri intent + both spoken phrases |
| `Sources/SendListToCartIntent.swift` | "send list to cart" Siri intent |
| `Sources/CartBackend.swift` | posts the list to your import service (stub) |
| `project.yml` | XcodeGen project spec (targets, Info.plist, privacy strings) |

## Siri: yes, you can send commands to a specific app

The intent is already wired up. Build to a device, then say:

> "Hey Siri, add bananas to Shopping List"

See the chat notes / `Sources/AddItemIntent.swift` for how it works and what the
limits are. Short version: an app can expose **App Intents**, and Siri/Spotlight/
the Shortcuts app can invoke them by phrase — but the app has to declare the
intent; Siri can't pipe arbitrary free-form commands into an app that hasn't
opted in.

## Send to cart (the backend hand-off)

The ✈️ toolbar button — and "Hey Siri, send my list to Shopping List" — POST the
list to a backend you run:

```
POST {CartBackendURL}/cart/import
Content-Type: application/json

{ "items": ["2% milk", "bananas", "2 dozen eggs"] }
```

The backend resolves each line to a UPC and adds it to the Kroger cart — that's
exactly what the Python `kroger` package at the repo root does, so the service
is a thin HTTP wrapper around `kroger.shopping_list.import_list(...)`. Keeping it
server-side means the family's Kroger OAuth tokens never touch the phone; the app
only sends plain item names.

**This is a stub** — there's no server yet, and `CartBackendURL` is empty in
`project.yml` (so "Send to cart" reports "not configured" until you set it). To
wire it up: deploy a small endpoint in front of the `kroger` package, then set
`CartBackendURL` to its base URL. `CartBackend` is a protocol, so you can also
drop in a different implementation (direct API, on-device, a mock for previews).

## Status

Written against the documented APIs but **not compiled here** (no macOS/Xcode in
the authoring environment). Expect to resolve a stray warning or two on first
build. The logic — list/persist, dictation lifecycle, and the intent — is
complete.
