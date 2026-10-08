#if os(iOS)
    import Foundation
    #if DEBUG
        import Observation
    #endif
    import SwiftUI
    import Textual
    import UIKit
    import WebKit

    /// Equation snapshots are in memory only and owned by one message revision.
    /// The WebKit process never receives a transcript, account, URL, or credential.
    @MainActor
    final class EquationRenderer {
        enum Failure: Error { case rejected, unavailable, timedOut, cancelled }
        private var webView: WKWebView?
        private let assets = EquationAssets()
        private let navigation = EquationNavigation()
        private var stopped = false
        private weak var host: EquationLifecycleWindowHost?
        private weak var attachedWindow: UIWindow?
        private var container: UIView?
        private var pendingMount: (() -> Void)?

        init(host: EquationLifecycleWindowHost) { self.host = host }

        #if DEBUG
            private var diagnostic: EquationPhaseDiagnostic?
            /// Only the fixed synthetic lifecycle fixture supplies this observer.
            init(
                diagnostic: EquationPhaseDiagnostic, host: EquationLifecycleWindowHost
            ) {
                self.diagnostic = diagnostic
                self.host = host
                assets.diagnostic = diagnostic
                navigation.diagnostic = diagnostic
            }
        #endif

        func render(_ formulas: [MessageFormula]) async throws -> [String: EquationAttachment] {
            guard !formulas.isEmpty else { return [:] }
            let permit = try await EquationWorkAdmission.shared.acquire()
            defer { invalidate(); EquationWorkAdmission.shared.release(permit) }
            let web = try await startSession()
            var results: [String: EquationAttachment] = [:]
            var totalPixels = 0
            let deadline = ContinuousClock.now.advanced(by: .seconds(8))
            for formula in formulas where formula.isWithinBudget {
                try Task.checkCancellation()
                guard !stopped, ContinuousClock.now < deadline else { break }
                #if DEBUG
                    diagnostic?.begin(.metric)
                #endif
                let metric: EquationMetric? = try await bounded { finish in
                    web.callAsyncJavaScript(
                        "return await window.mapleEquation(source, display)",
                        arguments: ["source": formula.latex, "display": formula.display],
                        in: nil, in: .page
                    ) { result in
                        switch result {
                        case .success(let value): finish(.success(EquationMetric(value)))
                        case .failure: finish(.success(nil))
                        }
                    }
                }
                try Task.checkCancellation()
                guard let metric else { continue }
                totalPixels += Int(metric.width * metric.height * 4)
                guard totalPixels <= 4_194_304 else { break }
                // Reserve aggregate decoded cost before materializing an image.
                guard let lease = EquationRasterBudget.shared.reserve(bytes: Int(metric.width * metric.height * 16))
                else { break }
                web.frame.size = CGSize(width: metric.width, height: metric.height)
                let options = WKSnapshotConfiguration()
                options.rect = CGRect(x: 0, y: 0, width: metric.width, height: metric.height)
                options.snapshotWidth = NSNumber(value: metric.width * 2)
                options.afterScreenUpdates = false
                #if DEBUG
                    diagnostic?.begin(.snapshot)
                #endif
                let png = try await snapshot(web, options: options, lease: lease)
                try Task.checkCancellation()
                guard !stopped else { throw Failure.cancelled }
                results[formula.id] = EquationAttachment(
                    source: formula.original, png: png, lease: lease,
                    width: metric.width, height: metric.height, descent: metric.descent)
            }
            return results
        }

        private func snapshot(
            _ web: WKWebView, options: WKSnapshotConfiguration, lease: EquationRasterLease
        ) async throws -> Data {
            try Task.checkCancellation()
            guard !stopped, webView === web, let ownerHost = host,
                let ownerWindow = attachedWindow, let ownerContainer = container,
                ownerHost.renderer === self, ownerHost.window === ownerWindow,
                ownerContainer.superview === ownerHost, ownerContainer.window === ownerWindow,
                web.superview === ownerContainer, web.window === ownerWindow,
                ownerWindow.windowScene?.activationState == .foregroundActive
            else { throw Failure.cancelled }

            // Capture the page without the offscreen window's render-server surface.
            // Only this renderer's web view is detached; its lifecycle host stays owned.
            web.removeFromSuperview()
            let ownsCapture: @MainActor @Sendable () -> Bool = { [weak self] in
                guard let self else { return false }
                return !stopped && webView === web && host === ownerHost
                    && attachedWindow === ownerWindow && container === ownerContainer
                    && ownerHost.renderer === self && ownerHost.window === ownerWindow
                    && ownerContainer.superview === ownerHost && ownerContainer.window === ownerWindow
                    && ownerWindow.windowScene?.activationState == .foregroundActive
                    && web.superview == nil && web.window == nil
            }
            guard ownsCapture() else { throw Failure.cancelled }
            let png: Data = try await bounded { finish in
                web.takeSnapshot(with: options) { image, error in
                    // A cancelled capture keeps its raster reservation until WebKit
                    // releases this callback, and cannot restore or publish late pixels.
                    withExtendedLifetime(lease) {
                        guard ownsCapture() else { finish(.failure(Failure.cancelled)); return }
                        guard error == nil, let png = image?.pngData(), png.count <= 4_194_304 else {
                            finish(.failure(Failure.unavailable)); return
                        }
                        finish(.success(png))
                    }
                }
            }
            try Task.checkCancellation()
            guard ownsCapture() else { throw Failure.cancelled }
            ownerContainer.addSubview(web)
            guard web.superview === ownerContainer, web.window === ownerWindow else {
                web.removeFromSuperview()
                throw Failure.cancelled
            }
            return png
        }

        private func startSession() async throws -> WKWebView {
            guard let html = EquationResourcePolicy.bootstrapHTML() else { throw Failure.unavailable }
            #if DEBUG
                diagnostic?.begin(.rules)
            #endif
            let rules: EquationRules = try await bounded { finish in
                WKContentRuleListStore.default().compileContentRuleList(
                    forIdentifier: "maple-equation-deny-network-v1",
                    encodedContentRuleList: EquationResourcePolicy.contentRules
                ) { list, _ in
                    if let list { finish(.success(EquationRules(list))) } else { finish(.failure(Failure.unavailable)) }
                }
            }
            try Task.checkCancellation()
            guard !stopped else { throw Failure.cancelled }
            let config = WKWebViewConfiguration()
            config.websiteDataStore = .nonPersistent()
            config.setURLSchemeHandler(assets, forURLScheme: EquationResourcePolicy.scheme)
            config.userContentController.add(rules.value)
            config.preferences.javaScriptCanOpenWindowsAutomatically = false
            config.allowsInlineMediaPlayback = false
            config.mediaTypesRequiringUserActionForPlayback = .all
            let web = WKWebView(frame: CGRect(x: 0, y: 0, width: 1024, height: 1), configuration: config)
            web.isOpaque = false
            web.backgroundColor = .clear
            web.underPageBackgroundColor = .clear
            web.scrollView.backgroundColor = .clear
            web.scrollView.isScrollEnabled = false
            web.isInspectable = false
            web.navigationDelegate = navigation
            web.uiDelegate = navigation
            webView = web
            #if DEBUG
                diagnostic?.begin(.navigation)
            #endif
            try await bounded(timeout: .seconds(12)) { finish in
                navigation.loaded = finish
                guard let host, host.renderer == nil else { finish(.failure(Failure.unavailable)); return }
                host.renderer = self
                // Initial SwiftUI mounting shares the existing navigation deadline.
                // One callback is cancelled with this renderer; no polling or retry.
                pendingMount = { [weak self, weak web] in
                    guard let self, let web, !stopped, let host = self.host,
                        host.renderer === self, let window = host.window,
                        window.windowScene?.activationState == .foregroundActive,
                        !window.bounds.isEmpty
                    else { return }
                    pendingMount = nil
                    #if DEBUG
                        diagnostic?.record(.hostFound)
                    #endif
                    let container = UIView(frame: Self.outsideFrame(host: host, window: window))
                    container.isUserInteractionEnabled = false
                    container.isAccessibilityElement = false
                    container.accessibilityElementsHidden = true
                    container.clipsToBounds = true
                    self.container = container
                    host.addSubview(container)
                    container.addSubview(web)
                    guard container.window === window, web.window === window else {
                        finish(.failure(Failure.unavailable)); invalidate(); return
                    }
                    attachedWindow = window
                    #if DEBUG
                        diagnostic?.record(.hostAttached)
                        let pending = web.loadHTMLString(html, baseURL: nil)
                        if pending == nil { diagnostic?.record(.loadReturnedNil) }
                    #else
                        web.loadHTMLString(html, baseURL: nil)
                    #endif
                }
                resumeMount()
            } as Void
            return web
        }

        fileprivate func resumeMount() { pendingMount?() }

        fileprivate func hostMovedToWindow() {
            if container != nil || host?.window == nil { invalidate() } else { resumeMount() }
        }

        fileprivate func updateHostLayout() {
            guard let container else { return }
            guard let host, let window = attachedWindow, host.window === window,
                window.windowScene?.activationState == .foregroundActive
            else { invalidate(); return }
            let frame = Self.outsideFrame(host: host, window: window)
            if container.frame != frame { container.frame = frame }
        }

        private static func outsideFrame(host: UIView, window: UIWindow) -> CGRect {
            host.convert(
                CGRect(x: window.bounds.maxX + 1, y: window.bounds.maxY + 1, width: 4096, height: 2048),
                from: window)
        }

        #if DEBUG
            /// Synthetic fixture only; loopback-only positive-control canary. This
            /// is compiled out of release and is never reachable from message data.
            func exerciseContainmentForTesting(endpoint: URL) async throws -> Bool {
                guard endpoint.scheme == "http", endpoint.host == "127.0.0.1", endpoint.port != nil,
                    endpoint.user == nil, endpoint.password == nil, endpoint.query == nil
                else { throw Failure.rejected }
                let permit = try await EquationWorkAdmission.shared.acquire()
                defer { invalidate(); EquationWorkAdmission.shared.release(permit) }
                let web = try await startSession()
                return try await bounded { finish in
                    web.callAsyncJavaScript(
                        Self.containmentProbe, arguments: ["endpoint": endpoint.absoluteString], in: nil, in: .page
                    ) { result in
                        if case .success(let value) = result {
                            finish(.success((value as? Bool) == true))
                        } else {
                            finish(.failure(Failure.unavailable))
                        }
                    }
                }
            }

            private static let containmentProbe = #"""
                const pending = [];
                const attempt = body => { try { body(); } catch (_) {} };
                attempt(() => pending.push(fetch(endpoint + '/fetch').catch(() => {})));
                attempt(() => pending.push(fetch(endpoint.replace('http:', 'https:') + '/https').catch(() => {})));
                attempt(() => { const x = new XMLHttpRequest(); x.open('GET', endpoint + '/xhr'); x.timeout = 500; x.send(); });
                attempt(() => { const w = new WebSocket(endpoint.replace('http:', 'ws:') + '/websocket'); w.onerror = () => {}; });
                attempt(() => { const i = new Image(); i.src = endpoint + '/image'; document.body.append(i); });
                attempt(() => { const i = new Image(); i.src = endpoint.replace('http:', '') + '/relative-image'; document.body.append(i); });
                attempt(() => { const s = document.createElement('link'); s.rel = 'stylesheet'; s.href = endpoint + '/stylesheet'; document.head.append(s); });
                attempt(() => { const s = document.createElement('style'); s.textContent = '@import url("' + endpoint + '/import");'; document.head.append(s); });
                attempt(() => pending.push(new FontFace('probe', 'url("' + endpoint + '/font")').load().catch(() => {})));
                attempt(() => { const f = document.createElement('iframe'); f.src = endpoint + '/frame'; document.body.append(f); });
                attempt(() => navigator.sendBeacon(endpoint + '/beacon', 'synthetic'));
                attempt(() => { const f = document.createElement('form'); f.action = endpoint + '/form'; document.body.append(f); f.submit(); });
                attempt(() => window.open(endpoint + '/popup'));
                attempt(() => { const a = document.createElement('a'); a.href = endpoint + '/download'; a.download = 'synthetic'; document.body.append(a); a.click(); });
                await Promise.race([Promise.allSettled(pending), new Promise(resolve => setTimeout(resolve, 500))]);
                attempt(() => { window.location.href = endpoint + '/navigation'; });
                return true;
                """#
        #endif

        func invalidate() {
            stopped = true
            pendingMount = nil
            navigation.loaded?(.failure(Failure.cancelled))
            navigation.loaded = nil
            webView?.stopLoading()
            webView?.navigationDelegate = nil
            webView?.uiDelegate = nil
            webView?.configuration.userContentController.removeAllUserScripts()
            webView?.removeFromSuperview()
            if let container {
                container.removeFromSuperview()
                #if DEBUG
                    if container.superview == nil, webView?.window == nil { diagnostic?.record(.hostDetached) }
                #endif
                self.container = nil
            }
            if host?.renderer === self { host?.renderer = nil }
            attachedWindow = nil
            webView = nil
        }

        private func bounded<Value: Sendable>(
            timeout: Duration = .seconds(3),
            _ start: (@escaping @MainActor (Result<Value, Error>) -> Void) -> Void
        ) async throws -> Value {
            let gate = EquationOperation<Value>()
            #if DEBUG
                let observer = diagnostic
                let phase = observer?.phase
                let navigationBegan = phase == .navigation ? ContinuousClock.now : nil
                defer {
                    // Observe the winning result only after the await resolves.
                    // A late callback has no continuation and cannot replace it.
                    if let phase { observer?.finish(phase, outcome: gate.outcome) }
                    if let navigationBegan, let finished = gate.finishedAt {
                        observer?.recordNavigationLatency(
                            navigationBegan.duration(to: finished), deadline: timeout)
                    }
                }
            #endif
            return try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { continuation in
                    gate.continuation = continuation
                    guard !Task.isCancelled, !stopped else {
                        gate.finish(.failure(Failure.cancelled)); return
                    }
                    gate.timer = Task { @MainActor [weak self] in
                        do { try await Task.sleep(for: timeout) } catch { return }
                        gate.finish(.failure(Failure.timedOut))
                        self?.invalidate()
                    }
                    start { result in gate.finish(result) }
                }
            } onCancel: {
                Task { @MainActor [weak self] in
                    gate.finish(.failure(Failure.cancelled))
                    self?.invalidate()
                }
            }
        }
    }

    /// Each MessageView mounts exactly one noninteractive host in its own window.
    /// It holds no message data and never searches application or scene windows.
    @MainActor
    final class EquationLifecycleWindowHost: UIView {
        weak var renderer: EquationRenderer?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            renderer?.hostMovedToWindow()
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            renderer?.updateHostLayout()
        }

        func resumeMount() {
            renderer?.resumeMount()
            renderer?.updateHostLayout()
        }
    }

    struct EquationLifecycleWindowAnchor: UIViewRepresentable {
        let host: EquationLifecycleWindowHost

        func makeUIView(context: Context) -> EquationLifecycleWindowHost {
            host.isUserInteractionEnabled = false
            host.isAccessibilityElement = false
            host.accessibilityElementsHidden = true
            return host
        }

        func updateUIView(_ uiView: EquationLifecycleWindowHost, context: Context) { uiView.resumeMount() }

        static func dismantleUIView(_ uiView: EquationLifecycleWindowHost, coordinator: ()) {
            uiView.renderer?.invalidate()
        }
    }

    @MainActor
    private final class EquationRules {
        let value: WKContentRuleList
        init(_ value: WKContentRuleList) { self.value = value }
    }

    @MainActor
    private final class EquationOperation<Value: Sendable> {
        var continuation: CheckedContinuation<Value, Error>?
        var timer: Task<Void, Never>?
        #if DEBUG
            private(set) var outcome = EquationPhaseDiagnostic.Outcome.none
            private(set) var finishedAt: ContinuousClock.Instant?
        #endif
        func finish(_ result: Result<Value, Error>) {
            let pending = continuation
            #if DEBUG
                if pending != nil {
                    finishedAt = ContinuousClock.now
                    switch result {
                    case .success: outcome = .completed
                    case .failure(let error):
                        switch error {
                        case EquationRenderer.Failure.timedOut: outcome = .timedOut
                        case EquationRenderer.Failure.cancelled, is CancellationError: outcome = .cancelled
                        default: outcome = .failed
                        }
                    }
                }
            #endif
            continuation = nil
            timer?.cancel()
            timer = nil
            pending?.resume(with: result)
        }
    }

    #if DEBUG
        /// Fixed numeric observations only; no source, URL, identity or error payload.
        @MainActor @Observable
        final class EquationPhaseDiagnostic {
            enum Phase: Int { case none, rules, navigation, metric, snapshot }
            enum Outcome: Int { case none, completed, timedOut, cancelled, failed }
            enum Detail: Int, CaseIterable {
                case actions, actionAllowed, actionDenied, responseAllowed, responseDenied
                case didStart, didCommit, didFinish, didFail, provisionalFailed, processTerminated
                case loadReturnedNil, assetStarted, assetFinished, assetRejected
                case hostFound, hostAttached, hostDetached
            }

            private var details = Array(repeating: 0, count: Detail.allCases.count)
            private(set) var detailSaturated = false
            private(set) var lastRejectionBits = 0
            private(set) var rejectionBits = 0

            func record(_ detail: Detail) {
                guard details[detail.rawValue] < 1_024 else { detailSaturated = true; return }
                details[detail.rawValue] += 1
            }

            func count(_ detail: Detail) -> Int { details[detail.rawValue] }

            func action(allowed: Bool, started: Bool, bootstrap: Bool, mainFrame: Bool, other: Bool) {
                record(.actions)
                record(allowed ? .actionAllowed : .actionDenied)
                guard !allowed else { return }
                // Bits identify failed terms of the unchanged admission predicate.
                lastRejectionBits =
                    (started ? 1 : 0) | (bootstrap ? 0 : 2)
                    | (mainFrame ? 0 : 4) | (other ? 0 : 8)
                rejectionBits |= lastRejectionBits
            }

            private(set) var phase = Phase.none
            private(set) var lastCompleted = Phase.none
            private(set) var timedOutPhase = Phase.none
            private(set) var outcome = Outcome.none
            private var starts = [0, 0, 0, 0, 0]
            private var completions = [0, 0, 0, 0, 0]
            private(set) var navigationDeadlineMilliseconds = 0
            private(set) var navigationElapsedMilliseconds = 0
            private(set) var navigationCrossedOriginalDeadline = false
            private(set) var navigationElapsedCapped = false

            func recordNavigationLatency(_ elapsed: Duration, deadline: Duration) {
                let deadlineParts = deadline.components
                let deadlineMilliseconds =
                    Double(deadlineParts.seconds) * 1_000
                    + Double(deadlineParts.attoseconds) / 1_000_000_000_000_000
                navigationDeadlineMilliseconds = Int(min(max(deadlineMilliseconds, 0), 60_000))
                let parts = elapsed.components
                let milliseconds = Double(parts.seconds) * 1_000 + Double(parts.attoseconds) / 1_000_000_000_000_000
                navigationElapsedMilliseconds = Int(min(max(milliseconds, 0), 60_000))
                // Retain the original deadline as a historical diagnostic comparator.
                navigationCrossedOriginalDeadline = elapsed >= .seconds(3)
                navigationElapsedCapped = milliseconds > 60_000
            }

            func begin(_ phase: Phase) {
                self.phase = phase
                outcome = .none
                starts[phase.rawValue] += 1
            }

            func finish(_ phase: Phase, outcome: Outcome) {
                self.outcome = outcome
                if outcome == .completed {
                    lastCompleted = phase
                    completions[phase.rawValue] += 1
                } else if outcome == .timedOut {
                    timedOutPhase = phase
                }
            }

            func started(_ phase: Phase) -> Int { starts[phase.rawValue] }
            func completed(_ phase: Phase) -> Int { completions[phase.rawValue] }
        }
    #endif

    private struct EquationMetric: Sendable {
        let width: Double, height: Double, descent: Double
        init?(_ raw: Any?) {
            guard let object = raw as? [String: NSNumber],
                let width = object["width"]?.doubleValue, let height = object["height"]?.doubleValue,
                let descent = object["descent"]?.doubleValue,
                width.isFinite, height.isFinite, descent.isFinite,
                (1...4096).contains(width), (1...2048).contains(height),
                (0...height).contains(descent)
            else { return nil }
            self.width = width; self.height = height; self.descent = descent
        }
    }

    @MainActor
    private final class EquationAssets: NSObject, WKURLSchemeHandler {
        #if DEBUG
            weak var diagnostic: EquationPhaseDiagnostic?
        #endif
        func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
            guard let url = urlSchemeTask.request.url, let path = EquationResourcePolicy.path(for: url),
                let root = Bundle.module.url(forResource: "EquationRenderer", withExtension: nil),
                let data = try? Data(contentsOf: root.appendingPathComponent(path)), data.count <= 1_048_576
            else {
                #if DEBUG
                    diagnostic?.record(.assetRejected)
                #endif
                urlSchemeTask.didFailWithError(EquationRenderer.Failure.rejected); return
            }
            let mime: String
            switch url.pathExtension {
            case "html": mime = "text/html"
            case "js": mime = "application/javascript"
            case "css": mime = "text/css"
            case "woff2": mime = "font/woff2"
            default:
                #if DEBUG
                    diagnostic?.record(.assetRejected)
                #endif
                urlSchemeTask.didFailWithError(EquationRenderer.Failure.rejected); return
            }
            #if DEBUG
                diagnostic?.record(.assetStarted)
            #endif
            urlSchemeTask.didReceive(
                URLResponse(
                    url: url, mimeType: mime, expectedContentLength: data.count,
                    textEncodingName: mime == "font/woff2" ? nil : "utf-8"))
            urlSchemeTask.didReceive(data)
            urlSchemeTask.didFinish()
            #if DEBUG
                diagnostic?.record(.assetFinished)
            #endif
        }
        func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}
    }

    @MainActor
    private final class EquationNavigation: NSObject, WKNavigationDelegate, WKUIDelegate {
        var loaded: ((Result<Void, Error>) -> Void)?
        private var policy = EquationBootstrapPolicy()
        #if DEBUG
            weak var diagnostic: EquationPhaseDiagnostic?

            func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
                diagnostic?.record(.didStart)
            }
            func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
                diagnostic?.record(.didCommit)
            }
        #endif
        func webView(
            _ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
            decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void
        ) {
            #if DEBUG
                let wasStarted = policy.started
            #endif
            let allowed = policy.admit(
                action.request.url, mainFrame: action.targetFrame?.isMainFrame == true,
                other: action.navigationType == .other)
            #if DEBUG
                diagnostic?.action(
                    allowed: allowed, started: wasStarted,
                    bootstrap: action.request.url?.absoluteString == EquationResourcePolicy.bootstrap.absoluteString,
                    mainFrame: action.targetFrame?.isMainFrame == true, other: action.navigationType == .other)
            #endif
            decisionHandler(allowed ? .allow : .cancel)
        }
        func webView(
            _ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
            decisionHandler: @escaping @MainActor @Sendable (WKNavigationResponsePolicy) -> Void
        ) {
            let allowed = policy.allowsResponse(response.response.url, mainFrame: response.isForMainFrame)
            #if DEBUG
                diagnostic?.record(allowed ? .responseAllowed : .responseDenied)
            #endif
            decisionHandler(allowed ? .allow : .cancel)
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            #if DEBUG
                diagnostic?.record(.didFinish)
            #endif
            loaded?(.success(())); loaded = nil
        }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            #if DEBUG
                diagnostic?.record(.didFail)
            #endif
            loaded?(.failure(EquationRenderer.Failure.unavailable)); loaded = nil
        }
        func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error
        ) {
            #if DEBUG
                diagnostic?.record(.provisionalFailed)
            #endif
            loaded?(.failure(EquationRenderer.Failure.unavailable)); loaded = nil
        }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
            #if DEBUG
                diagnostic?.record(.processTerminated)
            #endif
            loaded?(.failure(EquationRenderer.Failure.unavailable)); loaded = nil
        }
        func webView(
            _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
        ) -> WKWebView? { nil }
        func webView(
            _ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
            initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
            decisionHandler: @escaping @MainActor @Sendable (WKPermissionDecision) -> Void
        ) { decisionHandler(.deny) }
    }

    struct EquationAttachment: Attachment {
        let source: String, png: Data
        let lease: EquationRasterLease
        let width: Double, height: Double, descent: Double
        static func == (lhs: Self, rhs: Self) -> Bool { lhs.lease === rhs.lease }
        func hash(into hasher: inout Hasher) { hasher.combine(ObjectIdentifier(lease)) }
        var description: String { source }
        var selectionStyle: AttachmentSelectionStyle { .text }
        var body: some View {
            if let image = UIImage(data: png) {
                Image(uiImage: image).renderingMode(.template).resizable().foregroundStyle(.primary)
                    .accessibilityLabel(source)
            }
        }
        func sizeThatFits(_ proposal: ProposedViewSize, in environment: TextEnvironmentValues) -> CGSize {
            let scale = scale(in: environment)
            return CGSize(width: width * scale, height: height * scale)
        }
        func baselineOffset(in environment: TextEnvironmentValues) -> CGFloat { -descent * scale(in: environment) }
        func pngData() -> Data? { png }
        private func scale(in environment: TextEnvironmentValues) -> CGFloat {
            // The raster has a 20-point CSS basis; scale with the same body text category.
            let points: CGFloat
            switch environment.dynamicTypeSize {
            case .xSmall: points = 14
            case .small: points = 15
            case .medium: points = 16
            case .large: points = 17
            case .xLarge: points = 19
            case .xxLarge: points = 21
            case .xxxLarge: points = 23
            case .accessibility1: points = 28
            case .accessibility2: points = 33
            case .accessibility3: points = 40
            case .accessibility4: points = 47
            case .accessibility5: points = 53
            @unknown default: points = 17
            }
            return points / 20
        }
    }
#endif
