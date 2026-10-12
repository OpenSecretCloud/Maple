#if os(iOS)
    import MapleAppCore
    import Observation
    import SwiftUI
    import UIKit

    struct CompactMenuChatState: Equatable, Sendable {
        var occluded = false
        var returnRevision = 0
        var focusNewProjection = false
    }

    enum CompactMenuChatReturn: Sendable { case navigation, cancelledPreview }

    /// This lease owns no controller. Hosting roots can retain it without a
    /// controller → root → controller ownership cycle.
    @MainActor @Observable final class CompactMenuLease {
        private(set) var scope: AccountScope?
        private(set) var storageOwner: UUID?
        private var controllerOwner: UUID?
        private(set) var menuVisible = false
        private var occluded = false
        private var returnRevision = 0
        private var changedProjection = false
        private var focusNewProjection = false

        func activate(scope: AccountScope, storageOwner: UUID?, controllerOwner: UUID) {
            self.scope = scope; self.storageOwner = storageOwner; self.controllerOwner = controllerOwner
            menuVisible = false; occluded = false; returnRevision = 0
            changedProjection = false; focusNewProjection = false
        }

        func isOwned(scope: AccountScope, controllerOwner: UUID) -> Bool {
            self.scope == scope && self.controllerOwner == controllerOwner
        }

        func updateProjection(scope: AccountScope, storageOwner: UUID?, controllerOwner: UUID) {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return }
            if self.storageOwner != storageOwner {
                changedProjection = occluded
                self.storageOwner = storageOwner
            }
        }

        func begin(scope: AccountScope, controllerOwner: UUID) {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return }
            occluded = true; menuVisible = false; focusNewProjection = false
        }

        func showMenu(scope: AccountScope, controllerOwner: UUID) {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return }
            occluded = true; menuVisible = true
        }

        func showChat(scope: AccountScope, controllerOwner: UUID) {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return }
            menuVisible = false; occluded = false
            focusNewProjection = changedProjection; changedProjection = false
            returnRevision += 1
        }

        func retire(scope: AccountScope, controllerOwner: UUID) {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return }
            self.scope = nil; storageOwner = nil; self.controllerOwner = nil
            menuVisible = false; occluded = false
            changedProjection = false; focusNewProjection = false
        }

        func retireAccount(scope: AccountScope) {
            guard self.scope == scope, let owner = controllerOwner else { return }
            retire(scope: scope, controllerOwner: owner)
        }

        // Each hosted projection keeps its own account and storage-owner fence.
        // Temporary navigation occlusion belongs to the plain UIKit route.
        func chatProjectionAccessibilityHidden(scope: AccountScope, storageOwner: UUID?) -> Bool {
            guard self.scope == scope, self.storageOwner == storageOwner, controllerOwner != nil else { return true }
            return false
        }

        func chatNavigationAccessibilityHidden(scope: AccountScope, controllerOwner: UUID) -> Bool {
            guard isOwned(scope: scope, controllerOwner: controllerOwner) else { return true }
            return occluded
        }

        func state(scope: AccountScope, storageOwner: UUID) -> CompactMenuChatState {
            guard self.scope == scope, self.storageOwner == storageOwner else { return CompactMenuChatState() }
            return CompactMenuChatState(
                occluded: occluded, returnRevision: returnRevision, focusNewProjection: focusNewProjection)
        }
    }

    private struct CompactMenuChatStateKey: EnvironmentKey {
        static let defaultValue: @MainActor @Sendable (AccountScope, UUID) -> CompactMenuChatState = { _, _ in
            CompactMenuChatState()
        }
    }

    extension EnvironmentValues {
        var compactMenuChatState: @MainActor @Sendable (AccountScope, UUID) -> CompactMenuChatState {
            get { self[CompactMenuChatStateKey.self] }
            set { self[CompactMenuChatStateKey.self] = newValue }
        }
    }

    /// Compact-only outer navigation. Each child owns its existing SwiftUI
    /// navigation path; the outer stack owns only Library ↔ Chat.
    struct CompactMenuNavigation<Library: View, Chat: View>: UIViewControllerRepresentable {
        let scope: AccountScope
        let storageOwner: UUID?
        let requestedMenu: Bool
        let reduceMotion: Bool
        let lease: CompactMenuLease
        let library: Library
        let chat: Chat
        let isCurrent: @MainActor () -> Bool
        let presentationBlocked: @MainActor () -> Bool
        let requestMenu: @MainActor (Bool) -> Void
        let blockPresentation: @MainActor (UUID, Bool) -> Void
        let chatShown: @MainActor (CompactMenuChatReturn) -> Void

        func makeUIViewController(context: Context) -> CompactMenuController<Library, Chat> {
            CompactMenuController(
                scope: scope, storageOwner: storageOwner, lease: lease,
                library: CompactMenuHostedContent(content: library, environment: context.environment),
                chat: CompactMenuHostedContent(content: chat, environment: context.environment),
                isCurrent: isCurrent, presentationBlocked: presentationBlocked,
                requestMenu: requestMenu, blockPresentation: blockPresentation, chatShown: chatShown)
        }

        func updateUIViewController(_ controller: CompactMenuController<Library, Chat>, context: Context) {
            guard controller.scope == scope, controller.acceptsUpdates else { return }
            lease.updateProjection(scope: scope, storageOwner: storageOwner, controllerOwner: controller.ownership)
            withTransaction(context.transaction) {
                controller.libraryHost.rootView = CompactMenuHostedContent(
                    content: library, environment: context.environment,
                    publishBar: controller.barPublisher(role: .library, storageOwner: storageOwner))
                controller.chatHost.rootView = CompactMenuHostedContent(
                    content: chat, environment: context.environment,
                    publishBar: controller.barPublisher(role: .chat, storageOwner: storageOwner))
            }
            controller.update(requestedMenu: requestedMenu, reduceMotion: reduceMotion)
        }

        static func dismantleUIViewController(_ controller: CompactMenuController<Library, Chat>, coordinator: ()) {
            controller.retire()
        }
    }

    struct CompactMenuHostedContent<Content: View>: View {
        let content: Content
        let environment: EnvironmentValues
        var publishBar: CompactMenuBarPublicationHandler = { _, _, _ in }
        var body: some View {
            var merged = environment
            merged.compactMenuBarPublication = publishBar
            return content.environment(\.self, merged)
        }
    }

    enum CompactMenuBarRole: Equatable, Sendable { case library, chat }

    struct CompactMenuBarPublication {
        let hidden: Bool
        let title: String
        let backIdentifier: String
        let leading: AnyView?
        let principal: AnyView?
        let trailing: AnyView?
        let environment: EnvironmentValues
    }

    typealias CompactMenuBarPublicationHandler =
        @MainActor @Sendable (CompactMenuBarRole, UUID, CompactMenuBarPublication?) -> Void

    private struct CompactMenuBarPublicationKey: EnvironmentKey {
        static let defaultValue: CompactMenuBarPublicationHandler = { _, _, _ in }
    }

    extension EnvironmentValues {
        var compactMenuBarPublication: CompactMenuBarPublicationHandler {
            get { self[CompactMenuBarPublicationKey.self] }
            set { self[CompactMenuBarPublicationKey.self] = newValue }
        }
    }

    extension View {
        func compactMenuRootBar(
            role: CompactMenuBarRole, hidden: Bool = false, title: String = "",
            backIdentifier: String = "chat.menu", leading: AnyView? = nil,
            principal: AnyView? = nil, trailing: AnyView? = nil
        ) -> some View {
            background(
                CompactMenuRootBarPublisher(
                    role: role, hidden: hidden, title: title, backIdentifier: backIdentifier,
                    leading: leading, principal: principal, trailing: trailing
                )
                .frame(width: 0, height: 0))
        }
    }

    /// Publish from the view that owns each binding, without borrowing or
    /// replacing UIKit's navigation controller delegate. Offscreen roots retain
    /// their publication; teardown can withdraw only its own identity.
    private struct CompactMenuRootBarPublisher: UIViewControllerRepresentable {
        let role: CompactMenuBarRole
        let hidden: Bool
        let title: String
        let backIdentifier: String
        let leading: AnyView?
        let principal: AnyView?
        let trailing: AnyView?

        func makeUIViewController(context: Context) -> Controller { Controller() }

        func updateUIViewController(_ controller: Controller, context: Context) {
            controller.publish = context.environment.compactMenuBarPublication
            controller.role = role
            controller.publish(
                role, controller.owner,
                CompactMenuBarPublication(
                    hidden: hidden, title: title, backIdentifier: backIdentifier,
                    leading: leading, principal: principal, trailing: trailing, environment: context.environment))
        }

        static func dismantleUIViewController(_ controller: Controller, coordinator: ()) {
            if let role = controller.role { controller.publish(role, controller.owner, nil) }
        }

        final class Controller: UIViewController {
            let owner = UUID()
            var role: CompactMenuBarRole?
            var publish: CompactMenuBarPublicationHandler = { _, _, _ in }

            override func loadView() {
                let observer = UIView()
                observer.backgroundColor = .clear
                observer.isUserInteractionEnabled = false
                view = observer
            }
        }
    }

    /// A retained public hosting configuration gives bar controls their source
    /// environment and stable SwiftUI state without orphaned child controllers.
    @MainActor private final class CompactMenuBarSlot {
        private var hosted: (UIView & UIContentView)?

        func update(_ content: AnyView?, environment: EnvironmentValues) -> UIView? {
            guard let content else { clear(); return nil }
            let configuration = UIHostingConfiguration {
                content.environment(\.self, environment)
            }.margins(.all, 0)
            if let hosted {
                hosted.configuration = configuration
            } else {
                hosted = configuration.makeContentView()
            }
            hosted?.backgroundColor = .clear
            hosted?.sizeToFit()
            return hosted
        }

        func clear() {
            // UIKit requires a content view's original configuration type.
            // Removing and releasing this host releases its source environment.
            hosted?.removeFromSuperview()
            hosted = nil
        }
    }

    @MainActor private final class CompactMenuBarState {
        var publisherOwner: UUID?
        var hidden = false
        var leadingItem: UIBarButtonItem?
        var trailingItem: UIBarButtonItem?
        let leading = CompactMenuBarSlot()
        let principal = CompactMenuBarSlot()
        let trailing = CompactMenuBarSlot()
    }

    /// A route keeps its hosting child attached while UIKit moves only the plain
    /// parent view during outer navigation transitions.
    @MainActor private final class CompactMenuRouteController: UIViewController {
        private let hostingController: UIViewController
        private var hidesAccessibility: Bool

        init(hostingController: UIViewController, hidesAccessibility: Bool = false) {
            self.hostingController = hostingController
            self.hidesAccessibility = hidesAccessibility
            super.init(nibName: nil, bundle: nil)
            edgesForExtendedLayout = .all
            extendedLayoutIncludesOpaqueBars = true
        }

        required init?(coder: NSCoder) { return nil }

        override func viewDidLoad() {
            super.viewDidLoad()
            view.backgroundColor = UIColor(MapleSurface.canvas)
            view.accessibilityElementsHidden = hidesAccessibility
            addChild(hostingController)
            let hostedView = hostingController.view!
            hostedView.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(hostedView)
            NSLayoutConstraint.activate([
                hostedView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
                hostedView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
                hostedView.topAnchor.constraint(equalTo: view.topAnchor),
                hostedView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            ])
            hostingController.didMove(toParent: self)
        }

        func setAccessibilityHidden(_ hidden: Bool) {
            hidesAccessibility = hidden
            if let view = viewIfLoaded, view.accessibilityElementsHidden != hidden {
                view.accessibilityElementsHidden = hidden
            }
        }

        override var childForStatusBarStyle: UIViewController? { hostingController }
        override var childForStatusBarHidden: UIViewController? { hostingController }
        override var childForHomeIndicatorAutoHidden: UIViewController? { hostingController }
        override var childForScreenEdgesDeferringSystemGestures: UIViewController? { hostingController }
    }

    /// Freeze only the hosted Chat child while UIKit moves its plain route.
    /// The replica stays inside that route, so the system owns every frame of
    /// its native interactive transition and release animation.
    @MainActor private final class CompactMenuVisualProxy {
        private(set) var view: UIView?

        init?(source: UIView, container: UIView, mayRestoreSource: @escaping (UIView) -> Bool) {
            guard let window = source.window, container.window === window,
                source.superview === container, !source.isHidden, source.transform.isIdentity,
                mayRestoreSource(source)
            else { return nil }
            let frame = container.convert(source.bounds, from: source)
            guard [frame.minX, frame.minY, frame.width, frame.height].allSatisfy({ $0.isFinite }),
                frame.width > 0, frame.height > 0,
                let replica = source.snapshotView(afterScreenUpdates: false)
            else { return nil }
            replica.frame = frame
            replica.isUserInteractionEnabled = false
            replica.isAccessibilityElement = false
            replica.accessibilityElementsHidden = true
            view = replica
            container.addSubview(replica)
            // Do not hide the touched hosting subtree during a native contact.
            // The full-frame replica paints over its opaque canvas while the
            // live child continues receiving UIKit's gesture/keyboard lifecycle.
        }

        func remove() {
            guard let view else { return }
            view.layer.removeAllAnimations()
            view.removeFromSuperview(); self.view = nil
        }
    }

    @MainActor
    final class CompactMenuController<Library: View, Chat: View>: UINavigationController,
        UINavigationControllerDelegate
    {
        let scope: AccountScope
        let ownership = UUID()
        private let presentationID = UUID()
        let libraryHost: UIHostingController<CompactMenuHostedContent<Library>>
        let chatHost: UIHostingController<CompactMenuHostedContent<Chat>>
        private let libraryRoute: CompactMenuRouteController
        private let chatRoute: CompactMenuRouteController
        private let lease: CompactMenuLease
        private let isCurrent: @MainActor () -> Bool
        private let presentationBlocked: @MainActor () -> Bool
        private let requestMenu: @MainActor (Bool) -> Void
        private let blockPresentation: @MainActor (UUID, Bool) -> Void
        private let chatShown: @MainActor (CompactMenuChatReturn) -> Void
        private let admission = UIPanGestureRecognizer()
        private lazy var admissionDelegate = AdmissionDelegate(owner: self)
        private weak var configuredEdge: UIGestureRecognizer?
        private weak var configuredContent: UIGestureRecognizer?
        private weak var cancelledEdge: UIGestureRecognizer?
        private weak var cancelledContent: UIGestureRecognizer?
        private var requestedMenu = false
        private var reduceMotion = false
        private var retired = false
        private var reconcilePending = false
        private var blockHeld = false
        private var blockedContactActive = false
        private weak var lastShown: UIViewController?
        private var needsFreshChatCompletion = false
        private var freshCompletionWithoutTransition = false
        private var freshChatReturnReason = CompactMenuChatReturn.navigation
        private var cancelledChatWidth: CGFloat?
        private var operation: Operation?
        private let libraryBar = CompactMenuBarState()
        private let chatBar = CompactMenuBarState()
        #if DEBUG
            private let exposesNativeDiagnostics = ProcessInfo.processInfo.arguments.contains(
                "--maple-shipping-viewport-fixture")
            private let nativeDiagnostics = UILabel()
            private var nativeAdmissionAttempts = 0
            private var nativeAdmissionAllowed = 0
            private var nativeEdgeEnabledAtAdmission = false
            private var nativeWillShowCount = 0
            private var nativeDidShowCount = 0
            private var nativeMenuWillShowCount = 0
            private var nativeMenuDidShowCount = 0
            private var nativeCancelledCount = 0
            private var nativeAppCancelledCount = 0
            private var nativeReleaseProgress: CGFloat = 0
            private var nativeReleaseTranslation: CGFloat = 0
            private var nativeReleaseVelocity: CGFloat = 0
            private var nativeReleaseGestureState = -1
            private var nativeReleaseCancelled = false
            private var nativeEndEditingResult = false
        #endif

        /// UIKit keeps its recognizers, targets, delegates and interaction
        /// driver. This simultaneous no-op pan is only a failure requirement:
        /// it rejects an unowned contact or a non-edge content pan without
        /// stealing the transcript's vertical or nested horizontal scrolling.
        @MainActor private final class AdmissionDelegate: NSObject, UIGestureRecognizerDelegate {
            private weak var owner: CompactMenuController?

            init(owner: CompactMenuController) { self.owner = owner }

            func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
                guard let owner, gestureRecognizer === owner.admission else { return true }
                let admitted = owner.admitsNativeContact()
                #if DEBUG
                    owner.nativeAdmissionAttempts += 1
                    if admitted { owner.nativeAdmissionAllowed += 1 }
                    owner.nativeEdgeEnabledAtAdmission = owner.configuredEdge?.isEnabled == true
                    owner.updateNativeDiagnostics()
                #endif
                return !admitted
            }

            func gestureRecognizer(
                _ gestureRecognizer: UIGestureRecognizer,
                shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
            ) -> Bool { true }
        }

        @MainActor private final class Operation {
            let width: CGFloat
            let interactive: Bool
            let destinationMenu: Bool
            var invalidated = false
            var interactionEnded = false
            var coordinatorInstalled = false
            var visualProxy: CompactMenuVisualProxy?

            init(width: CGFloat, interactive: Bool, destinationMenu: Bool) {
                self.width = width; self.interactive = interactive; self.destinationMenu = destinationMenu
            }
        }

        init(
            scope: AccountScope, storageOwner: UUID?, lease: CompactMenuLease,
            library: CompactMenuHostedContent<Library>, chat: CompactMenuHostedContent<Chat>,
            isCurrent: @escaping @MainActor () -> Bool, presentationBlocked: @escaping @MainActor () -> Bool,
            requestMenu: @escaping @MainActor (Bool) -> Void,
            blockPresentation: @escaping @MainActor (UUID, Bool) -> Void,
            chatShown: @escaping @MainActor (CompactMenuChatReturn) -> Void
        ) {
            self.scope = scope; self.lease = lease
            let libraryHost = UIHostingController(rootView: library)
            let chatHost = UIHostingController(rootView: chat)
            libraryHost.safeAreaRegions = .all
            chatHost.safeAreaRegions = .all
            self.libraryHost = libraryHost; self.chatHost = chatHost
            libraryRoute = CompactMenuRouteController(hostingController: libraryHost)
            chatRoute = CompactMenuRouteController(hostingController: chatHost, hidesAccessibility: true)
            self.isCurrent = isCurrent; self.presentationBlocked = presentationBlocked
            self.requestMenu = requestMenu; self.blockPresentation = blockPresentation; self.chatShown = chatShown
            super.init(nibName: nil, bundle: nil)
            lease.activate(scope: scope, storageOwner: storageOwner, controllerOwner: ownership)
            delegate = self
            libraryHost.rootView.publishBar = barPublisher(role: .library, storageOwner: storageOwner)
            chatHost.rootView.publishBar = barPublisher(role: .chat, storageOwner: storageOwner)
            setViewControllers([libraryRoute, chatRoute], animated: false)
        }

        required init?(coder: NSCoder) { return nil }

        override var isToolbarHidden: Bool {
            get { super.isToolbarHidden }
            set { super.isToolbarHidden = true }
        }

        override func setToolbarHidden(_ hidden: Bool, animated: Bool) {
            super.setToolbarHidden(true, animated: false)
        }

        override func viewDidLoad() {
            super.viewDidLoad()
            let appearance = UINavigationBarAppearance()
            appearance.configureWithTransparentBackground()
            if let menuImage = UIImage(systemName: "line.3.horizontal") {
                appearance.setBackIndicatorImage(menuImage, transitionMaskImage: menuImage)
            }
            navigationBar.standardAppearance = appearance
            navigationBar.scrollEdgeAppearance = appearance
            navigationBar.compactAppearance = appearance
            navigationBar.compactScrollEdgeAppearance = appearance
            navigationBar.tintColor = .label
            libraryRoute.navigationItem.backButtonDisplayMode = .minimal
            configureNativeBack(identifier: "chat.menu")
            setNavigationBarHidden(false, animated: false)
            setToolbarHidden(true, animated: false)
            // The menu's physical-left interaction is independent of the
            // explicitly localized layout direction of each hosting child.
            view.semanticContentAttribute = .forceLeftToRight
            admission.delegate = admissionDelegate
            admission.cancelsTouchesInView = false
            admission.addTarget(self, action: #selector(handleAdmission(_:)))
            view.addGestureRecognizer(admission)
            configureNativeFailureRequirements()
            #if DEBUG
                if exposesNativeDiagnostics {
                    nativeDiagnostics.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
                    nativeDiagnostics.textColor = .clear
                    nativeDiagnostics.isUserInteractionEnabled = false
                    nativeDiagnostics.isAccessibilityElement = true
                    nativeDiagnostics.accessibilityIdentifier = "compact-menu.native-diagnostics"
                    view.addSubview(nativeDiagnostics)
                    updateNativeDiagnostics()
                }
            #endif
        }

        override func viewDidLayoutSubviews() {
            super.viewDidLayoutSubviews()
            configureNativeFailureRequirements()
            scheduleReconcile()
            #if DEBUG
                updateNativeDiagnostics()
            #endif
        }

        func update(requestedMenu: Bool, reduceMotion: Bool) {
            guard !retired, current else { retire(); return }
            self.requestedMenu = requestedMenu; self.reduceMotion = reduceMotion
            synchronizeChatAccessibility()
            if let operation, operation.interactive, !operation.interactionEnded, !requestedMenu {
                operation.invalidated = true
                cancelInteraction(operation)
            }
            scheduleReconcile()
        }

        func retire() {
            guard !retired else { return }
            retired = true
            chatRoute.setAccessibilityHidden(true)
            clearRootBars()
            navigationBar.accessibilityIdentifier = nil
            navigationBar.accessibilityElementsHidden = true
            navigationBar.isUserInteractionEnabled = false
            cancelledChatWidth = nil
            if let operation {
                operation.invalidated = true
                operation.visualProxy?.remove()
                cancelInteraction(operation)
            }
            lease.retire(scope: scope, controllerOwner: ownership)
            holdBlock(false)
        }

        private func holdBlock(_ value: Bool) {
            guard blockHeld != value else { return }
            blockHeld = value; blockPresentation(presentationID, value)
        }

        func barPublisher(role expected: CompactMenuBarRole, storageOwner: UUID?) -> CompactMenuBarPublicationHandler {
            { [weak self] role, publisherOwner, publication in
                guard let self, role == expected, !self.retired, self.current,
                    role == .library || self.lease.storageOwner == storageOwner
                else { return }
                self.publishRootBar(role: role, owner: publisherOwner, publication: publication)
            }
        }

        private func publishRootBar(role: CompactMenuBarRole, owner: UUID, publication: CompactMenuBarPublication?) {
            let state = role == .library ? libraryBar : chatBar
            let route = role == .library ? libraryRoute : chatRoute
            guard let publication else {
                guard state.publisherOwner == owner else { return }
                state.publisherOwner = nil
                state.leading.clear(); state.principal.clear(); state.trailing.clear()
                state.leadingItem = nil; state.trailingItem = nil
                route.navigationItem.leftBarButtonItem = nil
                route.navigationItem.titleView = nil
                route.navigationItem.rightBarButtonItem = nil
                // Preserve Settings' hidden state until the current Library
                // root returns; withdrawal must never manufacture its return.
                return
            }
            state.publisherOwner = owner
            state.hidden = publication.hidden
            route.navigationItem.title = publication.title
            route.navigationItem.titleView = state.principal.update(
                publication.principal, environment: publication.environment)
            let trailing = state.trailing.update(publication.trailing, environment: publication.environment)
            if let trailing {
                if state.trailingItem == nil { state.trailingItem = UIBarButtonItem(customView: trailing) }
            } else {
                state.trailingItem = nil
            }
            route.navigationItem.rightBarButtonItem = state.trailingItem
            if role == .library {
                let leading = state.leading.update(publication.leading, environment: publication.environment)
                if let leading {
                    if state.leadingItem == nil { state.leadingItem = UIBarButtonItem(customView: leading) }
                } else {
                    state.leadingItem = nil
                }
                route.navigationItem.leftBarButtonItem = state.leadingItem
            } else {
                // UIKit's real Back item owns the Menu action and interaction.
                // A replacement left item would disable standard pop admission.
                route.navigationItem.leftBarButtonItem = nil
                configureNativeBack(identifier: publication.backIdentifier)
            }
            if topViewController === route { applyBarVisibility(for: route) }
            synchronizeBarAuthority()
        }

        private func configureNativeBack(identifier: String) {
            let label = MapleStrings.text("Menu")
            if let back = libraryRoute.navigationItem.backBarButtonItem,
                back.title == "", back.accessibilityIdentifier == identifier, back.accessibilityLabel == label
            {
                return
            }
            let back = UIBarButtonItem(title: "", style: .plain, target: nil, action: nil)
            back.accessibilityIdentifier = identifier
            back.accessibilityLabel = label
            libraryRoute.navigationItem.backBarButtonItem = back
        }

        private func applyBarVisibility(for route: UIViewController) {
            guard !retired, current else { return }
            navigationBar.accessibilityIdentifier = route === chatRoute ? "chat.navigation" : "library.navigation"
            let hidden = route === libraryRoute ? libraryBar.hidden : chatBar.hidden
            if isNavigationBarHidden != hidden { setNavigationBarHidden(hidden, animated: false) }
        }

        private func synchronizeBarAuthority() {
            let authorized =
                !retired && current && operation == nil && viewIfLoaded?.window != nil
                && ((!requestedMenu && stableChat) || (requestedMenu && stableMenu)) && !ownedPresentation
            navigationBar.isUserInteractionEnabled = authorized
            navigationBar.accessibilityElementsHidden = !authorized
        }

        private func clearRootBars() {
            for (route, state) in [(libraryRoute, libraryBar), (chatRoute, chatBar)] {
                route.navigationItem.leftBarButtonItem = nil
                route.navigationItem.titleView = nil
                route.navigationItem.rightBarButtonItem = nil
                route.navigationItem.backBarButtonItem = nil
                state.publisherOwner = nil
                state.leading.clear(); state.principal.clear(); state.trailing.clear()
                state.leadingItem = nil; state.trailingItem = nil
            }
        }

        private func configureNativeFailureRequirements() {
            if let edge = interactivePopGestureRecognizer, configuredEdge !== edge {
                edge.require(toFail: admission)
                configuredEdge = edge
            }
            if #available(iOS 26.0, *), let content = interactiveContentPopGestureRecognizer,
                configuredContent !== content
            {
                content.require(toFail: admission)
                configuredContent = content
            }
        }

        #if DEBUG
            private func updateNativeDiagnostics() {
                guard exposesNativeDiagnostics else { return }
                nativeDiagnostics.text =
                    "admission \(nativeAdmissionAttempts); allowed \(nativeAdmissionAllowed); edge-at-contact \(nativeEdgeEnabledAtAdmission ? 1 : 0); edge \(configuredEdge?.isEnabled == true ? 1 : 0):\(configuredEdge?.state.rawValue ?? -1); content \(configuredContent?.isEnabled == true ? 1 : 0):\(configuredContent?.state.rawValue ?? -1); will \(nativeWillShowCount); did \(nativeDidShowCount); menu \(nativeMenuWillShowCount):\(nativeMenuDidShowCount); cancelled \(nativeCancelledCount):\(nativeAppCancelledCount); top \(chatRoute.viewIfLoaded?.safeAreaInsets.top ?? -1):\(chatHost.viewIfLoaded?.safeAreaInsets.top ?? -1):\(viewIfLoaded?.window?.safeAreaInsets.top ?? -1); release \(nativeReleaseProgress):\(nativeReleaseTranslation):\(nativeReleaseVelocity):\(nativeReleaseGestureState):\(nativeReleaseCancelled ? 1 : 0); end-editing \(nativeEndEditingResult ? 1 : 0)"
            }
        #endif

        var acceptsUpdates: Bool { !retired && current }
        private var current: Bool { isCurrent() && lease.isOwned(scope: scope, controllerOwner: ownership) }
        private var width: CGFloat { viewIfLoaded?.bounds.width ?? 0 }
        private var ownedPresentation: Bool {
            presentedViewController != nil || libraryRoute.presentedViewController != nil
                || chatRoute.presentedViewController != nil || libraryHost.presentedViewController != nil
                || chatHost.presentedViewController != nil
        }
        private var stableChat: Bool {
            topViewController === chatRoute && viewControllers.count == 2
                && viewControllers.first === libraryRoute && viewControllers.last === chatRoute
        }
        private var stableMenu: Bool {
            topViewController === libraryRoute && viewControllers.count == 1 && viewControllers.first === libraryRoute
        }
        private var admitted: Bool {
            !retired && !blockedContactActive && current && operation == nil && !requestedMenu && stableChat
                && viewIfLoaded?.window != nil && width.isFinite && width > 0
                && !ownedPresentation && !presentationBlocked()
        }
        private var nativeContactActive: Bool {
            [configuredEdge, configuredContent].contains { gesture in
                gesture?.state == .began || gesture?.state == .changed
            }
        }

        private func admitsNativeContact() -> Bool {
            guard admitted else { return false }
            let location = admission.location(in: view), translation = admission.translation(in: view)
            let originX = location.x - translation.x
            // The iOS 26 content recognizer must not interpret a wide table or
            // code swipe as outer navigation. UIKit still determines direction,
            // progress, velocity, cancellation and completion for an edge pop.
            return originX.isFinite && originX >= 0 && originX <= 44
        }

        @objc private func handleAdmission(_ gesture: UIPanGestureRecognizer) {
            guard gesture === admission else { return }
            switch gesture.state {
            case .began: blockedContactActive = true
            case .ended, .cancelled, .failed:
                blockedContactActive = false; scheduleReconcile()
            default: break
            }
        }

        private func synchronizeChatAccessibility() {
            let window = viewIfLoaded?.window
            let hidden =
                retired || !current || !stableChat || requestedMenu
                || window == nil || chatRoute.viewIfLoaded?.window !== window
                || chatHost.viewIfLoaded?.window !== window
                || lease.chatNavigationAccessibilityHidden(scope: scope, controllerOwner: ownership)
            chatRoute.setAccessibilityHidden(hidden)
            synchronizeBarAuthority()
        }

        private func scheduleReconcile() {
            guard !retired, !reconcilePending else { return }
            reconcilePending = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.reconcilePending = false; self.reconcile()
            }
        }

        private func reconcile() {
            guard !retired, current else { retire(); return }
            synchronizeChatAccessibility()
            guard viewIfLoaded?.window != nil, width.isFinite, width > 0 else { return }
            if let operation {
                bindNativeCoordinator(operation)
                if abs(operation.width - width) > 0.5 {
                    operation.invalidated = true
                    requestedMenu = false; requestMenu(false)
                    cancelInteraction(operation)
                }
                return
            }
            guard !blockedContactActive, !nativeContactActive else { return }
            if let cancelledChatWidth {
                if abs(cancelledChatWidth - width) > 0.5 {
                    self.cancelledChatWidth = nil
                    needsFreshChatCompletion = true
                    freshChatReturnReason = .cancelledPreview
                } else {
                    guard !ownedPresentation, settleCancelledChat(width: cancelledChatWidth) else { return }
                    self.cancelledChatWidth = nil
                    if !requestedMenu {
                        finishChat(reason: .cancelledPreview)
                        announceCancelledChatLayout(width: cancelledChatWidth)
                    }
                }
            }
            // Only a cancelled ownership/size boundary temporarily disables a
            // native recognizer. Restore it on a fresh owned route; ordinary
            // admission uses failure requirements and leaves UIKit in control.
            if stableChat, !requestedMenu {
                cancelledEdge?.isEnabled = true; cancelledEdge = nil
                cancelledContent?.isEnabled = true; cancelledContent = nil
            }
            guard !ownedPresentation else { return }
            if requestedMenu, stableChat {
                startNavigation(menu: true)
            } else if !requestedMenu, stableMenu {
                startNavigation(menu: false)
            } else if needsFreshChatCompletion, stableChat,
                lastShown === chatRoute || freshCompletionWithoutTransition
            {
                needsFreshChatCompletion = false; freshCompletionWithoutTransition = false
                finishChat(reason: freshChatReturnReason)
            }
        }

        private func begin(_ value: Operation) {
            operation = value
            chatRoute.setAccessibilityHidden(true)
            lease.begin(scope: scope, controllerOwner: ownership); holdBlock(true)
            synchronizeBarAuthority()
        }

        private func startNavigation(menu: Bool) {
            guard operation == nil, current, width.isFinite, width > 0 else { return }
            let value = Operation(width: width, interactive: false, destinationMenu: menu)
            begin(value)
            if menu {
                if popViewController(animated: !reduceMotion) == nil {
                    operation = nil
                    requestedMenu = false; requestMenu(false)
                    finishChat(); scheduleReconcile()
                }
            } else {
                pushViewController(chatRoute, animated: !reduceMotion)
            }
        }

        /// This delegate observes native navigation; it does not supply an
        /// animation or interaction controller. UIKit owns the release motion.
        func navigationController(
            _ navigationController: UINavigationController, willShow viewController: UIViewController, animated: Bool
        ) {
            guard navigationController === self, !retired, current else { return }
            applyBarVisibility(for: viewController)
            #if DEBUG
                nativeWillShowCount += 1
                if viewController === libraryRoute { nativeMenuWillShowCount += 1 }
                updateNativeDiagnostics()
            #endif
            if operation == nil, viewController === libraryRoute,
                transitionCoordinator?.viewController(forKey: .from) === chatRoute || lastShown === chatRoute
            {
                let value = Operation(
                    width: width,
                    interactive: transitionCoordinator?.initiallyInteractive == true || nativeContactActive,
                    destinationMenu: true)
                begin(value)
                requestedMenu = true; requestMenu(true)
            }
            guard let operation else { return }
            if viewController === libraryRoute, operation.destinationMenu,
                !operation.invalidated, operation.visualProxy == nil,
                let source = chatHost.viewIfLoaded, let container = chatRoute.viewIfLoaded
            {
                operation.visualProxy = CompactMenuVisualProxy(
                    source: source, container: container,
                    mayRestoreSource: { [weak self, weak operation] source in
                        guard let self, let operation, !self.retired, self.current,
                            self.operation === operation, self.chatHost.viewIfLoaded === source,
                            source.superview === self.chatRoute.viewIfLoaded
                        else { return false }
                        return true
                    })
                #if DEBUG
                    nativeEndEditingResult = source.endEditing(true)
                    updateNativeDiagnostics()
                #else
                    _ = source.endEditing(true)
                #endif
            }
            bindNativeCoordinator(operation)
        }

        private func bindNativeCoordinator(_ captured: Operation) {
            guard operation === captured, !captured.coordinatorInstalled, let coordinator = transitionCoordinator else {
                return
            }
            captured.coordinatorInstalled = true
            coordinator.notifyWhenInteractionChanges { [weak self, weak captured] context in
                guard let self, let captured, self.operation === captured, !self.retired, self.current,
                    !context.isInteractive
                else { return }
                captured.interactionEnded = true
                #if DEBUG
                    if self.exposesNativeDiagnostics {
                        let progress = context.percentComplete
                        self.nativeReleaseProgress = progress.isFinite ? progress : 0
                        self.nativeReleaseCancelled = context.isCancelled
                        let pans = [self.configuredEdge, self.configuredContent].compactMap {
                            $0 as? UIPanGestureRecognizer
                        }
                        if let window = self.viewIfLoaded?.window,
                            let pan = pans.first(where: { $0.state != .possible && $0.state != .failed }) ?? pans.first
                        {
                            let translation = pan.translation(in: window).x, velocity = pan.velocity(in: window).x
                            self.nativeReleaseTranslation = translation.isFinite ? translation : 0
                            self.nativeReleaseVelocity = velocity.isFinite ? velocity : 0
                            self.nativeReleaseGestureState = pan.state.rawValue
                        }
                        self.updateNativeDiagnostics()
                    }
                #endif
                if context.isCancelled {
                    #if DEBUG
                        self.nativeCancelledCount += 1
                        self.updateNativeDiagnostics()
                    #endif
                    self.requestedMenu = false; self.requestMenu(false)
                }
            }
            coordinator.animate(alongsideTransition: nil) { [weak self, weak captured] _ in
                guard let self, let captured, self.operation === captured else { return }
                captured.visualProxy?.remove()
                self.scheduleReconcile()
            }
        }

        private func cancelInteraction(_ captured: Operation) {
            if captured.invalidated { captured.visualProxy?.remove() }
            guard captured.interactive, !captured.interactionEnded else { return }
            captured.interactionEnded = true
            #if DEBUG
                nativeAppCancelledCount += 1
                updateNativeDiagnostics()
            #endif
            // There is no public coordinator cancellation API. Cancelling only
            // the active system recognizer terminates an obsolete contact; its
            // delegate and targets remain untouched and UIKit owns the rollback.
            if let edge = configuredEdge, edge.state == .began || edge.state == .changed {
                cancelledEdge = edge; edge.isEnabled = false
            }
            if let content = configuredContent, content.state == .began || content.state == .changed {
                cancelledContent = content; content.isEnabled = false
            }
        }

        func navigationController(
            _ navigationController: UINavigationController, didShow viewController: UIViewController, animated: Bool
        ) {
            guard navigationController === self, !retired, current else { return }
            #if DEBUG
                nativeDidShowCount += 1
                if viewController === libraryRoute { nativeMenuDidShowCount += 1 }
                updateNativeDiagnostics()
            #endif
            lastShown = viewController
            let finished = operation
            finished?.visualProxy?.remove()
            operation = nil
            applyBarVisibility(for: viewController)
            synchronizeBarAuthority()
            let validWidth = finished.map { abs($0.width - width) <= 0.5 && !$0.invalidated } ?? true
            if viewController === libraryRoute, stableMenu {
                lease.showMenu(scope: scope, controllerOwner: ownership)
                if !validWidth { requestedMenu = false; requestMenu(false) }
            } else if viewController === chatRoute, stableChat {
                if finished?.interactive == true {
                    requestedMenu = false; requestMenu(false)
                }
                if !requestedMenu {
                    if validWidth, let finished, finished.interactive, finished.destinationMenu {
                        // UIKit has restored the retained route. Complete its
                        // live child's layout before returning accessibility.
                        cancelledChatWidth = finished.width
                    } else if validWidth {
                        finishChat(reason: finished?.interactive == true ? .cancelledPreview : .navigation)
                    } else {
                        needsFreshChatCompletion = true
                        freshChatReturnReason = finished?.interactive == true ? .cancelledPreview : .navigation
                    }
                }
            }
            scheduleReconcile()
        }

        private func settleCancelledChat(width capturedWidth: CGFloat) -> Bool {
            guard !retired, current, operation == nil, stableChat, lastShown === chatRoute,
                let window = viewIfLoaded?.window, abs(capturedWidth - width) <= 0.5,
                let chatView = chatRoute.viewIfLoaded, let hostedView = chatHost.viewIfLoaded,
                chatView.window === window, hostedView.window === window, hostedView.superview === chatView,
                abs(chatView.bounds.width - capturedWidth) <= 0.5
            else { return false }
            UIView.performWithoutAnimation {
                hostedView.setNeedsLayout(); hostedView.layoutIfNeeded()
            }
            return true
        }

        private func finishChat(reason: CompactMenuChatReturn = .navigation) {
            guard !retired, current, stableChat, !requestedMenu, viewIfLoaded?.window != nil,
                width.isFinite, width > 0
            else { return }
            needsFreshChatCompletion = false; freshCompletionWithoutTransition = false
            freshChatReturnReason = .navigation
            lease.showChat(scope: scope, controllerOwner: ownership)
            synchronizeChatAccessibility()
            holdBlock(false); chatShown(reason)
        }

        private func announceCancelledChatLayout(width capturedWidth: CGFloat) {
            guard let window = chatHost.viewIfLoaded?.window, let owner = lease.storageOwner else { return }
            let revision = lease.state(scope: scope, storageOwner: owner).returnRevision
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, !self.retired, self.current,
                    self.operation == nil, !self.blockedContactActive, !self.nativeContactActive, self.stableChat,
                    self.lastShown === self.chatRoute, !self.requestedMenu,
                    !self.lease.menuVisible, self.lease.storageOwner == owner,
                    self.viewIfLoaded?.window === window, self.chatHost.viewIfLoaded?.window === window,
                    abs(self.width - capturedWidth) <= 0.5,
                    !self.ownedPresentation, !self.presentationBlocked()
                else { return }
                let state = self.lease.state(scope: self.scope, storageOwner: owner)
                guard !state.occluded, state.returnRevision == revision else { return }
                UIAccessibility.post(notification: .layoutChanged, argument: nil)
            }
        }
    }
#endif
