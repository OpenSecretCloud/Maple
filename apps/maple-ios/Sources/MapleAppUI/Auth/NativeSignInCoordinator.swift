#if os(iOS)
    import AuthenticationServices
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import UIKit

    /// Owns exactly one native presentation and its SessionController ticket.
    /// Neither browser completion nor a universal-link delivery installs before
    /// the Core's explicit verified-account confirmation step.
    @MainActor @Observable public final class NativeSignInCoordinator: NSObject {
        public let ceremonies: AuthCeremonies
        public private(set) var failure: AuthCeremonyIssue?
        public private(set) var isRunning = false
        @ObservationIgnored private let session: SessionController
        @ObservationIgnored private weak var anchor: UIWindow?
        @ObservationIgnored private var ticket: UUID?
        @ObservationIgnored private var attemptID: UUID?
        @ObservationIgnored private var appleController: ASAuthorizationController?
        @ObservationIgnored private var applePresentationContext: AppleSignInPresentationContext?
        @ObservationIgnored private var webSession: (any BrowserSignInPresentation)?
        @ObservationIgnored private let browserFactory: BrowserSignInPresentationFactory
        @ObservationIgnored private var browserCallbackClaimed = false

        public convenience init(session: SessionController, ceremonies: AuthCeremonies) {
            self.init(
                session: session, ceremonies: ceremonies,
                browserFactory: { url, scheme, anchor, completion in
                    NativeBrowserPresentation(url: url, callbackScheme: scheme, anchor: anchor, completion: completion)
                })
        }

        init(
            session: SessionController, ceremonies: AuthCeremonies,
            browserFactory: @escaping BrowserSignInPresentationFactory
        ) {
            self.session = session; self.ceremonies = ceremonies
            self.browserFactory = browserFactory
            super.init()
        }

        public func attach(window: UIWindow?) { anchor = window }

        public func beginApple() async {
            guard !isRunning, anchor != nil, let owned = session.beginExternalAuthentication() else { return }
            ticket = owned; isRunning = true; failure = nil
            guard let request = await ceremonies.beginAppleSignIn(), ticket == owned else {
                await settleFailure(owned); return
            }
            guard let anchor, anchor.windowScene != nil else { await cancel(); return }
            attemptID = request.id
            let appleRequest = ASAuthorizationAppleIDProvider().createRequest()
            appleRequest.requestedScopes = [.fullName, .email]
            appleRequest.nonce = request.nonce; appleRequest.state = request.state
            let controller = ASAuthorizationController(authorizationRequests: [appleRequest])
            let presentationContext = AppleSignInPresentationContext(anchor: anchor)
            appleController = controller
            applePresentationContext = presentationContext
            controller.delegate = self; controller.presentationContextProvider = presentationContext
            controller.performRequests()
        }

        public func beginBrowser(provider: OAuthProvider) async {
            guard !isRunning, anchor != nil, let owned = session.beginExternalAuthentication() else { return }
            ticket = owned; isRunning = true; failure = nil
            guard let request = await ceremonies.beginBrowserSignIn(provider: provider), ticket == owned else {
                await settleFailure(owned); return
            }
            attemptID = request.id
            browserCallbackClaimed = false
            guard let anchor else { await cancel(); return }
            let browser = browserFactory(request.url, request.callbackScheme, anchor) {
                [weak self] url, error in
                Task { @MainActor in
                    guard let self, self.ticket == owned, self.attemptID == request.id,
                        !self.browserCallbackClaimed
                    else { return }
                    self.webSession = nil
                    guard let url else {
                        if error != .cancelled {
                            self.failure = .connection
                        }
                        await self.cancel(); return
                    }
                    await self.receiveBrowser(url, id: request.id, ticket: owned)
                }
            }
            webSession = browser
            if !browser.start() { failure = .connection; await cancel() }
        }

        /// Route a manual "Open Maple" callback only to a currently owned attempt.
        @discardableResult public func receiveCallback(_ url: URL) async -> Bool {
            guard let id = attemptID, let owned = ticket, case .browser = ceremonies.state else { return false }
            await receiveBrowser(url, id: id, ticket: owned)
            return true
        }

        private func receiveBrowser(_ url: URL, id: UUID, ticket owned: UUID) async {
            guard ticket == owned, attemptID == id, !browserCallbackClaimed else { return }
            browserCallbackClaimed = true
            let presentation = webSession; webSession = nil
            presentation?.cancel()
            await ceremonies.redeemBrowserCallback(id: id, url: url)
            guard ticket == owned else { return }
            if case .confirmBrowser = ceremonies.state { return }
            await settleFailure(owned)
        }

        public func confirmBrowser() async {
            guard let owned = ticket, case .confirmBrowser(let account) = ceremonies.state else { return }
            await ceremonies.confirmBrowserAccount(id: account.id)
            await finish(owned)
        }

        public func cancel() async {
            guard let owned = ticket else { await cancelPresentation(); return }
            await cancelPresentation()
            await session.cancelExternalAuthentication(owned)
        }

        /// Wire to SessionController.onCancelExternal. This does not call back into
        /// SessionController, so sign-out and cancellation cannot recurse.
        public func cancelPresentation() async {
            ticket = nil; attemptID = nil; isRunning = false
            browserCallbackClaimed = false
            let apple = appleController, browser = webSession
            appleController = nil; webSession = nil
            apple?.cancel(); browser?.cancel()
            applePresentationContext = nil
            await ceremonies.cancel()
        }

        public func foreground() async {
            ceremonies.expireIfNeeded()
            if ceremonies.issue == .expired, let owned = ticket { await settleFailure(owned) }
        }

        private func finish(_ owned: UUID) async {
            guard ticket == owned else { return }
            if ceremonies.state == .completed(.signedIn) {
                ticket = nil; attemptID = nil; isRunning = false
                appleController = nil; webSession = nil
                applePresentationContext = nil
                await session.finishExternalAuthentication(owned)
            } else {
                await settleFailure(owned)
            }
        }

        private func settleFailure(_ owned: UUID) async {
            guard ticket == owned else { return }
            failure = ceremonies.issue ?? failure
            await cancel()
        }
    }

    @MainActor
    private final class AppleSignInPresentationContext: NSObject,
        ASAuthorizationControllerPresentationContextProviding
    {
        private let anchor: UIWindow

        init(anchor: UIWindow) { self.anchor = anchor }

        func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor { anchor }
    }

    extension NativeSignInCoordinator: ASAuthorizationControllerDelegate {
        public func authorizationController(
            controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization
        ) {
            guard controller === appleController, let owned = ticket, let id = attemptID else { return }
            appleController = nil
            applePresentationContext = nil
            guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
                let data = credential.identityToken, let token = String(data: data, encoding: .utf8)
            else {
                failure = .invalidCallback
                Task { await settleFailure(owned) }; return
            }
            let result = AppleSignInCredential(
                identityToken: token, userIdentifier: credential.user, state: credential.state,
                email: credential.email, givenName: credential.fullName?.givenName,
                familyName: credential.fullName?.familyName)
            Task {
                guard ticket == owned else { return }
                await ceremonies.completeAppleSignIn(id: id, credential: result)
                await finish(owned)
            }
        }
        public func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: Error) {
            guard controller === appleController, let owned = ticket else { return }
            if (error as? ASAuthorizationError)?.code != .canceled { failure = .connection }
            appleController = nil
            applePresentationContext = nil
            Task { await settleFailure(owned) }
        }
    }

    public struct NativeSignInButtons: View {
        @Bindable private var coordinator: NativeSignInCoordinator
        @Environment(\.scenePhase) private var scenePhase
        @Environment(\.colorScheme) private var colorScheme
        public init(coordinator: NativeSignInCoordinator) { self.coordinator = coordinator }
        public var body: some View {
            VStack(spacing: 12) {
                NativeAppleSignInButton { Task { await coordinator.beginApple() } }
                    .id(colorScheme)
                    .frame(height: 48).disabled(coordinator.isRunning).accessibilityIdentifier("auth.apple")
                Button {
                    Task { await coordinator.beginBrowser(provider: .google) }
                } label: {
                    Text("Continue with Google", tableName: "Auth", bundle: .module).frame(maxWidth: .infinity)
                }.buttonStyle(.bordered).disabled(coordinator.isRunning).accessibilityIdentifier("auth.google")
                Button {
                    Task { await coordinator.beginBrowser(provider: .github) }
                } label: {
                    Text("Continue with GitHub", tableName: "Auth", bundle: .module).frame(maxWidth: .infinity)
                }.buttonStyle(.bordered).disabled(coordinator.isRunning).accessibilityIdentifier("auth.github")
                if case .confirmBrowser(let account) = coordinator.ceremonies.state {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Confirm account", tableName: "Auth", bundle: .module).font(.headline)
                        Text(account.email ?? account.principalID).textSelection(.enabled)
                        Text(
                            "Only continue if this is the account you intended to use.", tableName: "Auth",
                            bundle: .module)
                        Button {
                            Task { await coordinator.confirmBrowser() }
                        } label: {
                            Text("Continue with this account", tableName: "Auth", bundle: .module)
                        }.buttonStyle(MaplePrimaryButtonStyle())
                            .accessibilityIdentifier("auth.confirm-account")
                    }.padding().background(.quaternary, in: RoundedRectangle(cornerRadius: 12))
                }
                if coordinator.isRunning {
                    Button(role: .cancel) {
                        Task { await coordinator.cancel() }
                    } label: {
                        Text("Cancel sign-in", tableName: "Auth", bundle: .module)
                    }.accessibilityIdentifier("auth.cancel-provider")
                }
                if let failure = coordinator.failure { AuthIssueView(issue: failure) }
            }
            .background(AuthPresentationAnchor { coordinator.attach(window: $0) }.frame(width: 0, height: 0))
            .onChange(of: scenePhase) { _, value in if value == .active { Task { await coordinator.foreground() } } }
            .onChange(of: coordinator.ceremonies.issue) { _, issue in
                if issue == .expired { Task { await coordinator.foreground() } }
            }
        }
    }

    private struct NativeAppleSignInButton: UIViewRepresentable {
        var action: @MainActor () -> Void
        @Environment(\.colorScheme) private var colorScheme
        @Environment(\.isEnabled) private var isEnabled
        func makeCoordinator() -> Target { Target(action: action) }
        func makeUIView(context: Context) -> ASAuthorizationAppleIDButton {
            let button = ASAuthorizationAppleIDButton(type: .signIn, style: colorScheme == .dark ? .white : .black)
            button.addTarget(context.coordinator, action: #selector(Target.activate), for: .touchUpInside)
            button.cornerRadius = 8
            return button
        }
        func updateUIView(_ uiView: ASAuthorizationAppleIDButton, context: Context) {
            context.coordinator.action = action
            uiView.isEnabled = isEnabled
        }
        @MainActor final class Target: NSObject {
            var action: @MainActor () -> Void
            init(action: @escaping @MainActor () -> Void) { self.action = action }
            @objc func activate() { action() }
        }
    }
    private struct AuthPresentationAnchor: UIViewRepresentable {
        let onWindow: @MainActor (UIWindow?) -> Void
        func makeUIView(context: Context) -> AnchorView {
            let view = AnchorView(); view.onWindow = onWindow; return view
        }
        func updateUIView(_ uiView: AnchorView, context: Context) { uiView.onWindow = onWindow }
        @MainActor final class AnchorView: UIView {
            var onWindow: (@MainActor (UIWindow?) -> Void)?
            override func didMoveToWindow() { super.didMoveToWindow(); onWindow?(window) }
        }
    }
#endif
