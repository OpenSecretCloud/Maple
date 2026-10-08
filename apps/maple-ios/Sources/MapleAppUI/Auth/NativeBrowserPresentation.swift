#if os(iOS)
    import AuthenticationServices
    import UIKit

    enum BrowserSignInPresentationError: Sendable, Equatable { case cancelled, failed }
    typealias BrowserSignInCompletion = @MainActor @Sendable (URL?, BrowserSignInPresentationError?) -> Void
    typealias BrowserSignInPresentationFactory =
        @MainActor (
            URL, String, UIWindow, @escaping BrowserSignInCompletion
        ) -> any BrowserSignInPresentation

    @MainActor protocol BrowserSignInPresentation: AnyObject {
        func start() -> Bool
        func cancel()
    }

    /// The injectable seam owns presentation only. Credential redemption and
    /// account confirmation always remain in the production coordinator/Core.
    @MainActor
    final class NativeBrowserPresentation: NSObject, BrowserSignInPresentation,
        ASWebAuthenticationPresentationContextProviding
    {
        private let anchor: UIWindow
        private let session: ASWebAuthenticationSession

        init(url: URL, callbackScheme: String, anchor: UIWindow, completion: @escaping BrowserSignInCompletion) {
            self.anchor = anchor
            session = ASWebAuthenticationSession(url: url, callback: .customScheme(callbackScheme)) { url, error in
                let failure: BrowserSignInPresentationError? = error.map {
                    ($0 as? ASWebAuthenticationSessionError)?.code == .canceledLogin ? .cancelled : .failed
                }
                Task { @MainActor in completion(url, failure) }
            }
            super.init()
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = true
        }

        func start() -> Bool { session.start() }
        func cancel() { session.cancel() }
        func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor { anchor }
    }
#endif
