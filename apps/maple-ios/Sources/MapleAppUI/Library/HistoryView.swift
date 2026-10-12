#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import SwiftUI

    #if DEBUG
        /// A synthetic list host exercises the shipping selection and deletion controls.
        /// The application itself presents chats only in its Library menu.
        struct HistoryView: View {
            @Bindable var library: LibraryModel
            let onSelectChat: @MainActor (String) -> Void
            @State private var selecting = false
            var body: some View {
                List {
                    ForEach(library.visibleConversations, id: \.id) { conversation in
                        LibrarySelectableConversationRow(
                            conversation: conversation, library: library, selectionLibrary: library,
                            selecting: $selecting, onSelect: onSelectChat)
                    }
                    if library.hasMore {
                        Button {
                            Task { await library.loadMore() }
                        } label: {
                            libraryText("Load more")
                        }
                        .disabled(library.isLoading)
                    }
                    if !library.isLoading, library.conversations.isEmpty, library.issue == nil {
                        libraryText("No chats yet")
                    }
                    if let issue = library.issue { LibraryIssueView(issue: issue) }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .background(MapleSurface.sidebar)
                .accessibilityIdentifier("library.fixture.chat-list")
                .modifier(LibraryChatManagement(library: library, selecting: $selecting))
                .task { await library.load(.all) }
                .refreshable { await library.load(.all) }
            }
        }
    #endif

    struct LibrarySelectableConversationRow: View {
        let conversation: Conversation
        @Bindable var library: LibraryModel
        @Bindable var selectionLibrary: LibraryModel
        @Binding var selecting: Bool
        let onSelect: @MainActor (String) -> Void
        var onMutation: (@MainActor () async -> Void)? = nil
        var leadingInset: CGFloat = 20
        var dragContext: LibraryChatDragContext? = nil
        var dragEnabled = true
        var dropDestination: LibraryConversationDestination? = nil
        var body: some View {
            Group {
                if selecting {
                    Button {
                        if selectionLibrary.selectedIDs.contains(conversation.id) {
                            selectionLibrary.selectedIDs.remove(conversation.id)
                        } else {
                            selectionLibrary.selectedIDs.insert(conversation.id)
                        }
                    } label: {
                        HStack(spacing: 12) {
                            Image(
                                systemName: selectionLibrary.selectedIDs.contains(conversation.id)
                                    ? "checkmark.circle.fill" : "circle")
                            ConversationTitle(conversation: conversation)
                            Spacer(minLength: 0)
                        }
                        .frame(minHeight: 50).contentShape(Rectangle()).foregroundStyle(.primary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("library.chat." + conversation.id)
                    .accessibilityAddTraits(selectionLibrary.selectedIDs.contains(conversation.id) ? [.isSelected] : [])
                    .disabled(library.isMutating || selectionLibrary.isMutating)
                } else {
                    ConversationLibraryRow(
                        conversation: conversation, library: library, onSelect: onSelect, onMutation: onMutation,
                        onSelectForSelection: {
                            selectionLibrary.selectedIDs.insert(conversation.id); selecting = true
                        }, leadingInset: leadingInset, contextMenuEnabled: dragContext == nil || !dragEnabled)
                }
            }
            .modifier(
                LibraryChatDragSource(
                    context: dragContext, library: library, id: conversation.id,
                    enabled: dragEnabled && !selecting && !selectionLibrary.isMutating)
            )
            .modifier(
                LibraryChatDropTarget(
                    context: dragContext, library: selectionLibrary, destination: dropDestination,
                    enabled: dragEnabled && !selecting, onMutation: onMutation)
            )
            .listRowInsets(.vertical, 0)
            .listRowInsets(.leading, leadingInset).listRowInsets(.trailing, 12)
            .listRowSeparatorTint(MapleSurface.border.opacity(0.28))
            .listRowSeparator(.visible).listRowBackground(Color.clear)
        }
    }

    struct LibraryChatManagement: ViewModifier {
        @Bindable var library: LibraryModel
        @Binding var selecting: Bool
        var onMutation: (@MainActor () async -> Void)? = nil
        var onSettings: (@MainActor () -> Void)? = nil
        var compactRootBar = false
        var nativeRootBarHidden = false
        var nativeRootBarTrailing: AnyView? = nil
        @State private var moving = false
        @State private var deleting = false
        @State private var deletingAll = false
        func body(content: Content) -> some View {
            content
                .modifier(LibraryChatDeletionConfirmation())
                .toolbar {
                    if !compactRootBar {
                        ToolbarItem(placement: .principal) { mapleMenu }
                            .sharedBackgroundVisibility(.hidden)
                        if selecting {
                            ToolbarItem(placement: .topBarLeading) { selectionDone }
                        }
                    }
                }
                .modifier(
                    LibraryCompactRootBar(
                        enabled: compactRootBar, hidden: nativeRootBarHidden,
                        leading: selecting ? AnyView(selectionDone) : nil,
                        principal: AnyView(mapleMenu), trailing: nativeRootBarTrailing)
                )
                .safeAreaInset(edge: .bottom) {
                    if selecting {
                        VStack(spacing: 8) {
                            Button {
                                moving = true
                            } label: {
                                Label {
                                    libraryText("Move selected chats")
                                } icon: {
                                    Image(systemName: "folder")
                                }
                                .frame(maxWidth: .infinity, minHeight: 44)
                            }
                            Button(role: .destructive) {
                                deleting = true
                            } label: {
                                Label {
                                    libraryText("Delete selected chats")
                                } icon: {
                                    Image(systemName: "trash")
                                }
                                .frame(maxWidth: .infinity, minHeight: 44)
                            }
                        }
                        .padding(8).background(MapleSurface.sidebar)
                        .disabled(library.selectedIDs.isEmpty || library.isMutating)
                    }
                }
                .confirmationDialog(
                    Text("Delete selected chats?", tableName: "Library", bundle: .module), isPresented: $deleting,
                    titleVisibility: .visible
                ) {
                    Button(role: .destructive) {
                        let ids = library.selectedIDs
                        Task {
                            await library.deleteConversations(ids: ids)
                            if library.issue == nil { await onMutation?() }
                        }
                    } label: {
                        libraryText("Delete selected chats")
                    }
                } message: {
                    libraryText("Deleted chats cannot be restored.")
                }
                .sheet(isPresented: $moving, onDismiss: { Task { await onMutation?() } }) {
                    NavigationStack { MoveChatsView(library: library, ids: library.selectedIDs) }
                }
                .sheet(isPresented: $deletingAll) {
                    NavigationStack {
                        DeleteLibraryContentView(
                            title: "Delete all chats",
                            explanation:
                                "This deletes every chat in this account, including chats in projects. Projects remain."
                        ) {
                            await library.deleteAllConversations()
                            let issue = library.issue
                            await onMutation?()
                            return issue
                        }
                    }
                }
        }

        private var mapleMenu: some View {
            Menu {
                Section {
                    if let onSettings {
                        Button(action: onSettings) {
                            Label {
                                libraryText("Settings")
                            } icon: {
                                Image(systemName: "gearshape")
                            }
                        }
                        .accessibilityIdentifier("library.settings")
                    }
                    Button {
                        selecting.toggle(); if !selecting { library.selectedIDs = [] }
                    } label: {
                        Label {
                            libraryText(selecting ? "Done selecting" : "Select chats")
                        } icon: {
                            Image(systemName: selecting ? "checkmark" : "checkmark.circle")
                        }
                    }
                }
                Section {
                    Button(role: .destructive) {
                        deletingAll = true
                    } label: {
                        Label {
                            libraryText("Delete all chats")
                        } icon: {
                            Image(systemName: "trash")
                        }
                    }
                }
            } label: {
                MapleWordmark().frame(width: 103, height: 20)
                    .overlay(alignment: .trailing) {
                        Image(systemName: "chevron.down").font(.caption2.weight(.semibold))
                            .foregroundStyle(MapleAccent.action).offset(x: 29).accessibilityHidden(true)
                    }
                    .padding(.horizontal, 30)
                    .frame(minHeight: 44).contentShape(Rectangle())
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .frame(minHeight: 44)
            .accessibilityLabel(libraryText("Maple menu"))
            .accessibilityIdentifier("library.actions")
            .disabled(library.isMutating)
        }

        private var selectionDone: some View {
            Button {
                selecting = false; library.selectedIDs = []
            } label: {
                libraryText("Done")
            }
            .accessibilityIdentifier("library.selection-done")
            .disabled(library.isMutating)
        }
    }

    private struct LibraryCompactRootBar: ViewModifier {
        let enabled: Bool
        let hidden: Bool
        let leading: AnyView?
        let principal: AnyView
        let trailing: AnyView?

        func body(content: Content) -> some View {
            if enabled {
                content.compactMenuRootBar(
                    role: .library, hidden: hidden, leading: leading, principal: principal, trailing: trailing)
            } else {
                content
            }
        }
    }

    struct ConversationTitle: View {
        let conversation: Conversation
        var body: some View {
            let title = LibraryModel.title(conversation)
            Group {
                if title.isEmpty { libraryText("Untitled chat") } else { Text(verbatim: title) }
            }
            .lineLimit(1).truncationMode(.tail)
        }
    }

    /// A swipe cell can be recycled as its actions close. Keep confirmation on
    /// the list host, and capture the exact account owner before presenting it.
    @MainActor fileprivate struct LibraryChatDeletionRequest {
        let id: String
        let library: LibraryModel
        let onMutation: (@MainActor () async -> Void)?
    }

    private struct LibraryChatDeletionRequestKey: EnvironmentKey {
        static let defaultValue: @MainActor @Sendable (LibraryChatDeletionRequest) -> Void = { _ in }
    }

    extension EnvironmentValues {
        fileprivate var requestChatDeletion: @MainActor @Sendable (LibraryChatDeletionRequest) -> Void {
            get { self[LibraryChatDeletionRequestKey.self] }
            set { self[LibraryChatDeletionRequestKey.self] = newValue }
        }
    }

    struct LibraryChatDeletionConfirmation: ViewModifier {
        @State private var pending: LibraryChatDeletionRequest?
        func body(content: Content) -> some View {
            content
                .environment(\.requestChatDeletion) { request in
                    guard pending == nil, request.library.isActive, !request.library.isMutating else { return }
                    pending = request
                }
                .alert(
                    Text("Delete chat?", tableName: "Library", bundle: .module),
                    isPresented: Binding(
                        get: { pending != nil }, set: { if !$0 { pending = nil } }),
                    presenting: pending
                ) { request in
                    Button(role: .cancel) {
                        pending = nil
                    } label: {
                        libraryText("Cancel")
                    }
                    Button(role: .destructive) {
                        pending = nil
                        Task {
                            guard request.library.isActive else { return }
                            await request.library.deleteConversations(ids: [request.id])
                            if request.library.isActive, request.library.issue == nil { await request.onMutation?() }
                        }
                    } label: {
                        libraryText("Delete")
                    }
                } message: { _ in
                    libraryText("Deleted chats cannot be restored.")
                }
                .onChange(of: pending?.library.isActive) { _, active in
                    if active == false { pending = nil }
                }
                .onDisappear { pending = nil }
        }
    }

    struct ConversationLibraryRow: View {
        let conversation: Conversation
        @Bindable var library: LibraryModel
        let onSelect: @MainActor (String) -> Void
        var onMutation: (@MainActor () async -> Void)? = nil
        var onSelectForSelection: (@MainActor () -> Void)? = nil
        var leadingInset: CGFloat = 20
        var contextMenuEnabled = true
        @Environment(\.requestChatDeletion) private var requestChatDeletion
        @State private var renaming = false
        @State private var name = ""
        @State private var moving = false
        var body: some View {
            HStack(spacing: 0) {
                titleButton
                Menu {
                    actions
                } label: {
                    Image(systemName: "ellipsis").font(.body.weight(.semibold)).foregroundStyle(MapleAccent.action)
                        .frame(width: 44, height: 44).contentShape(Rectangle())
                }
                .menuStyle(.button).buttonStyle(.plain)
                .accessibilityLabel(libraryText("Chat actions"))
                .accessibilityIdentifier("library.chat-actions." + conversation.id)
            }
            .listRowInsets(.vertical, 0)
            .listRowInsets(.leading, leadingInset).listRowInsets(.trailing, 12)
            .listRowSeparatorTint(MapleSurface.border.opacity(0.28))
            .listRowSeparator(.visible).listRowBackground(Color.clear)
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                // A destructive swipe role optimistically removes its cell.
                // Only the separate confirmation action may delete anything.
                Button {
                    requestDeletion()
                } label: {
                    Label {
                        libraryText("Delete")
                    } icon: {
                        Image(systemName: "trash")
                    }
                }
                .tint(.red)
                .accessibilityIdentifier("library.swipe-delete." + conversation.id)
                Button {
                    name = LibraryModel.title(conversation); renaming = true
                } label: {
                    Label {
                        libraryText("Rename")
                    } icon: {
                        Image(systemName: "pencil")
                    }
                }
            }
            .disabled(library.isMutating)
            .alert(Text("Rename chat", tableName: "Library", bundle: .module), isPresented: $renaming) {
                TextField(text: $name) { libraryText("Chat name") }
                Button(role: .cancel) {
                } label: {
                    libraryText("Cancel")
                }
                Button {
                    Task {
                        await library.renameConversation(id: conversation.id, title: name); await changed()
                    }
                } label: {
                    libraryText("Save")
                }
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .sheet(isPresented: $moving, onDismiss: { Task { await changed() } }) {
                NavigationStack { MoveChatsView(library: library, ids: [conversation.id]) }
            }
        }
        @ViewBuilder private var titleButton: some View {
            if contextMenuEnabled {
                conversationButton.contextMenu { actions }
            } else {
                // The row's long press starts a drag. Its visible ellipsis and
                // swipe actions remain available without a competing menu.
                conversationButton
            }
        }
        private var conversationButton: some View {
            Button {
                onSelect(conversation.id)
            } label: {
                ConversationTitle(conversation: conversation)
                    .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                    .contentShape(Rectangle()).foregroundStyle(.primary)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("library.chat." + conversation.id)
        }
        @ViewBuilder private var actions: some View {
            if let onSelectForSelection {
                Button(action: onSelectForSelection) {
                    Label {
                        libraryText("Select")
                    } icon: {
                        Image(systemName: "checkmark.circle")
                    }
                }
            }
            Button {
                Task {
                    await library.setPinned(id: conversation.id, pinned: !conversation.pinned); await changed()
                }
            } label: {
                Label {
                    libraryText(conversation.pinned ? "Unpin" : "Pin")
                } icon: {
                    Image(systemName: conversation.pinned ? "pin.slash" : "pin")
                }
            }
            Button {
                moving = true
            } label: {
                Label {
                    libraryText("Move to project")
                } icon: {
                    Image(systemName: "folder")
                }
            }
            Button {
                name = LibraryModel.title(conversation); renaming = true
            } label: {
                Label {
                    libraryText("Rename")
                } icon: {
                    Image(systemName: "pencil")
                }
            }
            Button(role: .destructive) {
                requestDeletion()
            } label: {
                Label {
                    libraryText("Delete")
                } icon: {
                    Image(systemName: "trash")
                }
            }
        }
        private func requestDeletion() {
            requestChatDeletion(
                LibraryChatDeletionRequest(id: conversation.id, library: library, onMutation: onMutation))
        }
        private func changed() async { if library.issue == nil { await onMutation?() } }
    }

    struct MoveChatsView: View {
        @Bindable var library: LibraryModel
        let ids: Set<String>
        @Environment(\.dismiss) private var dismiss
        var body: some View {
            List {
                Button {
                    move(nil)
                } label: {
                    libraryText("No project")
                }
                ForEach(library.projects, id: \.id) { project in Button(project.name) { move(project.id) } }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .disabled(library.isMutating)
            .navigationTitle(Text("Move to project", tableName: "Library", bundle: .module))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        dismiss()
                    } label: {
                        libraryText("Cancel")
                    }.disabled(library.isMutating)
                }
            }
            .interactiveDismissDisabled(library.isMutating)
            .task { await library.loadProjects() }
        }
        private func move(_ project: String?) {
            Task {
                await library.moveConversations(ids: ids, projectID: project); if library.issue == nil { dismiss() }
            }
        }
    }

    struct DeleteLibraryContentView: View {
        let title: String
        let explanation: String
        let action: @MainActor () async -> LibraryIssue?
        @Environment(\.dismiss) private var dismiss
        @State private var confirmation = ""
        @State private var working = false
        @State private var issue: LibraryIssue?
        var body: some View {
            List {
                libraryText(explanation)
                TextField(
                    text: $confirmation,
                    prompt: libraryText("Type DELETE to confirm").foregroundStyle(MapleFieldPrompt())
                ) { libraryText("Type DELETE to confirm") }
                .textInputAutocapitalization(.characters).autocorrectionDisabled()
                .accessibilityIdentifier("library.delete-confirmation")
                Button(role: .destructive) {
                    working = true
                    Task {
                        issue = await action(); working = false; if issue == nil { dismiss() }
                    }
                } label: {
                    libraryText(title)
                }.disabled(confirmation != "DELETE" || working)
                    .accessibilityIdentifier("library.delete-confirm")
                if let issue { LibraryIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(libraryText(title))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        dismiss()
                    } label: {
                        libraryText("Cancel")
                    }.disabled(working)
                }
            }
            .interactiveDismissDisabled(working)
        }
    }
#endif
