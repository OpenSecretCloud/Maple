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
        @Environment(\.dismiss) private var dismiss
        @State private var renamed: String?
        @State private var name = ""
        @State private var renaming = false
        @State private var deleting = false
        @State private var instructionEditor: InstructionEditor?
        public init(
            project: ConversationProjectListItem, library: LibraryModel,
            makeInstructionEditor: @escaping @MainActor (InstructionDestination) -> InstructionEditor,
            onSelectChat: @escaping @MainActor (String) -> Void, onNewChat: @escaping @MainActor (String?) -> Void
        ) {
            self.project = project; self.library = library; self.makeInstructionEditor = makeInstructionEditor
            self.onSelectChat = onSelectChat; self.onNewChat = onNewChat
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
                    }
                    Button {
                        instructionEditor = makeInstructionEditor(.project(project.id))
                    } label: {
                        Label {
                            libraryText("Project instructions")
                        } icon: {
                            Image(systemName: "text.alignleft")
                        }
                    }
                }
                .listRowSeparator(.hidden).listRowBackground(Color.clear)
                Section {
                    ForEach(library.visibleConversations, id: \.id) { conversation in
                        ConversationLibraryRow(
                            conversation: conversation, library: library, onSelect: onSelectChat,
                            onMutation: { await library.refresh() })
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
            .modifier(LibraryChatDeletionConfirmation())
            .navigationTitle(renamed ?? project.name)
            .navigationDestination(
                isPresented: Binding(
                    get: { instructionEditor != nil }, set: { if !$0 { instructionEditor = nil } }
                )
            ) {
                if let instructionEditor { InstructionEditorView(editor: instructionEditor) }
            }
            .searchable(
                text: $library.searchText, prompt: Text("Search chats", tableName: "Library", bundle: .module)
            )
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button {
                            name = renamed ?? project.name; renaming = true
                        } label: {
                            libraryText("Rename project")
                        }
                        Button(role: .destructive) {
                            deleting = true
                        } label: {
                            libraryText("Delete project")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .accessibilityLabel(Text("Project actions", tableName: "Library", bundle: .module)).disabled(
                        library.isMutating)
                }
            }
            .alert(Text("Rename project", tableName: "Library", bundle: .module), isPresented: $renaming) {
                TextField(text: $name) { libraryText("Project name") }
                Button(role: .cancel) {
                } label: {
                    libraryText("Cancel")
                }
                Button {
                    Task {
                        await library.renameProject(id: project.id, name: name);
                        if library.issue == nil { renamed = name }
                    }
                } label: {
                    libraryText("Save")
                }
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
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
                        if library.issue == nil { dismiss() }
                        return library.issue
                    }
                }
            }
            .task { await library.load(.project(project.id)) }
            .refreshable { await library.load(.project(project.id)) }
        }
    }
#endif
