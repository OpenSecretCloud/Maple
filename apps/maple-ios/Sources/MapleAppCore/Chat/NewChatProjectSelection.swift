import Foundation

/// A New Chat project belongs to the selection that requested it, even when
/// loading suspends across another navigation or an account replacement.
@MainActor public enum NewChatProjectSelection {
    public struct Current {
        public let accountScope: AccountScope?
        public let navigation: UUID
        public let selectedKey: String
        public let runtime: ChatRuntime?

        public init(accountScope: AccountScope?, navigation: UUID, selectedKey: String, runtime: ChatRuntime?) {
            self.accountScope = accountScope
            self.navigation = navigation
            self.selectedKey = selectedKey
            self.runtime = runtime
        }
    }

    public static func perform(
        scope: AccountScope, projectID: String?,
        navigate: @MainActor (UUID) async -> Bool,
        current: @MainActor () -> Current,
        assign: @MainActor (ChatRuntime, String?) -> Void
    ) async {
        let ticket = UUID()
        guard await navigate(ticket), !Task.isCancelled else { return }
        let selected = current()
        guard selected.accountScope == scope, selected.navigation == ticket,
            selected.selectedKey == ChatSnapshot.newChatKey,
            let runtime = selected.runtime, runtime.scope == scope, !runtime.isRetired,
            runtime.conversationID == nil, runtime.snapshot.storageKey == ChatSnapshot.newChatKey
        else { return }
        // No suspension between admission and mutation of this exact runtime.
        assign(runtime, projectID)
    }
}
