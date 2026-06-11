import Foundation

/// Result of a cart import.
struct SendResult {
    let sent: Int
    let message: String
}

enum CartBackendError: LocalizedError {
    case notConfigured
    case server(Int, String)

    var errorDescription: String? {
        switch self {
        case .notConfigured:
            return "No cart backend URL configured (set CartBackendURL in Info.plist)."
        case let .server(code, body):
            return "Backend error \(code): \(body)"
        }
    }
}

/// Hands the shopping list off to a backend that does the actual cart import.
protocol CartBackend {
    func send(items: [String]) async throws -> SendResult
}

/// Default backend: POSTs the list to your import service.
///
///   POST {baseURL}/cart/import
///   { "items": ["2% milk", "bananas", ...] }
///
/// The service resolves each line to a UPC and adds it to the Kroger cart — see
/// the Python `kroger` package at the repo root. Doing it server-side keeps the
/// family's Kroger OAuth tokens off the phone; the app only ever sends plain
/// text item names.
///
/// STUB: there's no server yet. Point `CartBackendURL` (Info.plist) at your
/// deployed endpoint, and make sure it accepts the JSON shape above.
struct HTTPCartBackend: CartBackend {
    var baseURL: URL?
    var session: URLSession

    init(baseURL: URL? = HTTPCartBackend.configuredURL(), session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session
    }

    /// Reads the backend URL from Info.plist (`CartBackendURL`), set in project.yml.
    static func configuredURL() -> URL? {
        guard
            let raw = Bundle.main.object(forInfoDictionaryKey: "CartBackendURL") as? String,
            !raw.isEmpty,
            let url = URL(string: raw)
        else { return nil }
        return url
    }

    func send(items: [String]) async throws -> SendResult {
        guard let baseURL else { throw CartBackendError.notConfigured }

        var request = URLRequest(url: baseURL.appendingPathComponent("cart/import"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(["items": items])

        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw CartBackendError.server(-1, "No HTTP response")
        }
        guard (200..<300).contains(http.statusCode) else {
            throw CartBackendError.server(http.statusCode, String(data: data, encoding: .utf8) ?? "")
        }
        return SendResult(sent: items.count, message: "Sent \(items.count) item(s) to your cart.")
    }
}
