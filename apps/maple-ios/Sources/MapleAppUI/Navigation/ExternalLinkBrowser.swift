#if os(iOS)
    import MapleAppCore
    import Observation
    import SwiftUI
    import WebKit

    extension View {
        func externalLinks(
            _ requested: Binding<URL?>, onPresentationChange: @escaping (Bool) -> Void = { _ in }
        ) -> some View {
            modifier(ExternalLinkPresentation(requested: requested, onPresentationChange: onPresentationChange))
        }
    }

    private struct BrowserDestination: Identifiable {
        let id = UUID()
        let url: URL
    }

    private struct ExternalLinkPresentation: ViewModifier {
        @Binding var requested: URL?
        let onPresentationChange: (Bool) -> Void
        @State private var browser: BrowserDestination?
        @Environment(\.openURL) private var openURL

        func body(content: Content) -> some View {
            content
                .alert(
                    MapleStrings.text("Open external link?"),
                    isPresented: Binding(
                        get: { requested != nil }, set: { if !$0 { requested = nil } }
                    ), presenting: requested
                ) { url in
                    if ExternalLinkPolicy.isWeb(url) || ExternalLinkPolicy.isMail(url) {
                        Button(MapleStrings.text("Open link")) {
                            requested = nil
                            if ExternalLinkPolicy.isWeb(url) {
                                browser = BrowserDestination(url: url)
                            } else {
                                openURL(url)
                            }
                        }.accessibilityIdentifier("link.confirm")
                    }
                    Button(MapleStrings.text("Cancel"), role: .cancel) { requested = nil }
                } message: { url in
                    if ExternalLinkPolicy.isWeb(url) || ExternalLinkPolicy.isMail(url) {
                        Text(MapleStrings.externalLinkMessage(url.absoluteString))
                    } else {
                        Text(MapleStrings.text("This link type cannot be opened by Maple."))
                    }
                }
                .sheet(item: $browser) { destination in LinkBrowser(url: destination.url) }
                .onChange(of: requested != nil || browser != nil, initial: true) { _, presented in
                    onPresentationChange(presented)
                }
                .onDisappear { onPresentationChange(false) }
        }
    }

    /// The web view is only a user-requested external browser. It never receives
    /// account tokens, conversation data, script messages, or an application bridge.
    @MainActor @Observable private final class LinkBrowserModel: NSObject, WKNavigationDelegate, WKUIDelegate {
        let webView: WKWebView
        let initialURL: URL
        var currentURL: URL
        var canGoBack = false
        var canGoForward = false
        var isLoading = false
        var failed = false
        var externalLink: URL?
        @ObservationIgnored private var observations: [NSKeyValueObservation] = []

        init(url: URL) {
            initialURL = url; currentURL = url
            let configuration = WKWebViewConfiguration()
            configuration.websiteDataStore = .nonPersistent()
            configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
            webView = WKWebView(frame: .zero, configuration: configuration)
            super.init()
            webView.navigationDelegate = self; webView.uiDelegate = self
            webView.allowsBackForwardNavigationGestures = true
            observations = [
                webView.observe(\.url, options: [.new]) { [weak self] _, _ in
                    Task { @MainActor [weak self] in self?.refresh() }
                },
                webView.observe(\.isLoading, options: [.new]) { [weak self] _, _ in
                    Task { @MainActor [weak self] in self?.refresh() }
                },
                webView.observe(\.canGoBack, options: [.new]) { [weak self] _, _ in
                    Task { @MainActor [weak self] in self?.refresh() }
                },
                webView.observe(\.canGoForward, options: [.new]) { [weak self] _, _ in
                    Task { @MainActor [weak self] in self?.refresh() }
                },
            ]
            if ExternalLinkPolicy.isWeb(url) { webView.load(URLRequest(url: url)) }
        }

        func close() {
            webView.stopLoading(); observations.removeAll()
            webView.navigationDelegate = nil; webView.uiDelegate = nil
        }

        private func refresh() {
            if let url = webView.url, ExternalLinkPolicy.isWeb(url) { currentURL = url }
            isLoading = webView.isLoading
            canGoBack = webView.canGoBack; canGoForward = webView.canGoForward
        }

        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction) async
            -> WKNavigationActionPolicy
        {
            guard let url = navigationAction.request.url else { return .cancel }
            if ExternalLinkPolicy.isWeb(url) { return .allow }
            if navigationAction.navigationType == .linkActivated, ExternalLinkPolicy.isMail(url) {
                externalLink = url
            }
            return .cancel
        }

        func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse) async
            -> WKNavigationResponsePolicy
        {
            guard let url = navigationResponse.response.url, ExternalLinkPolicy.isWeb(url),
                navigationResponse.canShowMIMEType
            else { failed = true; return .cancel }
            return .allow
        }

        func webView(
            _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            if navigationAction.targetFrame == nil, let url = navigationAction.request.url,
                ExternalLinkPolicy.isWeb(url)
            {
                webView.load(URLRequest(url: url))
            }
            return nil
        }

        func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) { failed = false }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { refresh() }
        func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error
        ) {
            if (error as NSError).code != NSURLErrorCancelled { failed = true }
        }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            if (error as NSError).code != NSURLErrorCancelled { failed = true }
        }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { failed = true }
    }

    private struct ExternalWebContent: UIViewRepresentable {
        let model: LinkBrowserModel
        func makeUIView(context: Context) -> WKWebView { model.webView }
        func updateUIView(_ uiView: WKWebView, context: Context) {}
    }

    private struct LinkBrowser: View {
        @State private var model: LinkBrowserModel
        @Environment(\.dismiss) private var dismiss
        @Environment(\.openURL) private var openURL
        init(url: URL) { _model = State(initialValue: LinkBrowserModel(url: url)) }

        var body: some View {
            NavigationStack {
                VStack(spacing: 0) {
                    if model.failed {
                        HStack {
                            Text(MapleStrings.text("This page couldn’t be loaded."))
                            Button(MapleStrings.text("Try again")) {
                                model.webView.load(URLRequest(url: model.currentURL))
                            }
                        }.font(.footnote).padding()
                    }
                    ExternalWebContent(model: model)
                }
                .navigationTitle(model.currentURL.host() ?? "")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button(MapleStrings.text("Done")) { dismiss() }.accessibilityIdentifier("browser.close")
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button(MapleStrings.text("Open in default browser"), systemImage: "arrow.up.right.square") {
                            guard ExternalLinkPolicy.isWeb(model.currentURL) else { return }
                            openURL(model.currentURL)
                        }.accessibilityIdentifier("browser.external")
                    }
                    ToolbarItemGroup(placement: .bottomBar) {
                        Button(MapleStrings.text("Back"), systemImage: "chevron.backward") { model.webView.goBack() }
                            .disabled(
                                !model.canGoBack)
                        Button(MapleStrings.text("Forward"), systemImage: "chevron.forward") {
                            model.webView.goForward()
                        }.disabled(
                            !model.canGoForward)
                        Spacer()
                        Button(MapleStrings.text("Reload"), systemImage: "arrow.clockwise") { model.webView.reload() }
                    }
                }
                .externalLinks($model.externalLink)
            }
            .onDisappear { model.close() }
        }
    }
#endif
