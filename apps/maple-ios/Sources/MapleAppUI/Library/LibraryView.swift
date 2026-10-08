#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI

    @MainActor @Observable private final class LibraryMenuModels {
        let pinned: LibraryModel
        let recent: LibraryModel
        var projects: [String: LibraryModel] = [:]
        private let makeLibrary: @MainActor () -> LibraryModel
        init(makeLibrary: @escaping @MainActor () -> LibraryModel) {
            self.makeLibrary = makeLibrary; pinned = makeLibrary(); recent = makeLibrary()
        }
        func project(_ id: String) -> LibraryModel {
            if let current = projects[id] { return current }
            let value = makeLibrary(); projects[id] = value; return value
        }
        func refresh() async {
            await pinned.load(.pinned); await recent.load(.unassigned)
            for (id, model) in projects { await model.load(.project(id)) }
        }
    }

    private enum LibraryDrawerDestination: Hashable {
        case settings, project(String)
    }

    /// Compact navigation uses this full-scene root beneath the retained chat
    /// page. Regular widths retain the NavigationSplitView sidebar.
    public struct LibraryView: View {
        @Bindable private var library: LibraryModel
        @State private var models: LibraryMenuModels
        @State private var search = ""
        @State private var projectName = ""
        @State private var creatingProject = false
        @State private var showingSettings = false
        @State private var selecting = false
        @State private var drawerPath: [LibraryDrawerDestination] = []
        @AccessibilityFocusState private var closeFocused: Bool
        private let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        private let onSelectChat: @MainActor (String) -> Void
        private let onNewChat: @MainActor (String?) -> Void
        private let settings: @MainActor () -> AnyView
        private let settingsFullScreen: Bool
        private let settingsPresented: Binding<Bool>?
        private let onSettingsDismiss: (@MainActor () -> Void)?
        private let supplementary: (@MainActor () -> AnyView)?
        private let onClose: (@MainActor () -> Void)?
        private let drawerNavigation: Bool
        private let navigationAvailable: Bool
        private let navigationFocusAvailable: Bool?
        private var focusAvailable: Bool { navigationFocusAvailable ?? navigationAvailable }
        private let onDrawerDestinationChanged: (@MainActor (Bool) -> Void)?

        public init(
            library: LibraryModel, makeLibrary: @escaping @MainActor () -> LibraryModel,
            makeInstructionEditor: @escaping @MainActor (InstructionDestination) -> InstructionEditor,
            onSelectChat: @escaping @MainActor (String) -> Void,
            onNewChat: @escaping @MainActor (String?) -> Void,
            settings: @escaping @MainActor () -> AnyView,
            onClose: (@MainActor () -> Void)? = nil,
            settingsFullScreen: Bool = false,
            settingsPresented: Binding<Bool>? = nil,
            onSettingsDismiss: (@MainActor () -> Void)? = nil,
            drawerNavigation: Bool = false,
            navigationAvailable: Bool = true,
            navigationFocusAvailable: Bool? = nil,
            onDrawerDestinationChanged: (@MainActor (Bool) -> Void)? = nil,
            supplementary: (@MainActor () -> AnyView)? = nil
        ) {
            self.library = library; self.makeInstructionEditor = makeInstructionEditor
            self.onSelectChat = onSelectChat; self.onNewChat = onNewChat; self.settings = settings;
            self.onClose = onClose; self.settingsFullScreen = settingsFullScreen
            self.settingsPresented = settingsPresented; self.onSettingsDismiss = onSettingsDismiss
            self.supplementary = supplementary
            self.drawerNavigation = drawerNavigation; self.navigationAvailable = navigationAvailable
            self.navigationFocusAvailable = navigationFocusAvailable
            self.onDrawerDestinationChanged = onDrawerDestinationChanged
            _models = State(initialValue: LibraryMenuModels(makeLibrary: makeLibrary))
        }
        public var body: some View {
            NavigationStack(path: $drawerPath) {
                Group {
                    List {
                        Section {
                            Button {
                                selectNewChat(nil)
                            } label: {
                                Label {
                                    libraryText("New chat")
                                } icon: {
                                    Image(systemName: "square.and.pencil")
                                }
                            }
                            .accessibilityIdentifier("library.new-chat")
                        }
                        if let supplementary { supplementary() }
                        Section {
                            ForEach(visibleProjects, id: \.id) { project in
                                if let projectModel = models.projects[project.id] {
                                    ProjectFolderView(
                                        project: project, model: projectModel, search: search,
                                        makeInstructionEditor: makeInstructionEditor, onSelectChat: selectChat,
                                        onNewChat: selectNewChat,
                                        onMutation: refresh, drawerNavigation: drawerNavigation,
                                        selectionLibrary: library, selecting: selecting)
                                }
                            }
                            Button {
                                projectName = ""; creatingProject = true
                            } label: {
                                Label {
                                    libraryText("New project")
                                } icon: {
                                    Image(systemName: "folder.badge.plus")
                                }
                            }.accessibilityIdentifier("library.new-project")
                            if library.isLoadingProjects { ProgressView() }
                        } header: {
                            libraryText("Projects")
                        }
                        Section {
                            conversationRows(models.pinned, loadMoreID: "library.pinned.load-more")
                        } header: {
                            libraryText("Pinned")
                        }
                        Section {
                            conversationRows(models.recent, loadMoreID: "library.recent.load-more")
                        } header: {
                            libraryText("Recent")
                        }
                        if let issue = library.issue { LibraryIssueView(issue: issue) }
                    }
                    .accessibilityIdentifier("library.sidebar-list")
                    .allowsHitTesting(navigationAvailable)
                    .accessibilityHidden(!navigationAvailable)
                    .safeAreaInset(edge: .top, spacing: 0) {
                        searchControls
                            .padding(.horizontal, 16).padding(.vertical, 8)
                            .background(MapleSurface.sidebar)
                            .allowsHitTesting(navigationAvailable)
                            .accessibilityHidden(!navigationAvailable)
                    }
                    .listStyle(.sidebar)
                    .scrollContentBackground(.hidden)
                    .background(MapleSurface.sidebar)
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .principal) {
                            MapleWordmark().frame(width: 103, height: 20).accessibilityHidden(true)
                        }
                        if let onClose, navigationAvailable {
                            ToolbarItem(placement: .topBarTrailing) {
                                Button(action: onClose) {
                                    Label {
                                        Text("Close menu", tableName: "Library", bundle: .module)
                                    } icon: {
                                        Image(systemName: "xmark")
                                    }
                                    .labelStyle(.iconOnly)
                                    .frame(minWidth: 44, minHeight: 44)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel(Text("Close menu", tableName: "Library", bundle: .module))
                                .accessibilityIdentifier("library.close")
                                .accessibilityFocused($closeFocused)
                            }
                        }
                    }
                    .alert(Text("New project", tableName: "Library", bundle: .module), isPresented: $creatingProject) {
                        TextField(text: $projectName) { libraryText("Project name") }
                        Button(role: .cancel) {
                        } label: {
                            libraryText("Cancel")
                        }
                        Button {
                            Task {
                                if let project = await library.createProject(name: projectName) {
                                    _ = models.project(project.id)
                                }
                            }
                        } label: {
                            libraryText("Create")
                        }
                        .disabled(projectName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .onChange(of: search) { _, value in
                        models.pinned.searchText = value; models.recent.searchText = value
                    }
                    .task(id: navigationAvailable) {
                        guard navigationAvailable else { return }
                        if drawerNavigation && focusAvailable && drawerPath.isEmpty { closeFocused = true }
                        await refresh()
                    }
                    .accessibilityAction(.escape) {
                        // Only the root drawer owns this action. Pushed editors
                        // retain their native Back/Cancel and discard safeguards.
                        if drawerNavigation && navigationAvailable && drawerPath.isEmpty { onClose?() }
                    }
                }
                .toolbarVisibility(navigationAvailable ? .visible : .hidden, for: .navigationBar)
                .navigationDestination(for: LibraryDrawerDestination.self) { destination in
                    switch destination {
                    case .settings:
                        settings()
                    case .project(let id):
                        if let project = library.projects.first(where: { $0.id == id }),
                            let model = models.projects[id]
                        {
                            ProjectView(
                                project: project, library: model, makeInstructionEditor: makeInstructionEditor,
                                onSelectChat: selectChat, onNewChat: selectNewChat)
                        }
                    }
                }
                .modifier(LibraryChatManagement(library: library, selecting: $selecting, onMutation: refresh))
            }
            .onChange(of: drawerPath) { _, path in
                guard drawerNavigation else { return }
                // Pushing clears root focus; native Back restores its own row.
                if !path.isEmpty { closeFocused = false }
                onDrawerDestinationChanged?(!path.isEmpty)
            }
            .onChange(of: focusAvailable) { _, available in
                closeFocused = drawerNavigation && available && drawerPath.isEmpty
            }
            .fullScreenCover(isPresented: settingsBinding, onDismiss: onSettingsDismiss) {
                NavigationStack {
                    settings().toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button {
                                settingsBinding.wrappedValue = false
                            } label: {
                                Label {
                                    Text("Close settings", tableName: "Library", bundle: .module)
                                } icon: {
                                    Image(systemName: "xmark")
                                }
                                .labelStyle(.iconOnly)
                                .frame(minWidth: 44, minHeight: 44)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(Text("Close settings", tableName: "Library", bundle: .module))
                            .accessibilityIdentifier("library.settings-close")
                        }
                    }
                }
            }
        }
        private func selectChat(_ id: String) {
            if drawerNavigation { drawerPath = [] }
            onSelectChat(id)
        }
        private func selectNewChat(_ projectID: String?) {
            if drawerNavigation { drawerPath = [] }
            selecting = false; library.selectedIDs = []
            onNewChat(projectID)
        }
        private var settingsBinding: Binding<Bool> { settingsPresented ?? $showingSettings }
        private var searchControls: some View {
            HStack(spacing: 12) {
                HStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary).accessibilityHidden(true)
                    TextField(text: $search) { libraryText("Search loaded titles") }
                        .textInputAutocapitalization(.never).autocorrectionDisabled().submitLabel(.search)
                        .frame(minHeight: 44)
                        .accessibilityIdentifier("library.search")
                    if !search.isEmpty {
                        Button {
                            search = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                        }
                        .frame(minWidth: 44, minHeight: 44)
                        .accessibilityLabel(libraryText("Clear search"))
                        .accessibilityIdentifier("library.clear-search")
                    }
                }
                .padding(.horizontal, 12).frame(minHeight: 44)
                .background(MapleSurface.card, in: RoundedRectangle(cornerRadius: 12))
                settingsControl
            }
        }
        private var settingsControl: some View {
            Group {
                if settingsFullScreen {
                    Button {
                        settingsBinding.wrappedValue = true
                    } label: {
                        settingsLabel
                    }
                } else if drawerNavigation {
                    NavigationLink(value: LibraryDrawerDestination.settings) { settingsLabel }
                } else {
                    NavigationLink {
                        settings()
                    } label: {
                        settingsLabel
                    }
                }
            }
            .buttonStyle(.plain).labelStyle(.iconOnly)
            .frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
            .accessibilityIdentifier("library.settings")
            .accessibilityLabel(libraryText("Settings"))
            .accessibilityShowsLargeContentViewer { settingsLabel }
        }
        private var settingsLabel: some View {
            Label {
                libraryText("Settings")
            } icon: {
                Image(systemName: "gearshape")
            }
        }
        private var visibleProjects: [ConversationProjectListItem] {
            let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
            return library.projects.filter { project in
                query.isEmpty || project.name.localizedCaseInsensitiveContains(query)
                    || models.projects[project.id]?.conversations.contains {
                        LibraryModel.title($0).localizedCaseInsensitiveContains(query)
                    } == true
            }
        }
        @ViewBuilder private func conversationRows(_ model: LibraryModel, loadMoreID: String) -> some View {
            ForEach(model.visibleConversations, id: \.id) { conversation in
                LibrarySelectableConversationRow(
                    conversation: conversation, library: model, selectionLibrary: library, selecting: selecting,
                    onSelect: selectChat, onMutation: refresh)
            }
            if model.isLoading { ProgressView() }
            if model.hasMore {
                Button {
                    Task { await model.loadMore() }
                } label: {
                    libraryText("Load more")
                }.disabled(model.isLoading).accessibilityIdentifier(loadMoreID)
            }
            if let issue = model.issue { LibraryIssueView(issue: issue) }
        }
        private func refresh() async {
            await library.loadProjects()
            let activeProjects = Set(library.projects.map(\.id))
            for id in Array(models.projects.keys) where !activeProjects.contains(id) {
                models.projects.removeValue(forKey: id)?.invalidate()
            }
            for project in library.projects { _ = models.project(project.id) }
            await models.refresh()
        }
    }

    private struct ProjectFolderView: View {
        let project: ConversationProjectListItem
        @Bindable var model: LibraryModel
        let search: String
        let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        let onSelectChat: @MainActor (String) -> Void
        let onNewChat: @MainActor (String?) -> Void
        let onMutation: @MainActor () async -> Void
        let drawerNavigation: Bool
        @Bindable var selectionLibrary: LibraryModel
        let selecting: Bool
        @State private var expanded = false
        var body: some View {
            DisclosureGroup(isExpanded: $expanded) {
                if drawerNavigation {
                    NavigationLink(value: LibraryDrawerDestination.project(project.id)) {
                        manageProjectLabel
                    }.accessibilityIdentifier("library.project.manage." + project.id)
                } else {
                    NavigationLink {
                        ProjectView(
                            project: project, library: model, makeInstructionEditor: makeInstructionEditor,
                            onSelectChat: onSelectChat, onNewChat: onNewChat)
                    } label: {
                        manageProjectLabel
                    }.accessibilityIdentifier("library.project.manage." + project.id)
                }
                Button {
                    onNewChat(project.id)
                } label: {
                    Label {
                        libraryText("New chat in project")
                    } icon: {
                        Image(systemName: "square.and.pencil")
                    }
                }
                ForEach(model.visibleConversations, id: \.id) { conversation in
                    LibrarySelectableConversationRow(
                        conversation: conversation, library: model, selectionLibrary: selectionLibrary,
                        selecting: selecting, onSelect: onSelectChat, onMutation: onMutation)
                }
                if model.isLoading { ProgressView() }
                if model.hasMore {
                    Button {
                        Task { await model.loadMore() }
                    } label: {
                        libraryText("Load more")
                    }.disabled(model.isLoading).accessibilityIdentifier("library.project.load-more." + project.id)
                }
                if let issue = model.issue { LibraryIssueView(issue: issue) }
            } label: {
                Label {
                    Text(project.name)
                } icon: {
                    Image(systemName: "folder")
                }
                .accessibilityIdentifier("library.project." + project.id)
            }
            .onChange(of: expanded) { _, value in
                if value {
                    Task {
                        await model.load(.project(project.id)); model.searchText = search
                    }
                }
            }
            .onChange(of: search) { _, value in model.searchText = value }
        }
        private var manageProjectLabel: some View {
            Label {
                libraryText("Manage project")
            } icon: {
                Image(systemName: "folder")
            }
        }
    }

    /// Fixed app-owned keys are maintained in Library.xcstrings. Keep literals
    /// as String until the explicit table lookup so extraction cannot misroute them.
    func libraryText(_ key: String) -> Text {
        Text(LocalizedStringKey(key), tableName: "Library", bundle: .module)
    }
#endif
