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

        func chatAccessibilityHidden(scope: AccountScope, storageOwner: UUID?) -> Bool {
            guard self.scope == scope, self.storageOwner == storageOwner, controllerOwner != nil else { return true }
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
        let chatShown: @MainActor () -> Void

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

        init(hostingController: UIViewController) {
            self.hostingController = hostingController
            super.init(nibName: nil, bundle: nil)
        }

        required init?(coder: NSCoder) { return nil }

        override func viewDidLoad() {
            super.viewDidLoad()
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

        override var childForStatusBarStyle: UIViewController? { hostingController }
        override var childForStatusBarHidden: UIViewController? { hostingController }
        override var childForHomeIndicatorAutoHidden: UIViewController? { hostingController }
        override var childForScreenEdgesDeferringSystemGestures: UIViewController? { hostingController }
    }

    @MainActor
    final class CompactMenuController<Library: View, Chat: View>: UINavigationController,
        UINavigationControllerDelegate, UIGestureRecognizerDelegate
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
        private let chatShown: @MainActor () -> Void
        private let edge = UIScreenEdgePanGestureRecognizer()
        private var requestedMenu = false
        private var reduceMotion = false
        private var retired = false
        private var reconcilePending = false
        private var blockHeld = false
        private weak var lastShown: UIViewController?
        private var needsFreshChatCompletion = false
        private var freshCompletionWithoutTransition = false
        private var contactRejected = false
        private var edgeContactActive = false
        private var operation: Operation?

        private final class Operation {
            let width: CGFloat
            let interactive: Bool
            let reducedMotion: Bool
            let destinationMenu: Bool
            var invalidated = false
            var navigationStarted = false
            init(width: CGFloat, interactive: Bool, reducedMotion: Bool, destinationMenu: Bool) {
                self.width = width; self.interactive = interactive
                self.reducedMotion = reducedMotion; self.destinationMenu = destinationMenu
            }
        }

        init(
            scope: AccountScope, storageOwner: UUID?, lease: CompactMenuLease,
            library: CompactMenuHostedContent<Library>, chat: CompactMenuHostedContent<Chat>,
            isCurrent: @escaping @MainActor () -> Bool, presentationBlocked: @escaping @MainActor () -> Bool,
            requestMenu: @escaping @MainActor (Bool) -> Void,
            blockPresentation: @escaping @MainActor (UUID, Bool) -> Void, chatShown: @escaping @MainActor () -> Void
        ) {
            self.scope = scope; self.lease = lease
            let libraryHost = UIHostingController(rootView: library)
            let chatHost = UIHostingController(rootView: chat)
            self.libraryHost = libraryHost; self.chatHost = chatHost
            libraryRoute = CompactMenuRouteController(hostingController: libraryHost)
            chatRoute = CompactMenuRouteController(hostingController: chatHost)
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
            edge.edges = .left; edge.delegate = self; edge.isEnabled = false
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
            if let operation, operation.interactive, !requestedMenu {
                operation.invalidated = true
                if !operation.navigationStarted { cancelStillPreview(operation) }
            }
            scheduleReconcile()
        }

        func retire() {
            guard !retired else { return }
            retired = true; contactRejected = edgeContactActive
            if let operation {
                operation.invalidated = true
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
            guard viewIfLoaded?.window != nil, width.isFinite, width > 0 else { return }
            if let operation {
                if abs(operation.width - width) > 0.5 {
                    operation.invalidated = true; contactRejected = edgeContactActive
                    requestedMenu = false; requestMenu(false)
                    if operation.interactive && !operation.navigationStarted { cancelStillPreview(operation) }
                }
                return
            }
            // A cancelled resized swipe remains this recognizer's contact until
            // its terminal event. Do not reset it or start a queued route midway.
            guard !edgeContactActive else { return }
            disableStockGestures()
            edge.isEnabled = admitted
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
                finishChat()
            }
        }

        private func startNavigation(menu: Bool) {
            guard operation == nil, current, width.isFinite, width > 0 else { return }
            let value = Operation(width: width, interactive: false, reducedMotion: reduceMotion, destinationMenu: menu)
            value.navigationStarted = true; operation = value
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
                lease.begin(scope: scope, controllerOwner: ownership); holdBlock(true)
                requestedMenu = true; requestMenu(true)
            // Keep the actual Chat route still throughout edge contact.
            // An accepted release starts ordinary native navigation.
            case .changed:
                guard let operation, operation.interactive, !operation.invalidated, !operation.navigationStarted else {
                    return
                }
                guard current, abs(width - operation.width) <= 0.5 else {
                    operation.invalidated = true; contactRejected = edgeContactActive
                    requestedMenu = false; requestMenu(false)
                    cancelStillPreview(operation)
                    return
                }
            case .ended, .cancelled, .failed:
                edgeContactActive = false
                defer { contactRejected = false; scheduleReconcile() }
                guard let operation, operation.interactive, !operation.navigationStarted else {
                    scheduleReconcile(); return
                }
                let translation = gesture.translation(in: view)
                let commit =
                    gesture.state == .ended && !operation.invalidated && current && stableChat && !ownedPresentation
                    && abs(width - operation.width) <= 0.5 && translation.x > 65
                    && translation.x > abs(translation.y) * 1.5
                if commit {
                    operation.navigationStarted = true
                    _ = popViewController(animated: !operation.reducedMotion)
                } else {
                    cancelStillPreview(operation)
                }
            default: break
            }
        }

        private func cancelStillPreview(_ captured: Operation) {
            guard operation === captured, !captured.navigationStarted else { return }
            operation = nil
            guard !retired, current, stableChat else { retire(); return }
            requestedMenu = false; requestMenu(false)
            // No native transition started on this path. Verify the same owned
            // Chat instead of manufacturing a UINavigationController didShow.
            if !captured.invalidated, abs(width - captured.width) <= 0.5 {
                finishChat()
            } else {
                needsFreshChatCompletion = true; freshCompletionWithoutTransition = true
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
            return CompactMenuAnimator(operation: operation)
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
                    if validWidth { finishChat() } else { needsFreshChatCompletion = true }
                }
            }
            if let finished, finished.interactive, finished.destinationMenu, !finished.reducedMotion,
                viewController === chatRoute, validWidth, !retired, current, stableChat, !requestedMenu,
                viewIfLoaded?.window != nil, width.isFinite, width > 0,
                abs(finished.width - width) <= 0.5, !finished.invalidated, !ownedPresentation
            {
                // The custom cancelled preview has returned to the same screen's layout.
                UIAccessibility.post(notification: .layoutChanged, argument: nil)
            }
            scheduleReconcile()
        }

        private func finishChat() {
            guard !retired, current, stableChat, !requestedMenu, viewIfLoaded?.window != nil,
                width.isFinite, width > 0
            else { return }
            needsFreshChatCompletion = false; freshCompletionWithoutTransition = false
            lease.showChat(scope: scope, controllerOwner: ownership); holdBlock(false); chatShown()
        }
    }

    @MainActor private final class CompactMenuAnimator: NSObject, UIViewControllerAnimatedTransitioning {
        let operation: UINavigationController.Operation
        private var animator: UIViewPropertyAnimator?
        init(operation: UINavigationController.Operation) { self.operation = operation }
        func transitionDuration(using transitionContext: (any UIViewControllerContextTransitioning)?) -> TimeInterval {
            0.2
        }
        func animateTransition(using context: any UIViewControllerContextTransitioning) {
            interruptibleAnimator(using: context).startAnimation()
        }

        func interruptibleAnimator(using context: any UIViewControllerContextTransitioning)
            -> any UIViewImplicitlyAnimating
        {
            if let animator { return animator }
            let animator = UIViewPropertyAnimator(duration: transitionDuration(using: context), curve: .linear)
            animator.flushUpdates = true
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
            from.bounds = fromBounds; from.center = fromCenter
            to.bounds = toBounds; to.center = toCenter
            if operation == .pop {
                container.insertSubview(to, belowSubview: from)
            } else {
                to.center = CGPoint(x: toCenter.x + width, y: toCenter.y)
                container.addSubview(to)
            }
            let operation = operation
            animator.addAnimations {
                if operation == .pop {
                    from.center = CGPoint(x: fromCenter.x + width, y: fromCenter.y)
                } else {
                    to.center = toCenter
                }
            }
            animator.addCompletion { _ in
                let cancelled = context.transitionWasCancelled
                // Settle model geometry before UIKit completes a cancelled or committed slide.
                UIView.performWithoutAnimation {
                    from.bounds = fromBounds; from.center = fromCenter
                    to.bounds = toBounds; to.center = toCenter
                    if cancelled { to.removeFromSuperview() } else { from.removeFromSuperview() }
                }
                context.completeTransition(!cancelled)
            }
            return animator
        }

        func animationEnded(_ transitionCompleted: Bool) {
            animator = nil
        }
    }
#endif
