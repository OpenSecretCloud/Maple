#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import SwiftUI

    public struct ProjectView: View {
        private let project: ConversationProjectListItem
        @Bindable private var library: LibraryModel
        private let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        private let onSelectChat: @MainActor (String) -> Void
        private let onNewChat: @MainActor (String?) -> Void
        private let selectedChatID: String?
        private let conversationActivity: @MainActor (String) -> ConversationActivity
        private let onMutation: (@MainActor () async -> Void)?
        private let menuSearch: String
        @Environment(\.dismiss) private var dismiss
        @State private var renamed: String?
        @State private var renaming = false
        @State private var deleting = false
        @State private var selecting = false
        @State private var didInitialize = false
        @State private var search = ""
        @State private var editingInstructions = false
        @State private var instructionEditor: InstructionEditor?
        public init(
            project: ConversationProjectListItem, library: LibraryModel,
            makeInstructionEditor: @escaping @MainActor (InstructionDestination) -> InstructionEditor,
            onSelectChat: @escaping @MainActor (String) -> Void, onNewChat: @escaping @MainActor (String?) -> Void,
            selectedChatID: String? = nil,
            conversationActivity: @escaping @MainActor (String) -> ConversationActivity = { _ in .idle },
            onMutation: (@MainActor () async -> Void)? = nil
        ) {
            self.project = project; self.library = library; self.makeInstructionEditor = makeInstructionEditor
            self.onSelectChat = onSelectChat; self.onNewChat = onNewChat
            self.selectedChatID = selectedChatID; self.conversationActivity = conversationActivity
            self.onMutation = onMutation; menuSearch = library.searchText
        }
        public var body: some View {
            List {
                Section {
                    Button {
                        onNewChat(project.id)
                    } label: {
                        Label {
                            libraryText("New chat in project")
                        } icon: {
                            Image(systemName: "square.and.pencil")
                        }
                        .frame(minHeight: 44)
                    }
                    .accessibilityIdentifier("library.project-detail.new-chat")
                    instructionsCard
                }
                .listRowSeparator(.hidden).listRowBackground(Color.clear)
                Section {
                    ForEach(library.visibleConversations, id: \.id) { conversation in
                        LibrarySelectableConversationRow(
                            conversation: conversation, library: library, selectionLibrary: library,
                            selecting: $selecting, onSelect: onSelectChat, onMutation: changed,
                            selectedChatID: selectedChatID, activity: conversationActivity(conversation.id))
                    }
                    if library.hasMore {
                        Button {
                            Task { await library.loadMore() }
                        } label: {
                            libraryText("Load more")
                        }.disabled(library.isLoading)
                    }
                    if !library.isLoading, library.conversations.isEmpty, library.issue == nil {
                        libraryText("No chats yet")
                    }
                    if let issue = library.issue { LibraryIssueView(issue: issue) }
                } header: {
                    libraryText("Chats").font(.headline)
                }
                .textCase(nil).listRowSeparator(.hidden).listRowBackground(Color.clear)
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.sidebar)
            .modifier(
                LibraryChatManagement(
                    library: library, selecting: $selecting, onMutation: changed, showsWordmarkMenu: false)
            )
            .navigationTitle(renamed ?? project.name)
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier("library.project-detail." + project.id)
            .navigationDestination(isPresented: $editingInstructions) {
                if let instructionEditor { InstructionEditorView(editor: instructionEditor) }
            }
            .searchable(text: $search, prompt: libraryText("Search chats"))
            .onChange(of: search) { _, value in library.searchText = value }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button {
                            selecting.toggle(); if !selecting { library.selectedIDs = [] }
                        } label: {
                            libraryText(selecting ? "Done selecting" : "Select chats")
                        }
                        Button {
                            renaming = true
                        } label: {
                            libraryText("Rename project")
                        }
                        Button(role: .destructive) {
                            deleting = true
                        } label: {
                            libraryText("Delete project")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle").frame(minWidth: 44, minHeight: 44)
                    }
                    .accessibilityLabel(libraryText("Project actions"))
                    .accessibilityIdentifier("library.project-detail.actions")
                    .disabled(library.isMutating)
                }
            }
            .sheet(isPresented: $renaming) {
                NavigationStack {
                    ProjectNameForm(
                        library: library, projectID: project.id, isCurrent: { library.isActive }, onCreated: nil,
                        initialName: renamed ?? project.name,
                        onRenamed: { value in
                            renamed = value; Task { await changed() }
                        }, isRoot: true)
                }
            }
            .sheet(isPresented: $deleting) {
                NavigationStack {
                    DeleteLibraryContentView(
                        title: "Delete this project?",
                        explanation: Text(
                            String(
                                localized:
                                    "This will permanently delete \"%@\" and any chats in it. Move chats out first if you want to keep them. This action cannot be undone.",
                                defaultValue:
                                    "This will permanently delete \"\(renamed ?? project.name)\" and any chats in it. Move chats out first if you want to keep them. This action cannot be undone.",
                                table: "Library", bundle: .module))
                    ) {
                        await library.deleteProject(id: project.id)
                        if library.issue == nil { await onMutation?(); dismiss() }
                        return library.issue
                    }
                }
            }
            .task {
                if !didInitialize {
                    library.selectedIDs = []; didInitialize = true
                }
                library.searchText = search
                await library.load(.project(project.id))
                guard library.isActive else { return }
                if instructionEditor == nil { instructionEditor = makeInstructionEditor(.project(project.id)) }
                await instructionEditor?.load()
            }
            .refreshable {
                await library.load(.project(project.id))
                await instructionEditor?.load()
            }
            .onDisappear { library.searchText = menuSearch }
        }
        private var instructionsCard: some View {
            Button {
                editingInstructions = true
            } label: {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        libraryText("Custom Instructions").font(.headline)
                        Spacer()
                        Image(systemName: "ellipsis").accessibilityHidden(true)
                    }
                    Text(
                        String(
                            localized: "Shared instructions for chats in %@",
                            defaultValue: "Shared instructions for chats in \(renamed ?? project.name)",
                            table: "Library", bundle: .module)
                    )
                    .font(.footnote).foregroundStyle(.secondary)
                    if let instructionEditor, instructionEditor.isLoaded {
                        if instructionEditor.text.isEmpty {
                            libraryText("Set shared custom instructions that apply to chats in this project.")
                                .foregroundStyle(.secondary)
                        } else {
                            Text(verbatim: instructionEditor.text).foregroundStyle(.secondary)
                        }
                    } else if let issue = instructionEditor?.issue {
                        LibraryIssueView(issue: issue)
                    } else {
                        ProgressView().accessibilityLabel(libraryText("Loading instructions"))
                    }
                }
                .padding(12).frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                .background(MapleSurface.canvas, in: RoundedRectangle(cornerRadius: 12))
                .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(MapleSurface.border) }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain).foregroundStyle(.primary)
            .disabled(instructionEditor == nil || !library.isActive)
            .accessibilityIdentifier("library.project-detail.instructions")
        }
        private func changed() async {
            await onMutation?()
            if library.isActive { library.searchText = search }
        }
    }
#endif
