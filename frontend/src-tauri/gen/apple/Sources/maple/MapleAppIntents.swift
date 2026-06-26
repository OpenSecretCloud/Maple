import AppIntents
import UIKit

/// Siri Shortcut / App Intent that opens a new Maple chat.
///
/// This is a thin platform binding over the app's cross-platform deep-link
/// contract: it only builds and opens
///
///   cloud.opensecret.maple://new-chat?folder=<name>&web_search=on|off&message=<text>
///
/// All routing and behavior lives in the web frontend's deep-link handler, so
/// every parameter is optional — an empty intent just opens a fresh chat using
/// the user's persisted defaults (last-used model, web-search state, etc.).
@available(iOS 16.0, *)
struct OpenChatIntent: AppIntent {
    static var title: LocalizedStringResource = "Open Chat"
    static var description = IntentDescription(
        "Open a new Maple chat — optionally in a folder, with web search on or off, and a starting message."
    )

    /// Foreground the app so the running webview can handle the deep link.
    static var openAppWhenRun: Bool = true

    @Parameter(title: "Message", description: "Starting message to prefill the chat with.")
    var message: String?

    @Parameter(title: "Folder", description: "Name of the folder to open the chat in.")
    var folder: String?

    @Parameter(title: "Web Search", description: "Turn web search on or off for this chat.")
    var webSearch: Bool?

    @Parameter(title: "Send Immediately", description: "Send the message right away instead of leaving it in the composer.")
    var autoSend: Bool?

    @Parameter(title: "Start Voice Recording", description: "Begin recording a voice message as soon as the chat opens.")
    var voice: Bool?

    @MainActor
    func perform() async throws -> some IntentResult {
        var components = URLComponents()
        components.scheme = "cloud.opensecret.maple"
        components.host = "new-chat"

        var queryItems: [URLQueryItem] = []
        if let folder, !folder.isEmpty {
            queryItems.append(URLQueryItem(name: "folder", value: folder))
        }
        if let webSearch {
            queryItems.append(URLQueryItem(name: "web_search", value: webSearch ? "on" : "off"))
        }
        if let message, !message.isEmpty {
            queryItems.append(URLQueryItem(name: "message", value: message))
        }
        if let autoSend {
            queryItems.append(URLQueryItem(name: "auto_send", value: autoSend ? "on" : "off"))
        }
        if let voice {
            queryItems.append(URLQueryItem(name: "voice", value: voice ? "on" : "off"))
        }
        if !queryItems.isEmpty {
            components.queryItems = queryItems
        }

        if let url = components.url {
            await UIApplication.shared.open(url)
        }

        return .result()
    }
}

/// Registers the shortcut with Siri / Spotlight / the Shortcuts app.
@available(iOS 16.0, *)
struct MapleAppShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenChatIntent(),
            phrases: [
                "New chat in \(.applicationName)",
                "Start a \(.applicationName) chat",
                "Ask \(.applicationName)"
            ],
            shortTitle: "Open Chat",
            systemImageName: "bubble.left.and.bubble.right"
        )
    }
}
