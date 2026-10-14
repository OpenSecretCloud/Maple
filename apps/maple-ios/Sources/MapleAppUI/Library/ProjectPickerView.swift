#if os(iOS)
    import MapleAppCore
    import SwiftUI

    /// The same project screens are used from Chat options and the main menu.
    /// Selection is admitted by the presenting account and conversation owner.
    enum ProjectSheetDestination: Hashable {
        case projects, create, manage(String), rename(String), instructions(String)

        var permitsOutsideDismissal: Bool {
            switch self {
            case .projects, .manage: true
            case .create, .rename, .instructions: false
            }
        }
    }

    struct ProjectManagementSheet: View {
        @Bindable var library: LibraryModel
        var selectedProjectID: String? = nil
        var initialDestination: ProjectSheetDestination = .projects
        let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        let isCurrent: @MainActor () -> Bool
        var onSelection: (@MainActor (String?) async -> Bool)? = nil
        var onCreatedProject: (@MainActor (String) -> Void)? = nil
        @Environment(\.dismiss) private var dismiss
        @State private var path: [ProjectSheetDestination] = []

        var body: some View {
            NavigationStack(path: $path) {
                ProjectSheetDestinationView(
                    destination: initialDestination, library: library, selectedProjectID: selectedProjectID,
                    makeInstructionEditor: makeInstructionEditor, isCurrent: isCurrent,
                    onSelection: selection, onCreatedProject: onCreatedProject, isRoot: true
                )
                .navigationDestination(for: ProjectSheetDestination.self) { destination in
                    ProjectSheetDestinationView(
                        destination: destination, library: library, selectedProjectID: selectedProjectID,
                        makeInstructionEditor: makeInstructionEditor, isCurrent: isCurrent,
                        onSelection: selection, onCreatedProject: onCreatedProject)
                }
            }
            .presentationDetents([.fraction(0.6), .fraction(0.98)])
            .presentationDragIndicator(.visible)
            .presentationSizing(.page)
            .presentationBackground(MapleSurface.canvas)
            .background(
                SheetOutsideTapDismissal {
                    if (path.last ?? initialDestination).permitsOutsideDismissal, !library.isMutating { dismiss() }
                }.frame(width: 0, height: 0))
        }

        private var selection: (@MainActor (String?) async -> Bool)? {
            guard let onSelection else { return nil }
            return { id in
                guard isCurrent(), await onSelection(id), isCurrent() else { return false }
                dismiss(); return true
            }
        }
    }

    struct ProjectSheetDestinationView: View {
        let destination: ProjectSheetDestination
        @Bindable var library: LibraryModel
        let selectedProjectID: String?
        let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        let isCurrent: @MainActor () -> Bool
        let onSelection: (@MainActor (String?) async -> Bool)?
        var onCreatedProject: (@MainActor (String) -> Void)? = nil
        var isRoot = false

        @ViewBuilder var body: some View {
            switch destination {
            case .projects:
                ProjectPickerView(
                    library: library, selectedProjectID: selectedProjectID,
                    isCurrent: isCurrent, onSelection: onSelection)
            case .create:
                ProjectNameForm(
                    library: library, projectID: nil, isCurrent: isCurrent, onCreated: onSelection,
                    onCreatedProject: onSelection == nil ? onCreatedProject : nil, isRoot: isRoot)
            case .manage(let id):
                ProjectManagementView(library: library, projectID: id, isCurrent: isCurrent)
            case .rename(let id):
                ProjectNameForm(library: library, projectID: id, isCurrent: isCurrent, onCreated: nil, isRoot: isRoot)
            case .instructions(let id):
                ProjectInstructionsDestination(
                    projectID: id, makeInstructionEditor: makeInstructionEditor, isCurrent: isCurrent)
            }
        }
    }

    struct ProjectPickerView: View {
        @Bindable var library: LibraryModel
        let selectedProjectID: String?
        let isCurrent: @MainActor () -> Bool
        let onSelection: (@MainActor (String?) async -> Bool)?
        @State private var selecting = false

        var body: some View {
            List {
                if onSelection != nil { selectionRow(name: libraryText("No project"), id: nil) }
                ForEach(library.projects, id: \.id) { project in
                    if onSelection != nil {
                        HStack(spacing: 8) {
                            selectionRow(name: Text(verbatim: project.name), id: project.id)
                            NavigationLink(value: ProjectSheetDestination.manage(project.id)) {
                                Image(systemName: "chevron.right")
                                    .font(.body.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                    .frame(minWidth: 48, minHeight: 48)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .navigationLinkIndicatorVisibility(.hidden)
                            .accessibilityLabel(libraryText("Project actions"))
                            .accessibilityIdentifier("project-picker.actions." + project.id)
                        }
                    } else {
                        NavigationLink(value: ProjectSheetDestination.manage(project.id)) {
                            Label {
                                Text(verbatim: project.name).foregroundStyle(.primary)
                            } icon: {
                                Image(systemName: "folder").foregroundStyle(MapleAccent.foreground)
                            }
                            .frame(minHeight: 44)
                        }
                        .accessibilityIdentifier("project-picker.manage." + project.id)
                    }
                }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(libraryText("Projects"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarVisibility(.visible, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    NavigationLink(value: ProjectSheetDestination.create) {
                        Image(systemName: "plus")
                            .frame(minWidth: 44, minHeight: 44)
                            .contentShape(Rectangle())
                    }
                    .accessibilityLabel(libraryText("New project"))
                    .accessibilityIdentifier("project-picker.new-project")
                }
            }
            .disabled(!isCurrent() || library.isMutating || selecting)
            .task { if isCurrent() { await library.loadProjects() } }
            .accessibilityIdentifier("project-picker.screen")
        }

        private func selectionRow(name: Text, id: String?) -> some View {
            Button {
                guard isCurrent(), !selecting, let onSelection else { return }
                selecting = true
                Task { @MainActor in
                    _ = await onSelection(id)
                    if isCurrent() { selecting = false }
                }
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: id == nil ? "tray" : "folder")
                        .foregroundStyle(MapleAccent.foreground)
                    name.foregroundStyle(.primary).lineLimit(2)
                    Spacer(minLength: 8)
                    if selectedProjectID == id {
                        Image(systemName: "checkmark").foregroundStyle(MapleAccent.foreground)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(selectedProjectID == id ? .isSelected : [])
            .accessibilityIdentifier("project-picker.project." + (id ?? "none"))
        }
    }

    struct ProjectNameForm: View {
        @Bindable var library: LibraryModel
        let projectID: String?
        let isCurrent: @MainActor () -> Bool
        let onCreated: (@MainActor (String?) async -> Bool)?
        var onCreatedProject: (@MainActor (String) -> Void)? = nil
        var initialName: String? = nil
        var onRenamed: (@MainActor (String) -> Void)? = nil
        let isRoot: Bool
        @Environment(\.dismiss) private var dismiss
        @State private var name = ""
        @State private var submitting = false
        @State private var loaded = false
        @State private var visible = false
        @State private var originalName = ""
        @State private var validation: String?
        @FocusState private var nameFocused: Bool

        var body: some View {
            Form {
                Section {
                    TextField(text: $name) { libraryText("Project name") }
                        .focused($nameFocused)
                        .submitLabel(.done)
                        .onSubmit { submit() }
                        .accessibilityIdentifier("project-editor.name")
                }
                if let validation {
                    libraryText(validation).foregroundStyle(MapleRedText())
                        .accessibilityIdentifier("project-editor.validation")
                }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(libraryText(projectID == nil ? "New project" : "Rename project"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarVisibility(.visible, for: .navigationBar)
            .toolbar {
                if isRoot {
                    ToolbarItem(placement: .cancellationAction) {
                        Button {
                            dismiss()
                        } label: {
                            libraryText("Cancel")
                        }
                        .disabled(submitting)
                        .accessibilityIdentifier("project-editor.cancel")
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        submit()
                    } label: {
                        libraryText(projectID == nil ? "Create" : "Save")
                    }
                    .disabled(trimmedName.isEmpty || submitting || library.isMutating || !isCurrent())
                    .accessibilityIdentifier("project-editor.save")
                }
            }
            .disabled(submitting || !isCurrent())
            .interactiveDismissDisabled(submitting)
            .onAppear { visible = true }
            .onDisappear { visible = false }
            .onChange(of: name) { _, _ in validation = nil }
            .task {
                guard !loaded, isCurrent() else { return }
                if let projectID {
                    name = initialName ?? library.projects.first(where: { $0.id == projectID })?.name ?? ""
                    originalName = name.trimmingCharacters(in: .whitespacesAndNewlines)
                }
                loaded = true; nameFocused = true
            }
        }

        private var trimmedName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }

        private func submit() {
            guard visible, isCurrent(), !submitting, !library.isMutating, !trimmedName.isEmpty else { return }
            let value = trimmedName
            if projectID != nil, value == originalName {
                validation = "Please enter a different project name."; return
            }
            validation = nil
            submitting = true; nameFocused = false
            Task { @MainActor in
                if let projectID {
                    await library.renameProject(id: projectID, name: value)
                    guard visible, isCurrent() else { return }
                    submitting = false
                    if library.issue == nil { onRenamed?(value); dismiss() }
                } else {
                    let project = await library.createProject(name: value)
                    guard visible, isCurrent() else { return }
                    if let project {
                        if let onCreated {
                            let selected = await onCreated(project.id)
                            // A membership failure leaves the newly created
                            // project available to retry from the project list.
                            if !selected, visible, isCurrent() { dismiss() }
                        } else if let onCreatedProject {
                            onCreatedProject(project.id); dismiss()
                        } else {
                            dismiss()
                        }
                    }
                    if isCurrent() { submitting = false }
                }
            }
        }
    }

    private struct ProjectManagementView: View {
        @Bindable var library: LibraryModel
        let projectID: String
        let isCurrent: @MainActor () -> Bool
        @Environment(\.dismiss) private var dismiss
        @State private var deleting = false

        var body: some View {
            List {
                Section {
                    NavigationLink(value: ProjectSheetDestination.rename(projectID)) {
                        Label {
                            libraryText("Rename project")
                        } icon: {
                            Image(systemName: "pencil")
                        }
                    }
                    .accessibilityIdentifier("project-editor.rename")
                    NavigationLink(value: ProjectSheetDestination.instructions(projectID)) {
                        Label {
                            libraryText("Project instructions")
                        } icon: {
                            Image(systemName: "text.alignleft")
                        }
                    }
                    .accessibilityIdentifier("project-editor.instructions")
                }
                Section {
                    Button(role: .destructive) {
                        if isCurrent() { deleting = true }
                    } label: {
                        Label {
                            libraryText("Delete project")
                        } icon: {
                            Image(systemName: "trash")
                        }
                    }
                    .accessibilityIdentifier("project-editor.delete")
                }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .navigationTitle(library.projects.first(where: { $0.id == projectID })?.name ?? "")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarVisibility(.visible, for: .navigationBar)
            .disabled(!isCurrent() || library.isMutating)
            .sheet(isPresented: $deleting) {
                NavigationStack {
                    DeleteLibraryContentView(
                        title: "Delete this project?", actionTitle: "Delete project",
                        explanation: Text(
                            String(
                                localized:
                                    "This will permanently delete \"%@\" and any chats in it. Move chats out first if you want to keep them. This action cannot be undone.",
                                defaultValue:
                                    "This will permanently delete \"\(library.projects.first(where: { $0.id == projectID })?.name ?? "")\" and any chats in it. Move chats out first if you want to keep them. This action cannot be undone.",
                                table: "Library", bundle: .module))
                    ) {
                        guard isCurrent() else { return .retired }
                        await library.deleteProject(id: projectID)
                        guard isCurrent() else { return .retired }
                        if library.issue == nil { deleting = false; dismiss() }
                        return library.issue
                    }
                }
            }
        }
    }

    private struct ProjectInstructionsDestination: View {
        let projectID: String
        let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        let isCurrent: @MainActor () -> Bool
        @State private var editor: InstructionEditor?

        var body: some View {
            Group {
                if let editor { InstructionEditorView(editor: editor) } else { Color.clear.accessibilityHidden(true) }
            }
            .toolbarVisibility(.visible, for: .navigationBar)
            .task {
                guard editor == nil, isCurrent() else { return }
                editor = makeInstructionEditor(.project(projectID))
            }
        }
    }
#endif
