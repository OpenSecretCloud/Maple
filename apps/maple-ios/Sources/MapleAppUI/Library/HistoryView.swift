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
                            selecting: selecting, onSelect: onSelectChat)
                    }
                    if library.isLoading { ProgressView() }
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
        let selecting: Bool
        let onSelect: @MainActor (String) -> Void
        var onMutation: (@MainActor () async -> Void)? = nil
        var body: some View {
            if selecting {
                Button {
                    if selectionLibrary.selectedIDs.contains(conversation.id) {
                        selectionLibrary.selectedIDs.remove(conversation.id)
                    } else {
                        selectionLibrary.selectedIDs.insert(conversation.id)
                    }
                } label: {
                    HStack {
                        Image(
                            systemName: selectionLibrary.selectedIDs.contains(conversation.id)
                                ? "checkmark.circle.fill" : "circle")
                        ConversationTitle(conversation: conversation)
                    }.foregroundStyle(.primary)
                }
                .accessibilityIdentifier("library.chat." + conversation.id)
                .accessibilityAddTraits(selectionLibrary.selectedIDs.contains(conversation.id) ? [.isSelected] : [])
                .disabled(library.isMutating || selectionLibrary.isMutating)
            } else {
                ConversationLibraryRow(
                    conversation: conversation, library: library, onSelect: onSelect, onMutation: onMutation)
            }
        }
    }

    struct LibraryChatManagement: ViewModifier {
        @Bindable var library: LibraryModel
        @Binding var selecting: Bool
        var onMutation: (@MainActor () async -> Void)? = nil
        @State private var moving = false
        @State private var deleting = false
        @State private var deletingAll = false
        func body(content: Content) -> some View {
            content
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) {
                        Menu {
                            Button {
                                selecting.toggle(); if !selecting { library.selectedIDs = [] }
                            } label: {
                                libraryText(selecting ? "Done selecting" : "Select chats")
                            }
                            Button(role: .destructive) {
                                deletingAll = true
                            } label: {
                                libraryText("Delete all chats")
                            }
                        } label: {
                            Image(systemName: "ellipsis").frame(minWidth: 44, minHeight: 44)
                        }
                        .accessibilityLabel(libraryText("Chat actions"))
                        .accessibilityIdentifier("library.actions")
                        .disabled(library.isMutating)
                    }
                }
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
                            await library.deleteConversations(ids: ids); await onMutation?()
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
    }

    struct ConversationTitle: View {
        let conversation: Conversation
        var body: some View {
            let title = LibraryModel.title(conversation)
            if title.isEmpty { libraryText("Untitled chat") } else { Text(title) }
        }
    }

    struct ConversationLibraryRow: View {
        let conversation: Conversation
        @Bindable var library: LibraryModel
        let onSelect: @MainActor (String) -> Void
        var onMutation: (@MainActor () async -> Void)? = nil
        @State private var renaming = false
        @State private var name = ""
        @State private var moving = false
        @State private var deleting = false
        var body: some View {
            Button {
                onSelect(conversation.id)
            } label: {
                HStack {
                    ConversationTitle(conversation: conversation).multilineTextAlignment(.leading)
                    Spacer(minLength: 8)
                    if conversation.pinned {
                        Image(systemName: "pin.fill").foregroundStyle(.secondary).accessibilityLabel(
                            Text("Pinned", tableName: "Library", bundle: .module))
                    }
                }.foregroundStyle(.primary)
            }
            .accessibilityIdentifier("library.chat." + conversation.id)
            .contextMenu { actions }
            .swipeActions(edge: .trailing) {
                Button(role: .destructive) {
                    deleting = true
                } label: {
                    Label {
                        libraryText("Delete")
                    } icon: {
                        Image(systemName: "trash")
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
            }
            .swipeActions(edge: .leading) {
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
            .confirmationDialog(
                Text("Delete chat?", tableName: "Library", bundle: .module), isPresented: $deleting,
                titleVisibility: .visible
            ) {
                Button(role: .destructive) {
                    Task {
                        await library.deleteConversations(ids: [conversation.id]); await changed()
                    }
                } label: {
                    libraryText("Delete")
                }
            } message: {
                libraryText("Deleted chats cannot be restored.")
            }
            .sheet(isPresented: $moving, onDismiss: { Task { await changed() } }) {
                NavigationStack { MoveChatsView(library: library, ids: [conversation.id]) }
            }
        }
        @ViewBuilder private var actions: some View {
            Button {
                name = LibraryModel.title(conversation); renaming = true
            } label: {
                Label {
                    libraryText("Rename")
                } icon: {
                    Image(systemName: "pencil")
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
                    Image(systemName: "pin")
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
            Button(role: .destructive) {
                deleting = true
            } label: {
                Label {
                    libraryText("Delete")
                } icon: {
                    Image(systemName: "trash")
                }
            }
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
                if library.isLoadingProjects { ProgressView() }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
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
            Form {
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
                if working { ProgressView() }
                if let issue { LibraryIssueView(issue: issue) }
            }
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
