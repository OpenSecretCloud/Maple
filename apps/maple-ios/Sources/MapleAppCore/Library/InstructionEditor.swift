import Foundation
import MapleSDK
import Observation

public enum InstructionDestination: Sendable, Equatable { case account, project(String) }

/// Explicit-save editor. A failed load is not interpreted as an empty default,
/// and navigation can use isDirty to ask whether to discard unsaved edits.
@MainActor @Observable public final class InstructionEditor {
    public let scope: AccountScope
    public let destination: InstructionDestination
    public private(set) var isLoading = false
    public private(set) var isSaving = false
    public private(set) var isLoaded = false
    public private(set) var didSave = false
    public private(set) var issue: LibraryIssue?
    private var draft = ""
    private var original = ""
    private var instructionID: String?
    @ObservationIgnored private let service: any LibraryServing
    @ObservationIgnored private var valid = true
    @ObservationIgnored private var operation = UUID()
    @ObservationIgnored private var editRevision: UInt64 = 0
    public var text: String {
        get { draft }
        set { guard valid, isLoaded, !isSaving else { return }; draft = newValue; didSave = false; editRevision &+= 1 }
    }
    public var isDirty: Bool { isLoaded && draft != original }

    public init(scope: AccountScope, destination: InstructionDestination, service: any LibraryServing) {
        self.scope = scope; self.destination = destination; self.service = service
    }
    public func load() async {
        guard valid, !isLoading, !isSaving, !isDirty else { return }
        let ticket = UUID(); operation = ticket; isLoading = true; issue = nil; didSave = false
        let loadedRevision = editRevision
        do {
            let text: String
            var loadedInstructionID: String?
            switch destination {
            case .project(let id):
                let project = try await service.getProject(id: id)
                guard owns(ticket) else { return }
                guard project.id == id else { throw LibraryIssue.invalidResponse }
                text = project.instructions ?? ""
            case .account:
                let current = try await loadDefault(ticket)
                guard owns(ticket) else { return }
                loadedInstructionID = current?.id; text = current?.prompt ?? ""
            }
            guard owns(ticket) else { return }
            guard loadedRevision == editRevision else { isLoading = false; return }
            instructionID = loadedInstructionID
            draft = text; original = text; isLoaded = true; isLoading = false
        } catch { if owns(ticket) { isLoading = false; issue = LibraryIssue.present(error) } }
    }
    public func save() async {
        guard valid, isLoaded, !isLoading, !isSaving else { return }
        let ticket = UUID(); operation = ticket; isSaving = true; issue = nil; didSave = false
        let submitted = draft
        let empty = submitted.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        do {
            switch destination {
            case .project(let id):
                let updated = try await service.updateProject(
                    id: id, name: nil, instructions: empty ? .clear : .set(submitted))
                guard owns(ticket) else { return }
                guard updated.id == id else { throw LibraryIssue.invalidResponse }
            case .account:
                if let id = instructionID {
                    if empty {
                        let deleted = try await service.deleteInstruction(id: id)
                        guard owns(ticket) else { return }
                        guard deleted.deleted, deleted.id == nil || deleted.id == id else {
                            throw LibraryIssue.invalidResponse
                        }
                        instructionID = nil
                    } else {
                        let updated = try await service.updateInstruction(
                            id: id, name: nil, prompt: submitted, isDefault: nil)
                        guard owns(ticket) else { return }
                        guard updated.id == id else { throw LibraryIssue.invalidResponse }
                    }
                } else if !empty {
                    let created = try await service.createInstruction(
                        name: "User Preferences", prompt: submitted, isDefault: true)
                    guard owns(ticket) else { return }
                    guard created.isDefault, !created.id.isEmpty else { throw LibraryIssue.invalidResponse }
                    instructionID = created.id
                }
            }
            guard owns(ticket) else { return }
            draft = empty ? "" : submitted; original = draft; isSaving = false; didSave = true
        } catch { if owns(ticket) { isSaving = false; issue = LibraryIssue.present(error) } }
    }
    public func discardChanges() {
        guard valid, !isSaving else { return }; draft = original; didSave = false; editRevision &+= 1
    }
    public func invalidate() {
        valid = false; operation = UUID(); draft = ""; original = ""; instructionID = nil
        isLoaded = false; isLoading = false; isSaving = false; didSave = false
    }
    private func owns(_ ticket: UUID) -> Bool { valid && operation == ticket }
    private func loadDefault(_ ticket: UUID) async throws -> Instruction? {
        var after: String?, visited = Set<String>(), current: Instruction?
        for _ in 0..<100 {
            let page = try await service.listInstructions(ListParameters(limit: 100, after: after, order: .desc))
            guard owns(ticket) else { throw LibraryIssue.retired }
            for instruction in page.data where instruction.isDefault {
                guard current == nil || current?.id == instruction.id else { throw LibraryIssue.invalidResponse }
                current = instruction
            }
            if !page.hasMore { return current }
            guard let next = page.lastId, !page.data.isEmpty, visited.insert(next).inserted else {
                throw LibraryIssue.invalidResponse
            }
            after = next
        }
        throw LibraryIssue.invalidResponse
    }
}
