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
        var selectedChatID: String? = nil
        var activity: ConversationActivity = .idle
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
                            ConversationTitle(
                                conversation: conversation,
                                emphasized: selectionLibrary.selectedIDs.contains(conversation.id), activity: activity)
                            Spacer(minLength: 0)
                        }
                        .frame(minHeight: 50).contentShape(Rectangle()).foregroundStyle(.primary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("library.chat." + conversation.id)
                    .accessibilityLabel(
                        conversationActivityLabel(title: displayedTitle(conversation), activity: activity)
                    )
                    .accessibilityAddTraits(selectionLibrary.selectedIDs.contains(conversation.id) ? [.isSelected] : [])
                    .disabled(library.isMutating || selectionLibrary.isMutating)
                } else {
                    ConversationLibraryRow(
                        conversation: conversation, library: library, onSelect: onSelect, onMutation: onMutation,
                        onSelectForSelection: {
                            selectionLibrary.selectedIDs.insert(conversation.id); selecting = true
                        }, leadingInset: leadingInset, contextMenuEnabled: dragContext == nil || !dragEnabled,
                        selectedChatID: selectedChatID, activity: activity)
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
        var account: AccountEntitlements? = nil
        var onSettings: (@MainActor () -> Void)? = nil
        var showsWordmarkMenu = true
        var compactRootBar = false
        var nativeRootBarHidden = false
        var nativeRootBarTrailing: AnyView? = nil
        var selectionDeleteTitle = "Delete selected chats"
        @Environment(\.colorScheme) private var colorScheme
        @Environment(\.displayScale) private var displayScale
        @State private var moving = false
        @State private var deleting = false
        @State private var deletingAll = false
        func body(content: Content) -> some View {
            content
                .modifier(LibraryChatDeletionConfirmation())
                .toolbar {
                    if !compactRootBar {
                        if showsWordmarkMenu {
                            ToolbarItem(placement: .principal) { mapleMenu }
                                .sharedBackgroundVisibility(.hidden)
                        }
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
                                    libraryText(selectionDeleteTitle)
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
                    Text(
                        String(
                            localized: "Delete %lld chats?", defaultValue: "Delete \(library.selectedIDs.count) chats?",
                            table: "Library", bundle: .module)), isPresented: $deleting,
                    titleVisibility: .visible
                ) {
                    Button(role: .destructive) {
                        let ids = library.selectedIDs
                        Task {
                            if await library.deleteConversations(ids: ids) {
                                selecting = false; library.selectedIDs = []
                                await onMutation?()
                            }
                        }
                    } label: {
                        libraryText("Delete selected chats")
                    }
                } message: {
                    Text(
                        String(
                            localized:
                                "This will permanently delete %lld selected chats. This action cannot be undone.",
                            defaultValue:
                                "This will permanently delete \(library.selectedIDs.count) selected chats. This action cannot be undone.",
                            table: "Library", bundle: .module))
                }
                .sheet(isPresented: $moving, onDismiss: { Task { await onMutation?() } }) {
                    NavigationStack {
                        MoveChatsView(library: library, ids: library.selectedIDs) {
                            selecting = false; library.selectedIDs = []
                        }
                    }
                }
                .sheet(isPresented: $deletingAll) {
                    NavigationStack {
                        DeleteLibraryContentView(
                            title: "Delete all chats",
                            explanation: libraryText(
                                "This deletes every chat in this account, including chats in projects. Projects remain."
                            )
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
                                settingsIcon
                            }
                        }
                        .accessibilityIdentifier("library.settings")
                        .accessibilityLabel(settingsAccessibilityLabel)
                        .accessibilityHint(settingsAttentionHint)
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

        private var settingsAccessibilityLabel: Text {
            guard let attention = account.flatMap({ LibraryAccountAttention(team: $0.team.value) }) else {
                return libraryText("Settings")
            }
            return Text(
                String(
                    localized: "Settings, %@", defaultValue: "Settings, \(attention.accessibilityLabel)",
                    table: "Library", bundle: .module))
        }

        private var settingsAttentionHint: Text {
            guard let attention = account.flatMap({ LibraryAccountAttention(team: $0.team.value) }) else {
                return Text(verbatim: "")
            }
            return Text(verbatim: attention.accessibilityLabel)
        }

        private var settingsIcon: Image {
            guard let account, LibraryAccountAttention(team: account.team.value) != nil else {
                return Image(systemName: "gearshape")
            }
            // Native menus export one title and image from Label. Render the
            // existing badge into that image so its color survives menu export.
            let renderer = ImageRenderer(
                content: Image(systemName: "gearshape").font(.system(size: 19)).foregroundStyle(.primary)
                    .frame(width: 24, height: 24)
                    .overlay(alignment: .topTrailing) {
                        LibraryAccountAttentionBadge(account: account).font(.system(size: 10))
                            .background(.background, in: Circle()).offset(x: 3, y: -3)
                    }
                    .padding(3).environment(\.colorScheme, colorScheme))
            renderer.scale = displayScale
            guard let image = renderer.uiImage else { return Image(systemName: "exclamationmark.circle.fill") }
            return Image(uiImage: image).renderingMode(.original)
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
        var emphasized = false
        var activity: ConversationActivity = .idle
        var body: some View {
            let title = LibraryModel.title(conversation)
            HStack(spacing: 6) {
                if conversation.pinned {
                    Image(systemName: "pin").font(.caption).accessibilityHidden(true)
                }
                Group {
                    if title.isEmpty { libraryText("Untitled chat") } else { Text(verbatim: title) }
                }
                .fontWeight(emphasized ? .bold : .regular)
                .lineLimit(1).truncationMode(.tail)
                ConversationActivityIndicator(activity: activity)
            }
        }
    }

    struct ConversationActivityIndicator: View {
        let activity: ConversationActivity
        @ViewBuilder var body: some View {
            switch activity {
            case .idle: EmptyView()
            case .running:
                ProgressView().controlSize(.mini).tint(MapleAccent.action).accessibilityHidden(true)
            case .unread:
                Circle().fill(.green).frame(width: 8, height: 8).accessibilityHidden(true)
            }
        }
    }

    @MainActor private func displayedTitle(_ conversation: Conversation) -> String {
        let title = LibraryModel.title(conversation)
        return title.isEmpty ? String(localized: "Untitled chat", table: "Library", bundle: .module) : title
    }

    func conversationActivityLabel(title: String, activity: ConversationActivity) -> Text {
        switch activity {
        case .idle: Text(verbatim: title)
        case .running:
            Text(String(localized: "%@, running", defaultValue: "\(title), running", table: "Library", bundle: .module))
        case .unread:
            Text(
                String(
                    localized: "%@, completed, unread", defaultValue: "\(title), completed, unread", table: "Library",
                    bundle: .module))
        }
    }

    /// Like the project name form, rename remains open on validation or
    /// service failure; a dismissed form cannot apply a later completion.
    private struct RenameChatForm: View {
        let conversation: Conversation
        @Bindable var library: LibraryModel
        let onMutation: (@MainActor () async -> Void)?
        @Environment(\.dismiss) private var dismiss
        @State private var name = ""
        @State private var validation: String?
        @State private var submitting = false
        @State private var visible = false
        @FocusState private var focused: Bool

        var body: some View {
            Form {
                libraryText("Enter a new name for this chat conversation.")
                TextField(text: $name) { libraryText("Chat Title") }
                    .focused($focused).submitLabel(.done).onSubmit { submit() }
                    .accessibilityIdentifier("library.rename-title")
                if let validation {
                    libraryText(validation).foregroundStyle(MapleRedText())
                        .accessibilityIdentifier("library.rename-validation")
                }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .scrollContentBackground(.hidden).background(MapleSurface.canvas)
            .navigationTitle(libraryText("Rename Chat"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        dismiss()
                    } label: {
                        libraryText("Cancel")
                    }.disabled(submitting)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(action: submit) { libraryText("Rename Chat") }
                        .disabled(submitting || library.isMutating || !library.isActive)
                        .accessibilityIdentifier("library.rename-save")
                }
            }
            .interactiveDismissDisabled(submitting)
            .disabled(submitting || !library.isActive)
            .onAppear {
                visible = true; name = LibraryModel.title(conversation); focused = true
            }
            .onDisappear { visible = false }
            .onChange(of: name) { _, _ in validation = nil }
        }

        private func submit() {
            guard visible, library.isActive, !submitting, !library.isMutating else { return }
            let title = name.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !title.isEmpty else { validation = "Chat title cannot be empty."; return }
            guard title != LibraryModel.title(conversation).trimmingCharacters(in: .whitespacesAndNewlines) else {
                validation = "Please enter a different title."; return
            }
            validation = nil; submitting = true; focused = false
            Task { @MainActor in
                await library.renameConversation(id: conversation.id, title: title)
                guard visible, library.isActive else { return }
                submitting = false
                if library.issue == nil { await onMutation?(); if visible, library.isActive { dismiss() } }
            }
        }
    }

    /// A swipe cell can be recycled as its actions close. Keep confirmation on
    /// the list host, and capture the exact account owner before presenting it.
    @MainActor fileprivate struct LibraryChatDeletionRequest {
        let id: String
        let title: String
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
                    Text("Are you sure you want to delete this chat?", tableName: "Library", bundle: .module),
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
                } message: { request in
                    Text(
                        String(
                            localized: "This will permanently delete \"%@\". This action cannot be undone.",
                            defaultValue:
                                "This will permanently delete \"\(request.title)\". This action cannot be undone.",
                            table: "Library", bundle: .module))
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
        var selectedChatID: String? = nil
        var activity: ConversationActivity = .idle
        @Environment(\.requestChatDeletion) private var requestChatDeletion
        @State private var renaming = false
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
                    renaming = true
                } label: {
                    Label {
                        libraryText("Rename")
                    } icon: {
                        Image(systemName: "pencil")
                    }
                }
            }
            .disabled(library.isMutating)
            .sheet(isPresented: $renaming) {
                NavigationStack {
                    RenameChatForm(conversation: conversation, library: library, onMutation: onMutation)
                }
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
                ConversationTitle(
                    conversation: conversation, emphasized: selectedChatID == conversation.id, activity: activity
                )
                .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                .contentShape(Rectangle()).foregroundStyle(.primary)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("library.chat." + conversation.id)
            .accessibilityLabel(conversationActivityLabel(title: displayedTitle(conversation), activity: activity))
            .accessibilityAddTraits(selectedChatID == conversation.id ? [.isSelected] : [])
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
                renaming = true
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
                LibraryChatDeletionRequest(
                    id: conversation.id, title: displayedTitle(conversation), library: library, onMutation: onMutation))
        }
        private func changed() async { if library.issue == nil { await onMutation?() } }
    }

    struct MoveChatsView: View {
        @Bindable var library: LibraryModel
        let ids: Set<String>
        var onMoved: (@MainActor () -> Void)? = nil
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
                if await library.moveConversations(ids: ids, projectID: project) {
                    onMoved?(); dismiss()
                }
            }
        }
    }

    struct DeleteLibraryContentView: View {
        let title: String
        var actionTitle: String? = nil
        let explanation: Text
        let action: @MainActor () async -> LibraryIssue?
        @Environment(\.dismiss) private var dismiss
        @State private var confirmation = ""
        @State private var working = false
        @State private var issue: LibraryIssue?
        var body: some View {
            List {
                explanation
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
                    libraryText(actionTitle ?? title)
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
