#if os(iOS)
    import MapleAppCore
    import Observation
    import SwiftUI
    import UIKit
    import UniformTypeIdentifiers

    /// The drag contains no exported chat metadata. Its local object is usable
    /// only by this menu instance and the exact account-session generation.
    @MainActor final class LibraryChatDragLease {
        let token = UUID()
        let scope: AccountScope
        let conversationID: String
        weak var library: LibraryModel?
        init(id: String, library: LibraryModel) {
            conversationID = id; scope = library.scope; self.library = library
        }
    }

    @MainActor @Observable final class LibraryChatDragContext {
        let scope: AccountScope
        private(set) var active: LibraryChatDragLease?
        var target: LibraryConversationDestination?
        private var working = false
        @ObservationIgnored private var prepared: LibraryChatDragLease?
        init(scope: AccountScope) { self.scope = scope }
        var isDragging: Bool { active != nil }
        func begin(id: String, library: LibraryModel) -> LibraryChatDragLease? {
            guard !working, active == nil, library.scope == scope, !library.isMutating,
                library.visibleConversations.contains(where: { $0.id == id })
            else { return nil }
            let lease = LibraryChatDragLease(id: id, library: library)
            // Changing List sections while UIKit is lifting a row cancels the
            // lift. Publish the empty Pinned target only after the drag begins.
            prepared = lease; return lease
        }
        func activate(_ lease: LibraryChatDragLease) {
            guard prepared === lease, !working, active == nil, lease.scope == scope,
                let library = lease.library, library.scope == scope, !library.isMutating,
                library.visibleConversations.contains(where: { $0.id == lease.conversationID })
            else { libraryDragTrace(.activationRejected); return }
            prepared = nil; active = lease; target = nil
            libraryDragTrace(.activated)
        }
        func cancel(_ lease: LibraryChatDragLease? = nil) {
            if lease == nil || prepared === lease { prepared = nil }
            if lease == nil || active === lease { active = nil; target = nil }
        }
        func admits(
            _ lease: LibraryChatDragLease, into library: LibraryModel, destination: LibraryConversationDestination
        ) -> Bool {
            guard !working, active === lease, scope == library.scope, scope == lease.scope,
                !library.isMutating, let source = lease.library, !source.isMutating,
                source.visibleConversations.contains(where: { $0.id == lease.conversationID })
            else { return false }
            if case .project(let id) = destination { return library.projects.contains { $0.id == id } }
            return true
        }
        func commit(
            _ lease: LibraryChatDragLease, into library: LibraryModel, destination: LibraryConversationDestination,
            onMutation: (@MainActor () async -> Void)?
        ) {
            guard admits(lease, into: library, destination: destination) else { return }
            libraryDragTrace(.commit)
            cancel(lease); working = true
            Task { [weak self] in
                await library.relocateConversation(id: lease.conversationID, to: destination, from: lease.scope)
                libraryDragTrace(library.issue == nil ? .acknowledged : .failed)
                if library.issue == nil { await onMutation?() }
                self?.working = false
            }
        }
    }

    struct LibraryChatDragSource: ViewModifier {
        let context: LibraryChatDragContext?
        let library: LibraryModel
        let id: String
        let enabled: Bool
        func body(content: Content) -> some View {
            content.background {
                if let context {
                    LibraryChatDragAnchor(context: context, library: library, id: id, enabled: enabled)
                        .frame(width: 0, height: 0)
                }
            }
        }
    }

    struct LibraryChatDropTarget: ViewModifier {
        let context: LibraryChatDragContext?
        let library: LibraryModel
        let destination: LibraryConversationDestination?
        let enabled: Bool
        var onMutation: (@MainActor () async -> Void)? = nil
        func body(content: Content) -> some View {
            content
                .background {
                    if let context, let destination, context.target == destination {
                        MapleAccent.prominent.opacity(0.12)
                    }
                }
                .background {
                    if let context, let destination {
                        LibraryChatDropAnchor(
                            context: context, library: library, destination: destination,
                            enabled: enabled, onMutation: onMutation
                        ).frame(width: 0, height: 0)
                    }
                }
        }
    }

    /// SwiftUI owns the List. Attach narrowly to its public UIKit cell or
    /// supplementary header, without replacing selection or scroll gestures.
    @MainActor private final class LibraryInteractionAnchorView: UIView {
        var reconcile: (() -> Void)?
        override func didMoveToSuperview() { super.didMoveToSuperview(); reconcile?() }
        override func didMoveToWindow() { super.didMoveToWindow(); reconcile?() }
        override func layoutSubviews() { super.layoutSubviews(); reconcile?() }
        var interactionHost: UIView? {
            var candidate = superview
            while let view = candidate {
                if let cell = view as? UICollectionViewCell { return cell.contentView }
                if let cell = view as? UITableViewCell { return cell.contentView }
                if view is UICollectionReusableView || view is UITableViewHeaderFooterView { return view }
                candidate = view.superview
            }
            return nil
        }
    }

    @MainActor private struct LibraryChatDragAnchor: UIViewRepresentable {
        let context: LibraryChatDragContext
        let library: LibraryModel
        let id: String
        let enabled: Bool
        func makeCoordinator() -> Coordinator { Coordinator(self) }
        func makeUIView(context: Context) -> LibraryInteractionAnchorView {
            let view = LibraryInteractionAnchorView()
            view.isUserInteractionEnabled = false
            view.reconcile = { [weak coordinator = context.coordinator, weak view] in
                if let view { coordinator?.reconcile(view) }
            }
            return view
        }
        func updateUIView(_ view: LibraryInteractionAnchorView, context: Context) {
            context.coordinator.parent = self; context.coordinator.reconcile(view)
        }
        static func dismantleUIView(_ view: LibraryInteractionAnchorView, coordinator: Coordinator) {
            view.reconcile = nil; coordinator.detach()
        }
        @MainActor final class Coordinator: NSObject, UIDragInteractionDelegate {
            var parent: LibraryChatDragAnchor
            private var interaction: UIDragInteraction?
            private weak var host: UIView?
            init(_ parent: LibraryChatDragAnchor) { self.parent = parent }
            func reconcile(_ anchor: LibraryInteractionAnchorView) {
                guard anchor.window != nil, let target = anchor.interactionHost else { detach(); return }
                if host !== target {
                    detach(); let interaction = UIDragInteraction(delegate: self)
                    interaction.isEnabled = parent.enabled; target.addInteraction(interaction)
                    self.interaction = interaction; host = target
                    libraryDragTrace(.source)
                } else {
                    interaction?.isEnabled = parent.enabled
                }
            }
            func detach() {
                if let interaction { host?.removeInteraction(interaction) }
                interaction = nil; host = nil
            }
            func dragInteraction(_ interaction: UIDragInteraction, itemsForBeginning session: any UIDragSession)
                -> [UIDragItem]
            {
                libraryDragTrace(.lift)
                guard parent.enabled,
                    let lease = parent.context.begin(id: parent.id, library: parent.library)
                else { libraryDragTrace(.liftRejected); return [] }
                libraryDragTrace(.liftAccepted)
                let token = Data(lease.token.uuidString.utf8)
                let provider = NSItemProvider()
                provider.registerDataRepresentation(
                    forTypeIdentifier: UTType.data.identifier, visibility: .ownProcess
                ) { completion in
                    completion(token, nil); return nil
                }
                let item = UIDragItem(itemProvider: provider); item.localObject = lease
                return [item]
            }
            func dragInteraction(_ interaction: UIDragInteraction, sessionWillBegin session: any UIDragSession) {
                libraryDragTrace(.began)
                if let lease = session.items.first?.localObject as? LibraryChatDragLease {
                    parent.context.activate(lease)
                }
            }
            func dragInteraction(
                _ interaction: UIDragInteraction, sessionAllowsMoveOperation session: any UIDragSession
            )
                -> Bool
            { true }
            func dragInteraction(
                _ interaction: UIDragInteraction, sessionIsRestrictedToDraggingApplication session: any UIDragSession
            ) -> Bool { true }
            func dragInteraction(
                _ interaction: UIDragInteraction, session: any UIDragSession, didEndWith operation: UIDropOperation
            ) {
                libraryDragTrace(operation == .move ? .moved : .cancelled)
                for item in session.items {
                    if let lease = item.localObject as? LibraryChatDragLease { parent.context.cancel(lease) }
                }
            }
        }
    }

    @MainActor private struct LibraryChatDropAnchor: UIViewRepresentable {
        let context: LibraryChatDragContext
        let library: LibraryModel
        let destination: LibraryConversationDestination
        let enabled: Bool
        let onMutation: (@MainActor () async -> Void)?
        func makeCoordinator() -> Coordinator { Coordinator(self) }
        func makeUIView(context: Context) -> LibraryInteractionAnchorView {
            let view = LibraryInteractionAnchorView()
            view.isUserInteractionEnabled = false
            view.reconcile = { [weak coordinator = context.coordinator, weak view] in
                if let view { coordinator?.reconcile(view) }
            }
            return view
        }
        func updateUIView(_ view: LibraryInteractionAnchorView, context: Context) {
            context.coordinator.parent = self; context.coordinator.reconcile(view)
        }
        static func dismantleUIView(_ view: LibraryInteractionAnchorView, coordinator: Coordinator) {
            view.reconcile = nil; coordinator.detach()
        }
        @MainActor final class Coordinator: NSObject, UIDropInteractionDelegate {
            var parent: LibraryChatDropAnchor
            private var interaction: UIDropInteraction?
            private weak var host: UIView?
            init(_ parent: LibraryChatDropAnchor) { self.parent = parent }
            func reconcile(_ anchor: LibraryInteractionAnchorView) {
                guard anchor.window != nil, let target = anchor.interactionHost else { detach(); return }
                if host !== target {
                    detach(); let interaction = UIDropInteraction(delegate: self)
                    target.addInteraction(interaction); self.interaction = interaction; host = target
                    libraryDragTrace(.target)
                }
            }
            func detach() {
                if let interaction { host?.removeInteraction(interaction) }
                interaction = nil; host = nil
            }
            private func lease(_ session: any UIDropSession) -> LibraryChatDragLease? {
                guard parent.enabled, session.localDragSession != nil, session.items.count == 1,
                    let lease = session.items.first?.localObject as? LibraryChatDragLease,
                    parent.context.admits(lease, into: parent.library, destination: parent.destination)
                else { return nil }
                return lease
            }
            func dropInteraction(_ interaction: UIDropInteraction, canHandle session: any UIDropSession) -> Bool {
                let admitted = lease(session) != nil
                libraryDragTrace(admitted ? .admitted : .rejected)
                return admitted
            }
            func dropInteraction(_ interaction: UIDropInteraction, sessionDidUpdate session: any UIDropSession)
                -> UIDropProposal
            {
                guard lease(session) != nil else { return UIDropProposal(operation: .forbidden) }
                libraryDragTrace(.hover)
                parent.context.target = parent.destination
                return UIDropProposal(operation: .move)
            }
            func dropInteraction(_ interaction: UIDropInteraction, sessionDidExit session: any UIDropSession) {
                if parent.context.target == parent.destination { parent.context.target = nil }
            }
            func dropInteraction(_ interaction: UIDropInteraction, sessionDidEnd session: any UIDropSession) {
                if parent.context.target == parent.destination { parent.context.target = nil }
            }
            func dropInteraction(_ interaction: UIDropInteraction, performDrop session: any UIDropSession) {
                libraryDragTrace(.performed)
                guard let lease = lease(session) else { return }
                parent.context.commit(
                    lease, into: parent.library, destination: parent.destination, onMutation: parent.onMutation)
            }
        }
    }

    private enum LibraryChatDragEvent: String, CaseIterable {
        case source, target, lift, liftAccepted, liftRejected, began, activated, activationRejected, admitted,
            rejected, hover, performed, commit, acknowledged, failed, moved, cancelled
    }

    @MainActor private func libraryDragTrace(_ event: LibraryChatDragEvent) {
        #if DEBUG
            LibraryChatDragTrace.shared.record(event.rawValue)
        #endif
    }

    #if DEBUG
        /// Numeric events only, never identifiers, titles or account metadata.
        /// Both explicit synthetic-fixture switches are required; normal app
        /// launches cannot enable these diagnostics.
        @MainActor @Observable final class LibraryChatDragTrace {
            static let shared = LibraryChatDragTrace()
            private let enabled: Bool
            private var counts: [String: Int] = [:]
            private init() {
                let arguments = ProcessInfo.processInfo.arguments
                enabled =
                    arguments.contains("--maple-library-controls-fixture")
                    && arguments.contains("--maple-library-drag-proof")
            }
            fileprivate func record(_ event: String) {
                guard enabled else { return }
                counts[event, default: 0] += 1
            }
            var summary: String {
                LibraryChatDragEvent.allCases.map { $0.rawValue + ":" + String(counts[$0.rawValue, default: 0]) }
                    .joined(separator: " ")
            }
        }
    #endif
#endif
