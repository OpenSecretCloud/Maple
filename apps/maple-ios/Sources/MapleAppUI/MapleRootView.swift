#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import Security
    import SwiftUI
    import UIKit

    public struct MapleRootView: View {
        @State private var environment: AppEnvironment?
        @State private var accountLinkInbox = AccountLinkInbox()
        #if DEBUG
            @State private var deliveredLinkCount = 0
        #endif
        @State private var configurationFailed = false
        @State private var secureStorageFailed = false
        @State private var keychainDiagnostic: OSStatus?
        @Environment(\.scenePhase) private var scenePhase

        public init() {}

        public var body: some View {
            #if DEBUG
                if ProcessInfo.processInfo.arguments.contains("--maple-renderer-fixture") {
                    RendererFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-shipping-viewport-fixture") {
                    ShippingViewportFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-library-controls-fixture") {
                    LibraryControlsFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-auth-workflow-fixture") {
                    AuthWorkflowFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-dictation-fixture") {
                    DictationFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-entitlement-fixture") {
                    EntitlementFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-chat-workflow-fixture") {
                    ChatWorkflowFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-browser-fixture") {
                    BrowserFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-player-fixture") {
                    ReplyPlayerFixtureView()
                } else if ProcessInfo.processInfo.arguments.contains("--maple-composer-fixture") {
                    ComposerKeyboardFixtureView()
                } else {
                    application
                }
            #else
                application
            #endif
        }

        private var application: some View {
            Group {
                if let environment {
                    SessionRoot(environment: environment)
                } else if configurationFailed {
                    ContentUnavailableView {
                        Label(
                            secureStorageFailed
                                ? MapleStrings.text("Secure storage unavailable")
                                : MapleStrings.text("Configuration unavailable"),
                            systemImage: "gear.badge.xmark")
                    } description: {
                        Text(
                            secureStorageFailed
                                ? MapleStrings.text("Maple couldn’t open protected account storage.")
                                : MapleStrings.text("This build needs a valid service configuration."))
                        #if DEBUG
                            if let keychainDiagnostic {
                                Text("Keychain probe: \(keychainDiagnostic)").accessibilityIdentifier(
                                    "startup.keychainStatus")
                            }
                        #endif
                    }
                } else {
                    MapleLaunchSurface()
                }
            }
            .safeAreaInset(edge: .top) {
                #if DEBUG
                    if let environment, environment.configuration.profile == .local {
                        let traits = ProcessInfo.processInfo.arguments.contains("--maple-localization-proof")
                        let identity = ProcessInfo.processInfo.arguments.contains("--maple-library-local-cleanup")
                        if traits || (identity && environment.session.account != nil) {
                            // These read-only diagnostics must not obscure or intercept
                            // the shipping toolbar when the actual window becomes narrow.
                            HStack(spacing: 8) {
                                if traits { LocalLocalizationProof().frame(width: 120, height: 10) }
                                if identity, let scope = environment.session.account?.scope {
                                    LibraryCleanupIdentityBadge(scope: scope)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .frame(height: 12)
                            .clipped()
                            .allowsHitTesting(false)
                        }
                    }
                    if ProcessInfo.processInfo.arguments.contains("--maple-account-link-fixture"),
                        environment?.configuration.profile == .local
                    {
                        Text("Delivered links \(deliveredLinkCount)").font(.caption2)
                            .accessibilityIdentifier("account-link.delivered")
                    }
                    if let fixture = environment?.lifecycleFixture,
                        !ProcessInfo.processInfo.arguments.contains("--maple-local-listening-fixture")
                    {
                        LocalLifecycleFixtureControls(driver: fixture)
                            .accessibilityHidden(environment?.menuPresented == true)
                    }
                #endif
            }
            .font(MapleTypography.body)
            .tint(MapleAccent.foreground)
            .background(MapleSurface.canvas.ignoresSafeArea())
            .task {
                guard environment == nil, !configurationFailed else { return }
                do {
                    let created = try AppEnvironment(
                        configuration: AppConfiguration.load(), accountLinkInbox: accountLinkInbox)
                    environment = created
                    await created.session.restore()
                    created.resumeAccountLinks()
                } catch {
                    secureStorageFailed = (error as? TransportError) == .persistence
                    configurationFailed = true
                    #if DEBUG
                        if ProcessInfo.processInfo.arguments.contains("--maple-startup-diagnostic") {
                            keychainDiagnostic = SecItemCopyMatching(
                                [
                                    kSecClass as String: kSecClassGenericPassword,
                                    kSecAttrService as String: "cloud.opensecret.maple.native.dev.absent-diagnostic",
                                ] as CFDictionary, nil)
                        }
                    #endif
                }
            }
            .onChange(of: scenePhase) { _, phase in
                environment?.foreground = phase == .active
                if phase == .active { Task { await environment?.signIn.foreground() } }
            }
            .onOpenURL(perform: receiveURL)
            .onChange(of: environment?.session.phase) { _, _ in environment?.resumeAccountLinks() }
            .onReceive(
                NotificationCenter.default.publisher(for: UIApplication.protectedDataWillBecomeUnavailableNotification)
            ) { _ in
                Task { await environment?.protectedDataChanged(false) }
            }
            .onReceive(
                NotificationCenter.default.publisher(for: UIApplication.protectedDataDidBecomeAvailableNotification)
            ) { _ in
                Task { await environment?.protectedDataChanged(true) }
            }
        }

        private func receiveURL(_ received: URL) {
            guard let configuration = environment?.configuration ?? (try? AppConfiguration.load()) else { return }
            var url = received
            #if DEBUG
                if configuration.profile == .local,
                    ProcessInfo.processInfo.arguments.contains("--maple-account-link-fixture")
                {
                    deliveredLinkCount += 1
                    url = LocalAccountLinkFixture.forward(received, configuration: configuration) ?? received
                }
            #endif
            let acceptedURL = url
            if let environment {
                environment.receiveLink(acceptedURL)
            } else if let link = IncomingAccountLink(url: url, configuration: configuration) {
                _ = accountLinkInbox.offer(link, phase: .restoring, scope: nil)
            }
        }
    }

    /// Text-only red for errors and destructive labels on standard app surfaces.
    struct MapleRedText: ShapeStyle {
        nonisolated func resolve(in environment: EnvironmentValues) -> Color {
            environment.colorScheme == .dark ? .red : Color(red: 0.7, green: 0.1, blue: 0.1)
        }
    }

    /// Legible prompts on the standard light and dark authentication surfaces.
    struct MapleFieldPrompt: ShapeStyle {
        nonisolated func resolve(in environment: EnvironmentValues) -> Color {
            environment.colorScheme == .dark ? Color(white: 0.65) : Color(white: 0.4)
        }
    }

    private struct SessionRoot: View {
        @Bindable var environment: AppEnvironment
        var body: some View {
            Group {
                switch environment.session.phase {
                case .signedIn:
                    ChatScreen(environment: environment)
                        .task(id: environment.session.account?.scope) {
                            if let account = environment.session.account { await environment.activate(account) }
                        }
                case .signedOut, .authenticating:
                    SignInView(
                        session: environment.session, coordinator: environment.signIn,
                        onPasswordReset: { environment.presentAccount(.reset) })
                case .restoring, .cleaningUp:
                    MapleLaunchSurface()
                case .unavailable:
                    ContentUnavailableView {
                        Label(MapleStrings.text("Unable to open your account"), systemImage: "wifi.exclamationmark")
                    } description: {
                        if case .clock(let clock)? = environment.session.issue {
                            Text(AttestationClockText.message(clock))
                        } else {
                            Text(
                                environment.session.issue == .cleanup
                                    ? MapleStrings.text("Account cleanup must finish before you can sign in again.")
                                    : MapleStrings.text(
                                        "Your saved work is preserved. Try again when your connection and protected storage are available."
                                    )
                            )
                        }
                    } actions: {
                        Button(MapleStrings.text("Try again")) {
                            Task {
                                await environment.session.restore()
                            }
                        }.buttonStyle(MaplePrimaryButtonStyle())
                        if environment.session.issue != .cleanup {
                            Button(MapleStrings.text("Sign out"), role: .destructive) {
                                Task { await environment.session.signOut() }
                            }
                        }
                    }
                }
            }
            .sheet(item: $environment.accountFlow) { flow in
                AccountFlowView(flow: flow, session: environment.session)
            }
            .preferredColorScheme(environment.preferences?.value.theme.preferredColorScheme)
        }
    }

    private struct MapleLaunchSurface: View {
        var body: some View {
            MapleWordmark()
                .frame(width: 130, height: 28)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(MapleSurface.canvas.ignoresSafeArea())
                .accessibilityLabel(Text(verbatim: "Maple"))
        }
    }
#endif
