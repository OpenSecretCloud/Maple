#if os(iOS)
    import MapleAppCore
    import SwiftUI

    /// Choosing or creating a project closes the picker. Its account-scoped
    /// library retains the last successful snapshot during background refresh.
    struct ProjectPickerView: View {
        @Bindable var library: LibraryModel
        let selectedProjectID: String?
        let onSelection: (String?) -> Void
        @State private var creatingProject = false
        @State private var projectName = ""

        var body: some View {
            List {
                projectRow(name: libraryText("No project"), id: nil)
                ForEach(library.projects, id: \.id) { project in
                    projectRow(name: Text(verbatim: project.name), id: project.id)
                }
                if let issue = library.issue { LibraryIssueView(issue: issue) }
            }
            .listStyle(.plain)
            .listRowBackground(Color.clear)
            .scrollContentBackground(.hidden)
            .background(MapleSurface.canvas)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .navigationTitle(libraryText("Projects"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        projectName = ""; creatingProject = true
                    } label: {
                        Image(systemName: "folder.badge.plus")
                    }
                    .accessibilityLabel(libraryText("New project"))
                    .accessibilityIdentifier("project-picker.new-project")
                }
            }
            .alert(libraryText("New project"), isPresented: $creatingProject) {
                TextField(text: $projectName) { libraryText("Project name") }
                Button(role: .cancel) {
                } label: {
                    libraryText("Cancel")
                }
                Button {
                    let name = projectName.trimmingCharacters(in: .whitespacesAndNewlines)
                    Task {
                        guard let project = await library.createProject(name: name), !Task.isCancelled else { return }
                        select(project.id)
                    }
                } label: {
                    libraryText("Create")
                }
                .disabled(projectName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .task { await library.loadProjects() }
        }

        private func projectRow(name: Text, id: String?) -> some View {
            Button {
                select(id)
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: id == nil ? "tray" : "folder").foregroundStyle(.secondary)
                    name.foregroundStyle(.primary).lineLimit(1)
                    Spacer(minLength: 8)
                    if selectedProjectID == id {
                        Image(systemName: "checkmark").foregroundStyle(MapleAccent.foreground)
                    }
                }
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(selectedProjectID == id ? .isSelected : [])
            .accessibilityIdentifier("project-picker.project." + (id ?? "none"))
        }

        private func select(_ id: String?) {
            // The presenting owner admits this callback and dismisses its sheet;
            // an alert covering the list must not retire the selection request.
            onSelection(id)
        }
    }
#endif
