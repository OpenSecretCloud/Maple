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
        private let service: SDKLibraryService
        private let deletions: ChatDeletionCoordinator
        private var active = true
        private var models: [WeakLibraryOwner<LibraryModel>] = []
        private var editors: [WeakLibraryOwner<InstructionEditor>] = []
        var onConversationsDeleted: ((Set<String>, Bool) -> Void)?
        var onConversationUpdated: ((Conversation) -> Void)?
        var onConversationProjectUpdated: ((Set<String>, String?) -> Void)?
        var onProjectDeleted: ((String) -> Void)?

        init(scope: AccountScope, api: ResearchAPI, deletions: ChatDeletionCoordinator) {
            self.scope = scope; service = SDKLibraryService(api: api); self.deletions = deletions
            root = LibraryModel(scope: scope, service: service, deletions: deletions)
            attach(root)
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
