#if os(iOS)
    import MapleAppCore
    import SwiftUI

    public struct InstructionEditorView: View {
        @Bindable private var editor: InstructionEditor
        @Environment(\.dismiss) private var dismiss
        @State private var confirmDiscard = false
        public init(editor: InstructionEditor) { self.editor = editor }
        public var body: some View {
            VStack(alignment: .leading, spacing: 12) {
                if editor.isLoaded {
                    Text(
                        "Instructions apply to future messages. Save to apply your changes.", tableName: "Library",
                        bundle: .module
                    )
                    .font(.callout).foregroundStyle(.secondary)
                    TextEditor(text: $editor.text).disabled(editor.isSaving)
                        .overlay(alignment: .topLeading) {
                            if isProject, editor.text.isEmpty {
                                libraryText("e.g. Use concise bullet points. Focus on practical examples.")
                                    .foregroundStyle(MapleFieldPrompt()).padding(.top, 8).padding(.leading, 5)
                                    .allowsHitTesting(false).accessibilityHidden(true)
                            }
                        }
                        .accessibilityLabel(Text("Instructions", tableName: "Library", bundle: .module))
                        .accessibilityIdentifier("library.instructions-text")
                        .scrollContentBackground(.hidden)
                    if isProject {
                        libraryText("Leave this empty to clear the project's custom instructions.")
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                } else if !editor.isLoading {
                    Button {
                        Task { await editor.load() }
                    } label: {
                        Text("Try again", tableName: "Library", bundle: .module)
                    }
                }
                if let issue = editor.issue { LibraryIssueView(issue: issue) }
            }
            .padding().frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(MapleSurface.canvas)
            .navigationTitle(Text("Instructions", tableName: "Library", bundle: .module))
            .navigationBarTitleDisplayMode(.inline)
            .navigationBarBackButtonHidden()
            .interactiveDismissDisabled(editor.isDirty || editor.isSaving)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button {
                        if editor.isDirty { confirmDiscard = true } else { dismiss() }
                    } label: {
                        Text("Cancel", tableName: "Library", bundle: .module)
                    }.disabled(editor.isSaving).accessibilityIdentifier("library.instructions-cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        Task {
                            await editor.save(); if editor.didSave { dismiss() }
                        }
                    } label: {
                        Text("Save", tableName: "Library", bundle: .module)
                    }.disabled(!editor.isLoaded || editor.isSaving || !editor.isDirty)
                        .accessibilityIdentifier("library.instructions-save")
                }
            }
            .confirmationDialog(
                Text("Discard unsaved changes?", tableName: "Library", bundle: .module), isPresented: $confirmDiscard,
                titleVisibility: .visible
            ) {
                Button(role: .destructive) {
                    editor.discardChanges(); dismiss()
                } label: {
                    Text("Discard changes", tableName: "Library", bundle: .module)
                }
            }
            .task { await editor.load() }
        }
        private var isProject: Bool {
            if case .project = editor.destination { return true }
            return false
        }
    }

    struct LibraryIssueView: View {
        let issue: LibraryIssue
        var body: some View {
            libraryText(message).foregroundStyle(MapleRedText())
                .accessibilityIdentifier("library.error")
        }
        private var message: String {
            switch issue {
            case .connection: "Maple could not load or update this information. Check your connection and try again."
            case .invalidInput: "Check the name or selection and try again."
            case .invalidResponse: "Maple could not verify the service response. Refresh before trying again."
            case .busy: "Another account update is in progress. Wait for it to finish."
            case .retired: "Your sign-in changed. Reopen this screen to continue."
            case .quiescence:
                "Maple could not confirm that all affected requests stopped. Nothing was deleted. Try again after the requests settle."
            case .cleanupRequired:
                "The server deletion finished, but local cleanup needs another attempt before more changes can be made."
            case .partialMutation:
                "Some selected chats could not be updated. The remaining selection still needs attention."
            }
        }
    }
#endif
