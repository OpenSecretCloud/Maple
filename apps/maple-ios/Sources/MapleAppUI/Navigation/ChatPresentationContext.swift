#if os(iOS)
    import SwiftUI

    private struct ChatPresentationContextKey: EnvironmentKey {
        static let defaultValue: @MainActor @Sendable (UUID, Bool) -> Void = { _, _ in }
    }

    extension EnvironmentValues {
        var chatPresentationChanged: @MainActor @Sendable (UUID, Bool) -> Void {
            get { self[ChatPresentationContextKey.self] }
            set { self[ChatPresentationContextKey.self] = newValue }
        }
    }

    extension View {
        /// Each sheet/picker owns a separate lease; dismissing one cannot expose
        /// a chat still covered by another presentation or a replacement account.
        func tracksChatPresentation(_ presented: Bool) -> some View {
            modifier(ChatPresentationLease(presented: presented))
        }

        func chatFullScreenCover<Item: Identifiable, Cover: View>(
            item: Binding<Item?>, @ViewBuilder content: @escaping (Item) -> Cover
        ) -> some View {
            modifier(ChatFullScreenPresentation(item: item, makeCover: content))
        }
    }

    private struct ChatFullScreenPresentation<Item: Identifiable, Cover: View>: ViewModifier {
        @Binding var item: Item?
        let makeCover: (Item) -> Cover
        @State private var lease = ChatPresentationLeaseOwner()
        @Environment(\.chatPresentationChanged) private var changed
        @Environment(\.compactMenuPresentationDismissed) private var compactMenuPresentationDismissed
        func body(content: Content) -> some View {
            content
                .fullScreenCover(
                    item: $item,
                    onDismiss: {
                        lease.update(item != nil, changed: changed)
                        // Binding changes can release the lease before native
                        // dismissal completes; notify even if it is already inactive.
                        compactMenuPresentationDismissed()
                    }
                ) { value in
                    // Retain the lease with the presented content too: lazy
                    // transcript eviction must not release a still-visible cover.
                    RetainedPresentationContent(lease: lease, content: makeCover(value))
                }
                .onChange(of: item != nil, initial: true) { _, value in lease.update(value, changed: changed) }
                .onAppear { lease.update(item != nil, changed: changed) }
        }
    }

    private struct RetainedPresentationContent<Content: View>: View {
        let lease: ChatPresentationLeaseOwner
        let content: Content
        var body: some View { content }
    }

    private struct ChatPresentationLease: ViewModifier {
        let presented: Bool
        @State private var lease = ChatPresentationLeaseOwner()
        @Environment(\.chatPresentationChanged) private var changed
        func body(content: Content) -> some View {
            content
                .onChange(of: presented, initial: true) { _, value in lease.update(value, changed: changed) }
                .onAppear { lease.update(presented, changed: changed) }
        }
    }

    /// A full-screen cover may hide its presenter without destroying its state.
    /// Keep the lease until dismissal or actual presenter destruction.
    @MainActor private final class ChatPresentationLeaseOwner {
        private let id = UUID()
        private var presented = false
        private var changed: (@MainActor @Sendable (UUID, Bool) -> Void)?
        func update(_ value: Bool, changed: @escaping @MainActor @Sendable (UUID, Bool) -> Void) {
            self.changed = changed
            // Layout candidates can appear repeatedly without presenting anything.
            // Publish only a transition, never an initial or repeated inactive lease.
            guard presented != value else { return }
            presented = value
            changed(id, value)
        }
        deinit {
            guard presented, let changed else { return }
            let id = id
            Task { @MainActor in changed(id, false) }
        }
    }
#endif
