#if os(iOS) && DEBUG
    import Foundation
    import SwiftUI

    /// Synthetic local pages exercise the real link confirmation and browser.
    struct BrowserFixtureView: View {
        @State private var requested: URL?
        @State private var text = "Keep this browser draft"
        @State private var focused = false
        @State private var owner = UUID()
        @State private var presented = false
        private var destination: URL? {
            let arguments = ProcessInfo.processInfo.arguments
            guard let index = arguments.firstIndex(of: "--maple-browser-url"), index + 1 < arguments.count,
                let url = URL(string: arguments[index + 1]), url.scheme == "http", url.host == "127.0.0.1",
                url.user == nil, url.password == nil, url.path == "/one"
            else { return nil }
            return url
        }
        var body: some View {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        Text(presented ? "Chat covered" : "Chat visible").accessibilityIdentifier(
                            "browser.fixture.visibility")
                        if let destination {
                            MessageView(text: "[Local reference](\(destination.absoluteString))") { requested = $0 }
                        }
                        Button("Unsupported link") { requested = URL(string: "file:///synthetic") }
                            .accessibilityIdentifier("browser.fixture.unsupported")
                        ComposerEditor(
                            text: $text, focused: $focused, ownerID: owner,
                            placeholder: "Synthetic browser draft", accessibilityIdentifier: "browser.fixture.draft",
                            onSubmit: {}
                        )
                        .frame(minHeight: 80)
                    }.padding()
                }
            }
            .externalLinks($requested) { presented = $0 }
        }
    }
#endif
