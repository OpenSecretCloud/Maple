#if os(iOS)
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI

    private enum LibraryDrawerDestination: Hashable {
        case settings
    }

    /// Compact navigation uses this full-scene root beneath the retained chat
    /// page. Regular widths retain the NavigationSplitView sidebar.
    public struct LibraryView: View {
        @Bindable private var library: LibraryModel
        @State private var models: LibraryMenuModel
        @State private var drag: LibraryChatDragContext
        @State private var search = ""
        @State private var projectDestination: ProjectSheetDestination?
        @State private var showingSettings = false
        @State private var selecting = false
        @State private var drawerPath: [LibraryDrawerDestination] = []
        @FocusState private var searchFocused: Bool
        @AccessibilityFocusState private var closeFocused: Bool
        private let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        private let onSelectChat: @MainActor (String) -> Void
        private let onNewChat: @MainActor (String?) -> Void
        private let selectedChatID: String?
        private let settings: @MainActor () -> AnyView
        private let settingsFullScreen: Bool
        private let settingsPresented: Binding<Bool>?
        private let onSettingsDismiss: (@MainActor () -> Void)?
        private let supplementary: (@MainActor () -> AnyView)?
        private let onClose: (@MainActor () -> Void)?
        private let drawerNavigation: Bool
        private let compactRootBar: Bool
        private let navigationAvailable: Bool
        private let navigationFocusAvailable: Bool?
        private var focusAvailable: Bool { navigationFocusAvailable ?? navigationAvailable }
        private let onDrawerDestinationChanged: (@MainActor (Bool) -> Void)?
        private let projectExpansion: LibraryProjectExpansionController?
        private let onManageProjects: (@MainActor (String?) -> Void)?
        private let conversationActivity: @MainActor (String) -> ConversationActivity
        private let projectActivity: @MainActor (String) -> ConversationActivity

        public init(
            library: LibraryModel, makeLibrary: @escaping @MainActor () -> LibraryModel,
            makeInstructionEditor: @escaping @MainActor (InstructionDestination) -> InstructionEditor,
            onSelectChat: @escaping @MainActor (String) -> Void,
            onNewChat: @escaping @MainActor (String?) -> Void,
            selectedChatID: String? = nil,
            settings: @escaping @MainActor () -> AnyView,
            conversationActivity: @escaping @MainActor (String) -> ConversationActivity = { _ in .idle },
            projectActivity: @escaping @MainActor (String) -> ConversationActivity = { _ in .idle },
            projectExpansion: LibraryProjectExpansionController? = nil,
            menu: LibraryMenuModel? = nil,
            onManageProjects: (@MainActor (String?) -> Void)? = nil,
            onClose: (@MainActor () -> Void)? = nil,
            settingsFullScreen: Bool = false,
            settingsPresented: Binding<Bool>? = nil,
            onSettingsDismiss: (@MainActor () -> Void)? = nil,
            drawerNavigation: Bool = false,
            compactRootBar: Bool = false,
            navigationAvailable: Bool = true,
            navigationFocusAvailable: Bool? = nil,
            onDrawerDestinationChanged: (@MainActor (Bool) -> Void)? = nil,
            supplementary: (@MainActor () -> AnyView)? = nil
        ) {
            self.library = library; self.makeInstructionEditor = makeInstructionEditor
            self.onSelectChat = onSelectChat; self.onNewChat = onNewChat; self.settings = settings;
            self.selectedChatID = selectedChatID
            self.conversationActivity = conversationActivity; self.projectActivity = projectActivity
            self.onClose = onClose; self.settingsFullScreen = settingsFullScreen
            self.settingsPresented = settingsPresented; self.onSettingsDismiss = onSettingsDismiss
            self.supplementary = supplementary
            self.drawerNavigation = drawerNavigation; self.navigationAvailable = navigationAvailable
            self.compactRootBar = compactRootBar
            self.navigationFocusAvailable = navigationFocusAvailable
            self.onDrawerDestinationChanged = onDrawerDestinationChanged
            self.projectExpansion = projectExpansion?.scope == library.scope ? projectExpansion : nil
            self.onManageProjects = onManageProjects
            if let menu, menu.scope == library.scope {
                _models = State(initialValue: menu)
            } else {
                _models = State(initialValue: LibraryMenuModel(root: library, makeLibrary: makeLibrary))
            }
            _drag = State(initialValue: LibraryChatDragContext(scope: library.scope))
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
                                    Image(systemName: "square.and.pencil").foregroundStyle(MapleAccent.action)
                                }
                                .frame(minHeight: 44).contentShape(Rectangle())
                            }
                            .buttonStyle(.plain).foregroundStyle(.primary)
                            .accessibilityIdentifier("library.new-chat")
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                        }
                        .listSectionMargins(.vertical, 0)
                        if let supplementary { supplementary() }
                        if showsNoSearchResults {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(
                                    String(
                                        localized: "No chats found matching \"%@\"",
                                        defaultValue: "No chats found matching \"\(trimmedSearch)\"",
                                        table: "Library", bundle: .module))
                                libraryText("Try a different search term").font(.footnote)
                            }
                            .foregroundStyle(.secondary)
                            .padding(.vertical, 8)
                            .accessibilityIdentifier("library.search.empty")
                            .listRowSeparator(.hidden).listRowBackground(Color.clear)
                        } else {
                            Section {
                                ForEach(visibleProjects, id: \.id) { project in
                                    if let projectModel = models.projects[project.id] {
                                        ProjectFolderView(
                                            project: project, model: projectModel, search: search,
                                            onSelectChat: selectChat, onNewChat: selectNewChat,
                                            selectedChatID: selectedChatID,
                                            conversationActivity: conversationActivity,
                                            projectActivity: projectActivity(project.id),
                                            onManageProject: manageProject,
                                            onMutation: refresh, selectionLibrary: library, selecting: $selecting,
                                            drag: drag, dragEnabled: navigationAvailable,
                                            expanded: Binding(
                                                get: {
                                                    projectExpansion?.isExpanded(id: project.id)
                                                        ?? models.expandedProjects.contains(project.id)
                                                },
                                                set: { value in
                                                    if let projectExpansion {
                                                        projectExpansion.setExpanded(id: project.id, expanded: value)
                                                    } else if value {
                                                        models.expandedProjects.insert(project.id)
                                                    } else {
                                                        models.expandedProjects.remove(project.id)
                                                    }
                                                }))
                                    }
                                }
                            } header: {
                                HStack {
                                    libraryText("Projects").font(.headline).foregroundStyle(.primary)
                                    Spacer()
                                    Button {
                                        suspendSearch()
                                        if let onManageProjects {
                                            onManageProjects(nil)
                                        } else {
                                            projectDestination = .create
                                        }
                                    } label: {
                                        Image(systemName: "folder.badge.plus").foregroundStyle(MapleAccent.action)
                                            .frame(minWidth: 44, minHeight: 44)
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(library.projects.count >= 10)
                                    .accessibilityLabel(libraryText("New project"))
                                    .accessibilityHint(
                                        libraryText("Maximum of 10 projects reached"),
                                        isEnabled: library.projects.count >= 10
                                    )
                                    .accessibilityIdentifier("library.new-project")
                                }
                                .listRowInsets(.top, 8).listRowInsets(.bottom, 0)
                            }
                            .textCase(nil)
                            .listSectionMargins(.vertical, 0)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            if drag.isDragging || !models.pinned.visibleConversations.isEmpty || models.pinned.hasMore
                                || models.pinned.issue != nil
                            {
                                Section {
                                    conversationRows(
                                        models.pinned, loadMoreID: "library.pinned.load-more", destination: .pinned)
                                } header: {
                                    dropHeading("Pinned", destination: .pinned, id: "library.drop.pinned")
                                }
                                .textCase(nil)
                                .listSectionMargins(.vertical, 0)
                                .listRowSeparator(.hidden)
                                .listRowBackground(Color.clear)
                            }
                            Section {
                                conversationRows(
                                    models.recent, loadMoreID: "library.recent.load-more", destination: .recent)
                            } header: {
                                dropHeading("Recent", destination: .recent, id: "library.drop.recent")
                            }
                            .textCase(nil)
                            .listSectionMargins(.vertical, 0)
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                        }
                        if let issue = library.issue { LibraryIssueView(issue: issue) }
                    }
                    .accessibilityIdentifier("library.sidebar-list")
                    .refreshable { await refresh() }
                    .allowsHitTesting(navigationAvailable)
                    .accessibilityHidden(!navigationAvailable)
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        searchControls
                            .padding(.horizontal, 16).padding(.vertical, 8)
                            .allowsHitTesting(navigationAvailable)
                            .accessibilityHidden(!navigationAvailable)
                    }
                    .listStyle(.plain)
                    .environment(\.defaultMinListRowHeight, 50)
                    .environment(\.defaultMinListHeaderHeight, 0)
                    // Row labels own their tap height; remove additional native
                    // insets instead of compressing those hit targets.
                    .listRowSpacing(0)
                    .listRowInsets(.vertical, 0)
                    .listRowInsets(.leading, 20).listRowInsets(.trailing, 12)
                    .scrollContentBackground(.hidden)
                    .background(MapleSurface.sidebar)
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar(removing: .sidebarToggle)
                    .toolbar {
                        if !usesCompactRootBar, onClose != nil, navigationAvailable {
                            ToolbarItem(placement: .topBarTrailing) {
                                closeControl
                            }
                        }
                    }
                    .onChange(of: search) { _, value in
                        models.setSearch(value)
                    }
                    .onChange(of: selecting) { _, _ in drag.cancel() }
                    .onChange(of: navigationAvailable) { _, value in
                        if !value { drag.cancel(); suspendSearch() }
                    }
                    .onDisappear {
                        drag.cancel(); suspendSearch()
                    }
                    .task(id: navigationAvailable) {
                        guard navigationAvailable else { return }
                        if drawerNavigation && focusAvailable && drawerPath.isEmpty { closeFocused = true }
                        await refresh()
                    }
                    .accessibilityAction(.escape) {
                        // Only the root drawer owns this action. Pushed editors
                        // retain their native Back/Cancel and discard safeguards.
                        if drawerNavigation && navigationAvailable && drawerPath.isEmpty {
                            suspendSearch(); onClose?()
                        }
                    }
                }
                .toolbarVisibility(
                    navigationAvailable && (!usesCompactRootBar || !drawerPath.isEmpty) ? .visible : .hidden,
                    for: .navigationBar
                )
                .navigationDestination(for: LibraryDrawerDestination.self) { destination in
                    switch destination {
                    case .settings:
                        if usesCompactRootBar {
                            settings().toolbarVisibility(.visible, for: .navigationBar)
                        } else {
                            settings()
                        }
                    }
                }
                .modifier(
                    LibraryChatManagement(
                        library: library, selecting: $selecting, onMutation: refresh,
                        onSettings: {
                            suspendSearch()
                            if settingsFullScreen {
                                settingsBinding.wrappedValue = true
                            } else {
                                drawerPath.append(.settings)
                            }
                        },
                        compactRootBar: usesCompactRootBar,
                        nativeRootBarHidden: !navigationAvailable || !drawerPath.isEmpty,
                        nativeRootBarTrailing: onClose != nil && navigationAvailable ? AnyView(closeControl) : nil))
            }
            .onChange(of: drawerPath) { _, path in
                guard drawerNavigation else { return }
                // Pushing clears root focus; native Back restores its own row.
                if !path.isEmpty { closeFocused = false; suspendSearch() }
                onDrawerDestinationChanged?(!path.isEmpty)
            }
            .onChange(of: focusAvailable) { _, available in
                closeFocused = drawerNavigation && available && drawerPath.isEmpty
                if !available { suspendSearch() } else if !search.isEmpty { models.setSearch(search) }
            }
            .sheet(isPresented: projectSheetBinding, onDismiss: { Task { await refresh() } }) {
                if let projectDestination {
                    ProjectManagementSheet(
                        library: library, initialDestination: projectDestination,
                        makeInstructionEditor: makeInstructionEditor,
                        isCurrent: { library.isActive && library.scope == models.scope })
                }
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
        private var usesCompactRootBar: Bool { compactRootBar && drawerNavigation }

        @ViewBuilder private var closeControl: some View {
            if let onClose, navigationAvailable {
                Button {
                    suspendSearch(); onClose()
                } label: {
                    Label {
                        Text("Close menu", tableName: "Library", bundle: .module)
                    } icon: {
                        Image(systemName: "xmark")
                    }
                    .labelStyle(.iconOnly)
                    .foregroundStyle(MapleAccent.action)
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text("Close menu", tableName: "Library", bundle: .module))
                .accessibilityIdentifier("library.close")
                .accessibilityFocused($closeFocused)
            }
        }

        private func selectChat(_ id: String) {
            suspendSearch()
            if drawerNavigation { drawerPath = [] }
            onSelectChat(id)
        }
        private func selectNewChat(_ projectID: String?) {
            suspendSearch()
            if drawerNavigation { drawerPath = [] }
            selecting = false; library.selectedIDs = []
            onNewChat(projectID)
        }
        private var settingsBinding: Binding<Bool> { settingsPresented ?? $showingSettings }
        private var projectSheetBinding: Binding<Bool> {
            Binding(get: { projectDestination != nil }, set: { if !$0 { projectDestination = nil } })
        }
        private func manageProject(_ id: String) {
            suspendSearch()
            if let onManageProjects { onManageProjects(id) } else { projectDestination = .manage(id) }
        }
        private func dismissSearch() { searchFocused = false }
        private func suspendSearch() { dismissSearch(); models.cancelSearchLoading() }
        private var searchControls: some View {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary).accessibilityHidden(true)
                TextField(text: $search) { libraryText("Search chats") }
                    .textInputAutocapitalization(.never).autocorrectionDisabled().submitLabel(.search)
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                    .focused($searchFocused)
                    .onSubmit { dismissSearch() }
                    .accessibilityIdentifier("library.search")
                if !search.isEmpty {
                    Button {
                        search = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                            .frame(minWidth: 44, minHeight: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(libraryText("Clear search"))
                    .accessibilityIdentifier("library.clear-search")
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 4)
            // Glass alone leaves transparent gaps in the hit-test surface.
            // The complete capsule owns taps, including its padded edges.
            .background(MapleSurface.canvas, in: Capsule())
            .contentShape(Capsule())
            .onTapGesture { searchFocused = true }
            .glassEffect(.regular.interactive(), in: .capsule)
            .overlay { Capsule().strokeBorder(MapleSurface.border.opacity(0.7), lineWidth: 0.75) }
            .shadow(color: .black.opacity(0.16), radius: 14, x: 0, y: 6)
        }
        private var visibleProjects: [ConversationProjectListItem] {
            let query = trimmedSearch
            return library.projects.filter { project in
                query.isEmpty || project.name.localizedCaseInsensitiveContains(query)
                    || models.projects[project.id]?.conversations.contains {
                        LibraryModel.title($0).localizedCaseInsensitiveContains(query)
                    } == true
            }
        }
        private var trimmedSearch: String { search.trimmingCharacters(in: .whitespacesAndNewlines) }
        private var showsNoSearchResults: Bool {
            !trimmedSearch.isEmpty && models.searchText == search && models.isPrepared
                && !library.isLoadingProjects && library.issue == nil && visibleProjects.isEmpty
                && models.pinned.hasLoadedConversations && !models.pinned.isLoading && models.pinned.issue == nil
                && models.pinned.visibleConversations.isEmpty
                && models.recent.hasLoadedConversations && !models.recent.isLoading && models.recent.issue == nil
                && models.recent.visibleConversations.isEmpty
                && library.projects.allSatisfy { project in
                    guard let model = models.projects[project.id] else { return false }
                    return model.hasLoadedConversations && !model.isLoading && model.issue == nil
                }
        }
        private func dropHeading(_ title: String, destination: LibraryConversationDestination, id: String) -> some View
        {
            libraryText(title).font(.headline).foregroundStyle(.primary)
                .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
                .listRowInsets(.top, 8).listRowInsets(.bottom, 0)
                .accessibilityIdentifier(id)
                .modifier(
                    LibraryChatDropTarget(
                        context: drag, library: library, destination: destination,
                        enabled: navigationAvailable && !selecting, onMutation: refresh))
        }
        @ViewBuilder private func conversationRows(
            _ model: LibraryModel, loadMoreID: String, destination: LibraryConversationDestination
        ) -> some View {
            ForEach(model.visibleConversations, id: \.id) { conversation in
                LibrarySelectableConversationRow(
                    conversation: conversation, library: model, selectionLibrary: library, selecting: $selecting,
                    onSelect: selectChat, onMutation: refresh, dragContext: drag,
                    dragEnabled: navigationAvailable, dropDestination: destination, selectedChatID: selectedChatID,
                    activity: conversationActivity(conversation.id)
                )
                .onScrollVisibilityChange(threshold: 0.1) { visible in
                    guard visible, destination == .recent, navigationAvailable, focusAvailable else { return }
                    Task { await model.loadMoreIfNeeded(visibleID: conversation.id) }
                }
                // Apply row traits to the visibility wrapper, matching the
                // project rows rather than retaining List's default padding.
                .listRowInsets(.vertical, 0)
                .listRowInsets(.leading, 20).listRowInsets(.trailing, 12)
            }
            if model.hasMore && (destination != .recent || model.issue != nil || !search.isEmpty) {
                Button {
                    Task { await model.loadMore() }
                } label: {
                    libraryText("Load more")
                }.disabled(model.isLoading).accessibilityIdentifier(loadMoreID)
            }
            if let issue = model.issue { LibraryIssueView(issue: issue) }
            if destination == .recent, model.hasLoadedConversations, !model.isLoading,
                model.visibleConversations.isEmpty, model.issue == nil
            {
                libraryText("No recent chats.").font(.footnote).foregroundStyle(.secondary)
                    .accessibilityIdentifier("library.recent.empty")
            }
        }
        private func refresh() async {
            models.setSearch(search)
            await models.refresh { id in
                projectExpansion?.isExpanded(id: id) ?? models.expandedProjects.contains(id)
            }
        }
    }

    private struct ProjectFolderView: View {
        let project: ConversationProjectListItem
        @Bindable var model: LibraryModel
        let search: String
        let onSelectChat: @MainActor (String) -> Void
        let onNewChat: @MainActor (String?) -> Void
        let selectedChatID: String?
        let conversationActivity: @MainActor (String) -> ConversationActivity
        let projectActivity: ConversationActivity
        let onManageProject: @MainActor (String) -> Void
        let onMutation: @MainActor () async -> Void
        @Bindable var selectionLibrary: LibraryModel
        @Binding var selecting: Bool
        let drag: LibraryChatDragContext
        let dragEnabled: Bool
        @Binding var expanded: Bool
        private var showingChildren: Bool {
            expanded || !search.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
        var body: some View {
            HStack(spacing: 8) {
                Button {
                    expanded.toggle()
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: "folder")
                        Text(project.name).fontWeight(.semibold).lineLimit(1)
                        ConversationActivityIndicator(activity: folderActivity)
                        Spacer(minLength: 0)
                    }
                    .frame(minHeight: 48).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("library.project." + project.id)
                .accessibilityValue(libraryText(showingChildren ? "Expanded" : "Collapsed"))
                .accessibilityAction(named: libraryText("Manage project")) { onManageProject(project.id) }
                .contextMenu {
                    Button {
                        onManageProject(project.id)
                    } label: {
                        libraryText("Manage project")
                    }
                    .accessibilityIdentifier("library.project.manage." + project.id)
                }
                Button {
                    onNewChat(project.id)
                } label: {
                    Image(systemName: "square.and.pencil").foregroundStyle(MapleAccent.action)
                        .frame(minWidth: 44, minHeight: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(libraryText("New chat in project"))
                .accessibilityIdentifier("library.project.new-chat." + project.id)
            }
            .listRowInsets(.vertical, 0)
            .listRowInsets(.leading, 20).listRowInsets(.trailing, 12)
            .listRowSeparator(.hidden).listRowBackground(Color.clear)
            #if DEBUG
                .background(alignment: .topLeading) {
                    if ProcessInfo.processInfo.arguments.contains("--maple-library-local-cleanup"),
                        model.scope == selectionLibrary.scope, UUID(uuidString: project.id) != nil
                    {
                        // A read-only test probe admits exact, non-cascade cleanup
                        // from the raw project owner, including pinned children.
                        Text(verbatim: cleanupState).foregroundStyle(.clear).frame(width: 1, height: 1)
                        .accessibilityLabel(cleanupState)
                        .accessibilityIdentifier("library.cleanup-project." + project.id)
                    }
                }
            #endif
            .modifier(
                LibraryChatDropTarget(
                    context: drag, library: selectionLibrary, destination: .project(project.id),
                    enabled: dragEnabled && !selecting, onMutation: onMutation)
            )
            .task(id: showingChildren) {
                // A List may materialize the row after its saved disclosure
                // is already open. Load that first page from visibility rather
                // than depending on a change observed by this row instance.
                guard showingChildren, !model.hasLoadedConversations else { return }
                model.searchText = search
                await model.load(.project(project.id))
            }
            .onChange(of: search) { _, value in model.searchText = value }
            if showingChildren {
                ForEach(model.visibleConversations, id: \.id) { conversation in
                    LibrarySelectableConversationRow(
                        conversation: conversation, library: model, selectionLibrary: selectionLibrary,
                        selecting: $selecting, onSelect: onSelectChat, onMutation: onMutation,
                        leadingInset: 56, dragContext: drag, dragEnabled: dragEnabled,
                        dropDestination: .project(project.id), selectedChatID: selectedChatID
                    )
                    .listRowInsets(.vertical, 0)
                    .listRowInsets(.leading, 56).listRowInsets(.trailing, 12)
                }
                if model.hasMore {
                    Button {
                        Task { await model.loadMore() }
                    } label: {
                        libraryText("Load more")
                    }
                    .disabled(model.isLoading)
                    .accessibilityIdentifier("library.project.load-more." + project.id)
                    .listRowInsets(.vertical, 0)
                    .listRowInsets(.leading, 56).listRowInsets(.trailing, 12)
                    .listRowSeparator(.hidden).listRowBackground(Color.clear)
                }
                if let issue = model.issue { LibraryIssueView(issue: issue) }
            }
        }
        private var folderActivity: ConversationActivity {
            guard !showingChildren else { return .idle }
            let children = model.conversations.map { conversationActivity($0.id) }
            if projectActivity == .running || children.contains(.running) { return .running }
            if projectActivity == .unread || children.contains(.unread) { return .unread }
            return .idle
        }
        #if DEBUG
            private var cleanupState: String {
                guard model.isActive, model.filter == .project(project.id) else { return "loading" }
                if model.issue != nil { return "error" }
                guard model.hasLoadedConversations, !model.isLoading else { return "loading" }
                return model.conversations.isEmpty && !model.hasMore ? "empty" : "nonempty"
            }
        #endif
    }

    /// Fixed app-owned keys are maintained in Library.xcstrings. Keep literals
    /// as String until the explicit table lookup so extraction cannot misroute them.
    func libraryText(_ key: String) -> Text {
        Text(LocalizedStringKey(key), tableName: "Library", bundle: .module)
    }
#endif
