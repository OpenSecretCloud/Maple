#if os(iOS)
    import MapleAppCore
    import MapleSDK

    @MainActor private final class WeakLibraryOwner<Value: AnyObject> {
        weak var value: Value?
        init(_ value: Value) { self.value = value }
    }

    /// Independent page cursors share one account and deletion authority. The
    /// registry retains no invisible page contents after its view is released.
    @MainActor final class LibraryContext {
        let root: LibraryModel
        let scope: AccountScope
        let projectExpansion: LibraryProjectExpansionController
        let menu: LibraryMenuModel
        private let service: SDKLibraryService
        private let deletions: ChatDeletionCoordinator
        private var active = true
        private var models: [WeakLibraryOwner<LibraryModel>] = []
        private var editors: [WeakLibraryOwner<InstructionEditor>] = []
        var onConversationsDeleted: ((Set<String>, Bool) -> Void)?
        var onConversationUpdated: ((Conversation) -> Void)?
        var onConversationProjectUpdated: ((Set<String>, String?) -> Void)?
        var onProjectDeleted: ((String) -> Void)?

        init(
            scope: AccountScope, api: ResearchAPI, deletions: ChatDeletionCoordinator,
            projectExpansion: LibraryProjectExpansionController
        ) {
            let service = SDKLibraryService(api: api)
            self.scope = scope; self.service = service; self.deletions = deletions
            self.projectExpansion = projectExpansion
            let root = LibraryModel(scope: scope, service: service, deletions: deletions)
            self.root = root
            menu = LibraryMenuModel(root: root) {
                LibraryModel(scope: scope, service: service, deletions: deletions)
            }
            attach(root); attach(menu.pinned); attach(menu.recent)
            menu.onModelCreated = { [weak self] model in self?.attach(model) }
        }

        func prepareMenu() async {
            await menu.prepare { [weak projectExpansion] in projectExpansion?.isExpanded(id: $0) ?? false }
        }

        func refreshMenu() async {
            await menu.refresh { [weak projectExpansion] in projectExpansion?.isExpanded(id: $0) ?? false }
        }

        func makeLibrary() -> LibraryModel {
            let model = LibraryModel(scope: scope, service: service, deletions: deletions)
            attach(model)
            if !active { model.invalidate() }
            return model
        }

        func makeInstructionEditor(_ destination: InstructionDestination) -> InstructionEditor {
            let editor = InstructionEditor(scope: scope, destination: destination, service: service)
            editors.removeAll { $0.value == nil }; editors.append(WeakLibraryOwner(editor))
            if !active { editor.invalidate() }
            return editor
        }

        func invalidate() {
            active = false
            menu.invalidate()
            projectExpansion.invalidate()
            for model in models { model.value?.invalidate() }
            for editor in editors { editor.value?.invalidate() }
            models.removeAll(); editors.removeAll()
            onConversationsDeleted = nil; onConversationUpdated = nil; onConversationProjectUpdated = nil
            onProjectDeleted = nil
        }

        private func attach(_ model: LibraryModel) {
            models.removeAll { $0.value == nil }; models.append(WeakLibraryOwner(model))
            model.onConversationsDeleted = { [weak self, weak model] ids, all in
                guard let self, active else { return }
                for other in models.compactMap(\.value) where other !== model {
                    other.receiveDeletion(ids, all: all, from: scope)
                }
                onConversationsDeleted?(ids, all)
            }
            model.onConversationUpdated = { [weak self, weak model] value in
                guard let self, active else { return }
                for other in models.compactMap(\.value) where other !== model {
                    other.receiveConversationUpdate(value, from: scope)
                }
                onConversationUpdated?(value)
            }
            model.onConversationProjectUpdated = { [weak self, weak model] ids, projectID in
                guard let self, active else { return }
                for other in models.compactMap(\.value) where other !== model {
                    other.receiveProjectMembership(ids, projectID: projectID, from: scope)
                }
                onConversationProjectUpdated?(ids, projectID)
            }
            model.onProjectDeleted = { [weak self, weak model] id in
                guard let self, active else { return }
                projectExpansion.setExpanded(id: id, expanded: false)
                for other in models.compactMap(\.value) where other !== model {
                    other.receiveProjectDeletion(id, from: scope)
                }
                onProjectDeleted?(id)
            }
            model.onProjectUpdated = { [weak self, weak model] value in
                guard let self, active else { return }
                for other in models.compactMap(\.value) where other !== model {
                    other.receiveProjectUpdate(value, from: scope)
                }
            }
        }
    }
#endif
