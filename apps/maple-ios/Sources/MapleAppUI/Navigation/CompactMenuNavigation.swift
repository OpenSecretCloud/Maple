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
                    content: library, environment: context.environment)
                controller.chatHost.rootView = CompactMenuHostedContent(content: chat, environment: context.environment)
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
        var body: some View { content.environment(\.self, environment) }
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
        }

        required init?(coder: NSCoder) { return nil }

        override func viewDidLoad() {
            super.viewDidLoad()
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

    /// UIKit's transition view can wrap the route controller's view. Remember
    /// the exact animated view without retaining its hierarchy or controller.
    @MainActor private final class CompactMenuSourceGeometry {
        weak var view: UIView?
        weak var container: UIView?
        let bounds: CGRect
        let center: CGPoint
        let transform: CGAffineTransform

        init(view: UIView, container: UIView) {
            self.view = view; self.container = container
            bounds = view.bounds; center = view.center; transform = view.transform
        }
    }

    /// A temporary in-memory visual replica keeps the live Chat's geometry
    /// fixed during a pop. It never participates in interaction or accessibility.
    @MainActor private final class CompactMenuVisualProxy {
        private weak var source: UIView?
        private weak var parent: UIView?
        private weak var window: UIWindow?
        private let sourceWasHidden: Bool
        private let mayRestoreSource: (UIView) -> Bool
        private(set) var view: UIView?

        init?(source: UIView, container: UIView, mayRestoreSource: @escaping (UIView) -> Bool) {
            guard let window = source.window, container.window === window,
                let parent = source.superview, !source.isHidden, source.transform.isIdentity,
                mayRestoreSource(source)
            else { return nil }
            let frame = container.convert(source.bounds, from: source)
            guard [frame.minX, frame.minY, frame.width, frame.height].allSatisfy({ $0.isFinite }),
                frame.width > 0, frame.height > 0,
                let replica = source.snapshotView(afterScreenUpdates: false)
            else { return nil }
            self.source = source; self.parent = parent; self.window = window
            sourceWasHidden = source.isHidden; self.mayRestoreSource = mayRestoreSource
            replica.frame = frame
            replica.isUserInteractionEnabled = false
            replica.isAccessibilityElement = false
            replica.accessibilityElementsHidden = true
            view = replica
            container.addSubview(replica)
            source.isHidden = true
        }

        func remove(restoreSource: Bool) {
            guard let view else { return }
            view.layer.removeAllAnimations()
            view.removeFromSuperview(); self.view = nil
            guard restoreSource, let source, let parent, let window,
                source.superview === parent, source.window === window, mayRestoreSource(source)
            else { return }
            source.isHidden = sourceWasHidden
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
        private let edge = UIScreenEdgePanGestureRecognizer()
        private lazy var edgeDelegate = EdgeDelegate(owner: self)
        private var requestedMenu = false
        private var reduceMotion = false
        private var retired = false
        private var reconcilePending = false
        private var blockHeld = false
        private weak var lastShown: UIViewController?
        private var needsFreshChatCompletion = false
        private var freshCompletionWithoutTransition = false
        private var freshChatReturnReason = CompactMenuChatReturn.navigation
        private var cancelledChat: (width: CGFloat, geometry: CompactMenuSourceGeometry)?
        private var contactRejected = false
        private var edgeContactActive = false
        private var operation: Operation?

        /// Own only this controller's menu edge. UIKit's navigation controller
        /// delegate filters belong to its stock recognizers, not this gesture.
        @MainActor private final class EdgeDelegate: NSObject, UIGestureRecognizerDelegate {
            private weak var owner: CompactMenuController?

            init(owner: CompactMenuController) { self.owner = owner }

            func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
                guard let owner, owner.acceptsEdgeDelegate(gestureRecognizer) else { return false }
                return owner.gestureRecognizerShouldBegin(gestureRecognizer)
            }

            func gestureRecognizer(
                _ gestureRecognizer: UIGestureRecognizer,
                shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
            ) -> Bool {
                guard let owner, owner.acceptsEdgeDelegate(gestureRecognizer) else { return false }
                return owner.gestureRecognizer(gestureRecognizer, shouldRecognizeSimultaneouslyWith: other)
            }

            func gestureRecognizer(
                _ gestureRecognizer: UIGestureRecognizer,
                shouldBeRequiredToFailBy other: UIGestureRecognizer
            ) -> Bool {
                guard let owner, owner.acceptsEdgeDelegate(gestureRecognizer) else { return false }
                return owner.gestureRecognizer(gestureRecognizer, shouldBeRequiredToFailBy: other)
            }
        }

        private func acceptsEdgeDelegate(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            gestureRecognizer === edge && !retired && current
        }
        @MainActor private final class Operation {
            let width: CGFloat
            let interactive: Bool
            let reducedMotion: Bool
            let destinationMenu: Bool
            var invalidated = false
            var navigationStarted = false
            var interactionEnded = false
            var sourceGeometry: CompactMenuSourceGeometry?
            var visualProxy: CompactMenuVisualProxy?
            let interaction: UIPercentDrivenInteractiveTransition?
            init(width: CGFloat, interactive: Bool, reducedMotion: Bool, destinationMenu: Bool) {
                self.width = width; self.interactive = interactive
                self.reducedMotion = reducedMotion; self.destinationMenu = destinationMenu
                if interactive {
                    let interaction = UIPercentDrivenInteractiveTransition()
                    interaction.completionCurve = .easeOut
                    self.interaction = interaction
                } else {
                    interaction = nil
                }
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
            self.libraryHost = libraryHost; self.chatHost = chatHost
            libraryRoute = CompactMenuRouteController(hostingController: libraryHost)
            chatRoute = CompactMenuRouteController(hostingController: chatHost, hidesAccessibility: true)
            self.isCurrent = isCurrent; self.presentationBlocked = presentationBlocked
            self.requestMenu = requestMenu; self.blockPresentation = blockPresentation; self.chatShown = chatShown
            super.init(nibName: nil, bundle: nil)
            lease.activate(scope: scope, storageOwner: storageOwner, controllerOwner: ownership)
            delegate = self
            setViewControllers([libraryRoute, chatRoute], animated: false)
        }

        required init?(coder: NSCoder) { return nil }

        // This outer stack owns no bar; each hosted NavigationStack owns its bar.
        override var isNavigationBarHidden: Bool {
            get { super.isNavigationBarHidden }
            set { super.isNavigationBarHidden = true }
        }

        override func setNavigationBarHidden(_ hidden: Bool, animated: Bool) {
            super.setNavigationBarHidden(true, animated: false)
        }

        override var isToolbarHidden: Bool {
            get { super.isToolbarHidden }
            set { super.isToolbarHidden = true }
        }

        override func setToolbarHidden(_ hidden: Bool, animated: Bool) {
            super.setToolbarHidden(true, animated: false)
        }

        override func viewDidLoad() {
            super.viewDidLoad()
            setNavigationBarHidden(true, animated: false)
            setToolbarHidden(true, animated: false)
            edge.edges = .left; edge.delegate = edgeDelegate; edge.isEnabled = false
            edge.addTarget(self, action: #selector(handleEdge(_:)))
            view.addGestureRecognizer(edge)
            disableStockGestures()
        }

        override func viewDidLayoutSubviews() {
            super.viewDidLayoutSubviews()
            scheduleReconcile()
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
            retired = true; contactRejected = edgeContactActive
            chatRoute.setAccessibilityHidden(true)
            cancelledChat = nil
            if let operation {
                operation.invalidated = true
                cancelInteraction(operation)
                if !operation.navigationStarted { self.operation = nil }
            }
            edge.isEnabled = false
            lease.retire(scope: scope, controllerOwner: ownership)
            holdBlock(false)
        }

        private func holdBlock(_ value: Bool) {
            guard blockHeld != value else { return }
            blockHeld = value; blockPresentation(presentationID, value)
        }

        private func disableStockGestures() {
            // These are this outer controller's recognizers only. The inner
            // Library NavigationStack retains its normal Back/editor behavior.
            interactivePopGestureRecognizer?.isEnabled = false
            if #available(iOS 26.0, *) { interactiveContentPopGestureRecognizer?.isEnabled = false }
        }

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
            !retired && !edgeContactActive && !contactRejected && current && operation == nil && !requestedMenu
                && stableChat
                && viewIfLoaded?.window != nil && width.isFinite && width > 0
                && !ownedPresentation && !presentationBlocked()
        }

        private func synchronizeChatAccessibility() {
            let window = viewIfLoaded?.window
            let hidden =
                retired || !current || !stableChat || requestedMenu
                || window == nil || chatRoute.viewIfLoaded?.window !== window
                || chatHost.viewIfLoaded?.window !== window
                || lease.chatNavigationAccessibilityHidden(scope: scope, controllerOwner: ownership)
            chatRoute.setAccessibilityHidden(hidden)
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
                if abs(operation.width - width) > 0.5 {
                    operation.invalidated = true; contactRejected = edgeContactActive
                    requestedMenu = false; requestMenu(false)
                    cancelInteraction(operation)
                }
                return
            }
            // A cancelled resized swipe remains this recognizer's contact until
            // its terminal event. Do not reset it or start a queued route midway.
            guard !edgeContactActive else { return }
            if let cancelledChat {
                if abs(cancelledChat.width - width) > 0.5 {
                    self.cancelledChat = nil
                    needsFreshChatCompletion = true
                    freshChatReturnReason = .cancelledPreview
                } else {
                    guard !ownedPresentation,
                        settleCancelledChat(width: cancelledChat.width, geometry: cancelledChat.geometry)
                    else { return }
                    self.cancelledChat = nil
                    if !requestedMenu {
                        finishChat(reason: .cancelledPreview)
                        announceCancelledChatLayout(width: cancelledChat.width)
                    }
                }
            }
            disableStockGestures()
            // Keep the recognizer ready on the owned Chat route. A sheet or
            // keyboard can finish without another outer navigation layout;
            // disabling here for a transient presentation would strand the
            // next edge contact. Admission still checks current account,
            // presentation and transition ownership at gesture start.
            edge.isEnabled = stableChat && !requestedMenu
            guard !ownedPresentation else { return }
            if requestedMenu, stableChat {
                startNavigation(menu: true)
            } else if !requestedMenu, stableMenu {
                startNavigation(menu: false)
            } else if needsFreshChatCompletion, stableChat,
                lastShown === chatRoute || freshCompletionWithoutTransition
            {
                // A changed width retired the previous callback. This is a new
                // stable owned-route check after that actual transition closed.
                needsFreshChatCompletion = false; freshCompletionWithoutTransition = false
                finishChat(reason: freshChatReturnReason)
            }
        }

        private func startNavigation(menu: Bool) {
            guard operation == nil, current, width.isFinite, width > 0 else { return }
            let value = Operation(width: width, interactive: false, reducedMotion: reduceMotion, destinationMenu: menu)
            value.navigationStarted = true; operation = value
            chatRoute.setAccessibilityHidden(true)
            lease.begin(scope: scope, controllerOwner: ownership); holdBlock(true); edge.isEnabled = false
            if menu {
                _ = popViewController(animated: !reduceMotion)
            } else {
                pushViewController(chatRoute, animated: !reduceMotion)
            }
        }

        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard gestureRecognizer === edge, admitted, !contactRejected else { return false }
            // The edge recognizer owns the origin; velocity checks initial direction.
            let velocity = edge.velocity(in: view)
            return velocity.x.isFinite && velocity.y.isFinite && velocity.x > abs(velocity.y) * 1.5
        }

        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool { false }

        func gestureRecognizer(
            _ gestureRecognizer: UIGestureRecognizer, shouldBeRequiredToFailBy other: UIGestureRecognizer
        ) -> Bool {
            // Priority is limited to the directly owned Chat subtree. A vertical
            // pan can proceed as soon as this physical edge rejects its intent.
            guard gestureRecognizer === edge, stableChat, let otherView = other.view,
                let chatView = chatHost.viewIfLoaded
            else { return false }
            return otherView.isDescendant(of: chatView)
        }

        @objc private func handleEdge(_ gesture: UIScreenEdgePanGestureRecognizer) {
            switch gesture.state {
            case .began:
                let canBegin = admitted
                edgeContactActive = true
                guard canBegin else { contactRejected = true; return }
                let value = Operation(
                    width: width, interactive: true, reducedMotion: reduceMotion, destinationMenu: true)
                operation = value
                chatRoute.setAccessibilityHidden(true)
                lease.begin(scope: scope, controllerOwner: ownership); holdBlock(true)
                requestedMenu = true; requestMenu(true)
                // The interaction delegate must already have its driver when
                // UIKit starts this pop. Keep both hosting controllers retained.
                value.navigationStarted = true
                if popViewController(animated: true) == nil {
                    value.navigationStarted = false
                    cancelStillPreview(value)
                    return
                }
                value.interaction?.update(
                    CompactMenuGesture.progress(translationX: gesture.translation(in: view).x, width: value.width))
            case .changed:
                guard let operation, operation.interactive, !operation.invalidated, !operation.interactionEnded else {
                    return
                }
                guard current, abs(width - operation.width) <= 0.5 else {
                    operation.invalidated = true; contactRejected = edgeContactActive
                    requestedMenu = false; requestMenu(false)
                    cancelInteraction(operation)
                    return
                }
                operation.interaction?.update(
                    CompactMenuGesture.progress(
                        translationX: gesture.translation(in: view).x, width: operation.width))
            case .ended, .cancelled, .failed:
                edgeContactActive = false
                defer { contactRejected = false; scheduleReconcile() }
                guard let operation, operation.interactive, !operation.interactionEnded else {
                    scheduleReconcile(); return
                }
                let translation = gesture.translation(in: view)
                let velocity = gesture.velocity(in: view)
                let commit =
                    gesture.state == .ended && !operation.invalidated && current && !ownedPresentation
                    && abs(width - operation.width) <= 0.5
                    && CompactMenuGesture.shouldFinish(
                        translationX: translation.x, velocityX: velocity.x, width: operation.width)
                if commit {
                    operation.interactionEnded = true
                    operation.interaction?.finish()
                } else {
                    requestedMenu = false; requestMenu(false)
                    cancelInteraction(operation)
                }
            default: break
            }
        }

        private func cancelInteraction(_ captured: Operation) {
            // Retirement never re-exposes an old source. A same-account width
            // change discards stale-sized pixels and reveals only its live route.
            if captured.invalidated { captured.visualProxy?.remove(restoreSource: current) }
            guard captured.interactive, !captured.interactionEnded else { return }
            captured.interactionEnded = true
            captured.interaction?.cancel()
        }

        private func cancelStillPreview(_ captured: Operation) {
            guard operation === captured, !captured.navigationStarted else { return }
            operation = nil
            guard !retired, current, stableChat else { retire(); return }
            requestedMenu = false; requestMenu(false)
            // No native transition started on this path. Verify the same owned
            // Chat instead of manufacturing a UINavigationController didShow.
            if !captured.invalidated, abs(width - captured.width) <= 0.5 {
                finishChat(reason: .cancelledPreview)
            } else {
                needsFreshChatCompletion = true; freshCompletionWithoutTransition = true
                freshChatReturnReason = .cancelledPreview
            }
            scheduleReconcile()
        }

        func navigationController(
            _ navigationController: UINavigationController,
            animationControllerFor operation: UINavigationController.Operation,
            from fromVC: UIViewController, to toVC: UIViewController
        ) -> (any UIViewControllerAnimatedTransitioning)? {
            guard navigationController === self,
                (fromVC === chatRoute && toVC === libraryRoute && operation == .pop)
                    || (fromVC === libraryRoute && toVC === chatRoute && operation == .push)
            else { return nil }
            let capturedOperation = self.operation
            return CompactMenuAnimator(
                operation: operation, reduceMotion: capturedOperation?.reducedMotion ?? reduceMotion,
                recordSourceGeometry: { [weak capturedOperation] geometry in
                    capturedOperation?.sourceGeometry = geometry
                },
                recordVisualProxy: { [weak self, weak capturedOperation] proxy in
                    guard let self, let capturedOperation, self.current,
                        self.operation === capturedOperation, !capturedOperation.invalidated
                    else { proxy.remove(restoreSource: false); return }
                    capturedOperation.visualProxy = proxy
                },
                mayRestoreVisualSource: { [weak self, weak capturedOperation] source in
                    guard let self, let capturedOperation, !self.retired, self.current,
                        self.operation === capturedOperation,
                        let chat = self.chatRoute.viewIfLoaded,
                        let window = self.viewIfLoaded?.window, source.window === window
                    else { return false }
                    return chat === source || chat.isDescendant(of: source)
                })
        }

        func navigationController(
            _ navigationController: UINavigationController,
            interactionControllerFor animationController: any UIViewControllerAnimatedTransitioning
        ) -> (any UIViewControllerInteractiveTransitioning)? {
            guard navigationController === self,
                let animator = animationController as? CompactMenuAnimator, animator.operation == .pop,
                let operation, operation.interactive, operation.navigationStarted, !operation.interactionEnded
            else { return nil }
            return operation.interaction
        }

        func navigationController(
            _ navigationController: UINavigationController, didShow viewController: UIViewController, animated: Bool
        ) {
            guard navigationController === self, !retired, current else { return }
            lastShown = viewController
            let finished = operation; operation = nil
            let validWidth = finished.map { abs($0.width - width) <= 0.5 && !$0.invalidated } ?? true
            disableStockGestures()
            if viewController === libraryRoute, stableMenu {
                lease.showMenu(scope: scope, controllerOwner: ownership)
                if !validWidth { requestedMenu = false; requestMenu(false) }
            } else if viewController === chatRoute, stableChat {
                if finished?.interactive == true {
                    requestedMenu = false; requestMenu(false)
                }
                if !requestedMenu {
                    if validWidth, let finished, finished.interactive, finished.destinationMenu,
                        let geometry = finished.sourceGeometry
                    {
                        // Acknowledge the exact retained transition view after
                        // UIKit's cancellation callback before exposing Chat again.
                        cancelledChat = (finished.width, geometry)
                        edge.isEnabled = false
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

        private func settleCancelledChat(width capturedWidth: CGFloat, geometry: CompactMenuSourceGeometry) -> Bool {
            guard !retired, current, operation == nil, stableChat, lastShown === chatRoute,
                let window = viewIfLoaded?.window, abs(capturedWidth - width) <= 0.5,
                let chatView = chatRoute.viewIfLoaded, let transitionView = geometry.view,
                let hostedView = chatHost.viewIfLoaded,
                let container = geometry.container,
                transitionView.window === window, container.window === window,
                hostedView.window === window, hostedView.isDescendant(of: chatView),
                transitionView.superview === container,
                chatView === transitionView || chatView.isDescendant(of: transitionView),
                container === view || container.isDescendant(of: view)
            else { return false }
            let bounds = geometry.bounds, center = geometry.center, transform = geometry.transform
            guard bounds.origin.x.isFinite, bounds.origin.y.isFinite,
                bounds.width.isFinite, bounds.height.isFinite, bounds.height > 0,
                bounds.width > 0, abs(bounds.width - capturedWidth) <= 0.5,
                center.x.isFinite, center.y.isFinite,
                transform.a.isFinite, transform.b.isFinite, transform.c.isFinite, transform.d.isFinite,
                transform.tx.isFinite, transform.ty.isFinite,
                abs(container.bounds.width - capturedWidth) <= 0.5
            else { return false }
            UIView.performWithoutAnimation {
                if transitionView.transform != transform { transitionView.transform = transform }
                // Complete the retained hosting subtree's pending layout at its
                // settled position before its accessibility elements return.
                hostedView.setNeedsLayout()
                hostedView.layoutIfNeeded()
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
            // The same accessibility subtree reappears after this custom
            // preview. Allow SwiftUI to expose it before reporting its layout;
            // nil requests no new VoiceOver focus target.
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, !self.retired, self.current,
                    self.operation == nil, !self.edgeContactActive, self.stableChat,
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

    @MainActor private final class CompactMenuAnimator: NSObject, UIViewControllerAnimatedTransitioning {
        let operation: UINavigationController.Operation
        let reduceMotion: Bool
        let recordSourceGeometry: (CompactMenuSourceGeometry) -> Void
        let recordVisualProxy: (CompactMenuVisualProxy) -> Void
        let mayRestoreVisualSource: (UIView) -> Bool
        private var animator: UIViewPropertyAnimator?
        private var visualProxy: CompactMenuVisualProxy?
        init(
            operation: UINavigationController.Operation, reduceMotion: Bool,
            recordSourceGeometry: @escaping (CompactMenuSourceGeometry) -> Void,
            recordVisualProxy: @escaping (CompactMenuVisualProxy) -> Void,
            mayRestoreVisualSource: @escaping (UIView) -> Bool
        ) {
            self.operation = operation; self.reduceMotion = reduceMotion
            self.recordSourceGeometry = recordSourceGeometry
            self.recordVisualProxy = recordVisualProxy; self.mayRestoreVisualSource = mayRestoreVisualSource
        }
        func transitionDuration(using transitionContext: (any UIViewControllerContextTransitioning)?) -> TimeInterval {
            reduceMotion ? 0.15 : 0.30
        }
        func animateTransition(using context: any UIViewControllerContextTransitioning) {
            interruptibleAnimator(using: context).startAnimation()
        }

        func interruptibleAnimator(using context: any UIViewControllerContextTransitioning)
            -> any UIViewImplicitlyAnimating
        {
            if let animator { return animator }
            let timing: any UITimingCurveProvider =
                context.isInteractive || reduceMotion
                ? UICubicTimingParameters(animationCurve: context.isInteractive ? .linear : .easeOut)
                : UISpringTimingParameters(dampingRatio: 0.88)
            let animator = UIViewPropertyAnimator(
                duration: transitionDuration(using: context), timingParameters: timing)
            // Direct manipulation stays linear with the finger; the release
            // eases to rest. Button navigation uses the native spring feel.
            animator.scrubsLinearly = true
            self.animator = animator
            guard let from = context.view(forKey: .from), let to = context.view(forKey: .to),
                let fromController = context.viewController(forKey: .from),
                let toController = context.viewController(forKey: .to)
            else {
                animator.addAnimations {}
                animator.addCompletion { _ in context.completeTransition(false) }
                return animator
            }
            let container = context.containerView
            let width = container.bounds.width
            guard width.isFinite, width > 0 else {
                animator.addAnimations {}
                animator.addCompletion { _ in context.completeTransition(false) }
                return animator
            }
            let fromFrame = context.initialFrame(for: fromController)
            let toFrame = context.finalFrame(for: toController)
            let fromBounds = CGRect(origin: from.bounds.origin, size: fromFrame.size)
            let toBounds = CGRect(origin: to.bounds.origin, size: toFrame.size)
            let fromCenter = CGPoint(x: fromFrame.midX, y: fromFrame.midY)
            let toCenter = CGPoint(x: toFrame.midX, y: toFrame.midY)
            let fromTransform = from.transform, toTransform = to.transform
            let parallax = reduceMotion ? 0 : width * 0.3
            to.bounds = toBounds; to.center = toCenter
            let proxy: CompactMenuVisualProxy?
            if operation == .pop {
                // Preserve the currently painted chat before its keyboard
                // changes the layout. Resignation and horizontal navigation
                // then share this transition rather than showing two steps.
                proxy = CompactMenuVisualProxy(
                    source: from, container: container, mayRestoreSource: mayRestoreVisualSource)
                _ = from.endEditing(true)
            } else {
                proxy = nil
            }
            visualProxy = proxy
            if let proxy { recordVisualProxy(proxy) }
            // Snapshot availability is optional. Preserve the existing live-view
            // transition as a fallback; its AX cancellation outcome is unproved.
            if proxy == nil {
                from.bounds = fromBounds; from.center = fromCenter
            }
            if operation == .pop {
                recordSourceGeometry(CompactMenuSourceGeometry(view: from, container: container))
                to.transform = toTransform.concatenating(CGAffineTransform(translationX: -parallax, y: 0))
                container.insertSubview(to, belowSubview: from)
            } else {
                to.transform = toTransform.concatenating(CGAffineTransform(translationX: width, y: 0))
                container.addSubview(to)
            }
            let operation = operation
            animator.addAnimations {
                if operation == .pop {
                    if let proxy {
                        proxy.view?.transform = CGAffineTransform(translationX: width, y: 0)
                    } else {
                        from.transform = fromTransform.concatenating(CGAffineTransform(translationX: width, y: 0))
                    }
                    to.transform = toTransform
                } else {
                    from.transform = fromTransform.concatenating(CGAffineTransform(translationX: -parallax, y: 0))
                    to.transform = toTransform
                }
            }
            animator.addCompletion { _ in
                let cancelled = context.transitionWasCancelled
                UIView.performWithoutAnimation {
                    // Drop all replica pixels before completing navigation. The
                    // same live source is restored only under its originating owner.
                    proxy?.remove(restoreSource: true)
                    if proxy == nil { from.transform = fromTransform }
                    to.transform = toTransform
                    if cancelled { to.removeFromSuperview() } else { from.removeFromSuperview() }
                }
                context.completeTransition(!cancelled)
            }
            return animator
        }

        func animationEnded(_ transitionCompleted: Bool) {
            visualProxy?.remove(restoreSource: true); visualProxy = nil
            animator = nil
        }
    }
#endif
