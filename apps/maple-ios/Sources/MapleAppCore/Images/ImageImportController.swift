import Foundation
import Observation

public struct SelectedImageSource: Sendable {
    public let read: @Sendable () async throws -> Data
    public init(read: @escaping @Sendable () async throws -> Data) { self.read = read }
}

@MainActor public final class ImageImportBudget {
    private var tickets: [UUID: Int] = [:]
    private let maximumTemporaryBytes: Int
    public init(maximumTemporaryBytes: Int = 256 * 1_024 * 1_024) {
        self.maximumTemporaryBytes = min(256 * 1_024 * 1_024, max(0, maximumTemporaryBytes))
    }
    fileprivate func acquire() -> UUID? {
        guard tickets.count < 4 else { return nil }
        let ticket = UUID(); tickets[ticket] = 0; return ticket
    }
    fileprivate func release(_ ticket: UUID) { tickets.removeValue(forKey: ticket) }
    fileprivate func reserve(_ bytes: Int, ticket: UUID) throws {
        guard bytes >= 0, tickets[ticket] != nil else { throw ImageImportIssue.retired }
        let others = tickets.filter { $0.key != ticket }.values.reduce(0, +)
        guard bytes <= maximumTemporaryBytes - others else { throw ImageImportIssue.attachmentLimit }
        tickets[ticket] = bytes
    }
}

/// A picker result belongs to the draft which opened it. Importing into held B
/// is allowed; importing into a sent/retired draft or a replacement account is not.
@MainActor @Observable public final class ImageImportController {
    public let runtime: ChatRuntime
    public private(set) var issue: ImageImportIssue? { didSet { scheduleIssueExpiry() } }
    public private(set) var importingDrafts: Set<UUID> = []
    @ObservationIgnored private let processor: any ImageProcessing
    @ObservationIgnored private let budget: ImageImportBudget
    @ObservationIgnored private let admission: @MainActor () -> Bool
    @ObservationIgnored private let capacityIssue: @MainActor (ChatSnapshot) -> ImageImportIssue?
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private(set) var issueExpiry: Task<Void, Never>?
    @ObservationIgnored private var issueOccurrence = UUID()
    @ObservationIgnored private var jobs: [UUID: Task<Void, Never>] = [:]
    @ObservationIgnored private var tickets: [UUID: UUID] = [:]
    private var valid = true

    public init(
        runtime: ChatRuntime, processor: any ImageProcessing, budget: ImageImportBudget = ImageImportBudget(),
        capacityIssue: @escaping @MainActor (ChatSnapshot) -> ImageImportIssue? = { _ in nil },
        sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
        admission: @escaping @MainActor () -> Bool = { true }
    ) {
        self.runtime = runtime; self.processor = processor; self.budget = budget; self.sleep = sleep
        self.admission = admission; self.capacityIssue = capacityIssue
    }

    private func scheduleIssueExpiry() {
        issueExpiry?.cancel(); issueExpiry = nil
        issueOccurrence = UUID()
        guard valid, issue != nil else { return }
        let occurrence = issueOccurrence, sleep = sleep
        issueExpiry = Task { [weak self] in
            do { try await sleep(.seconds(5)) } catch { return }
            guard !Task.isCancelled, let self, valid, issueOccurrence == occurrence else { return }
            issue = nil
        }
    }
    public func waitForImports() async {
        for job in Array(jobs.values) { await job.value }
    }

    public func importImages(_ sources: [SelectedImageSource], draftID: UUID) {
        guard valid, !runtime.isRetired, !runtime.isLoading, runtime.deletionFenceIDs.isEmpty,
            let draft = runtime.ownedDraft(id: draftID), !sources.isEmpty
        else { issue = .retired; return }
        guard admission() else { issue = .unavailable; return }
        guard !runtime.busyDraftIDs.contains(draftID) else { issue = .unavailable; return }
        guard sources.count <= 10 - draft.images.count else { issue = .attachmentLimit; return }
        guard let ticket = budget.acquire() else { issue = .unavailable; return }
        tickets[draftID] = ticket
        importingDrafts.insert(draftID); runtime.busyDraftIDs.insert(draftID); issue = nil
        let processor = processor, budget = budget
        jobs[ticket] = Task { [weak self] in
            defer { budget.release(ticket); self?.finish(draftID: draftID, ticket: ticket) }
            do {
                var imported: [ChatImageAttachment] = []
                for source in sources {
                    guard self?.admission() == true else { throw ImageImportIssue.unavailable }
                    let data = try await source.read()
                    try Task.checkCancellation()
                    guard self?.admission() == true else { throw ImageImportIssue.unavailable }
                    let retainedBytes = imported.reduce(0) { $0 + $1.data.count }
                    try budget.reserve(retainedBytes + data.count, ticket: ticket)
                    let image = try await processor.process(data)
                    try Task.checkCancellation()
                    try budget.reserve(retainedBytes + image.data.count, ticket: ticket)
                    guard let self, valid, tickets[draftID] == ticket,
                        let proposed = proposedImages(imported + [image], draftID: draftID)
                    else { return }
                    guard admission() else { throw ImageImportIssue.unavailable }
                    try validateCapacity(proposed)
                    imported.append(image)
                }
                guard let self, valid, tickets[draftID] == ticket,
                    var current = runtime.ownedDraft(id: draftID)
                else { return }
                guard current.images.count + imported.count <= 10 else { throw ImageImportIssue.attachmentLimit }
                current.images += imported
                var proposed = runtime.snapshot
                if proposed.draft.id == draftID {
                    proposed.draft = current
                } else if proposed.queue?.edit?.heldDraft.id == draftID {
                    proposed.queue?.edit?.heldDraft = current
                } else {
                    return
                }
                guard admission() else { throw ImageImportIssue.unavailable }
                try validateCapacity(proposed)
                _ = runtime.updateOwnedDraft(current)
            } catch is CancellationError { return } catch {
                guard let self, valid, tickets[draftID] == ticket else { return }
                issue = (error as? ImageImportIssue) ?? .unavailable
            }
        }
    }

    private func validateCapacity(_ proposed: ChatSnapshot) throws {
        let usage = ChatResourceUsage(snapshots: [proposed])
        if usage.images > 640 { throw ImageImportIssue.accountImageLimit }
        if usage.imageBytes > 256 * 1_024 * 1_024 { throw ImageImportIssue.accountAttachmentLimit }
        if let issue = capacityIssue(proposed) { throw issue }
        guard usage.fitsAccount, runtime.canAdmitSnapshot?(proposed) ?? true else {
            throw ImageImportIssue.attachmentLimit
        }
    }

    private func proposedImages(_ images: [ChatImageAttachment], draftID: UUID) -> ChatSnapshot? {
        guard var current = runtime.ownedDraft(id: draftID), !runtime.isRetired else { return nil }
        current.images += images
        var proposed = runtime.snapshot
        if proposed.draft.id == draftID {
            proposed.draft = current
        } else if proposed.queue?.edit?.heldDraft.id == draftID {
            proposed.queue?.edit?.heldDraft = current
        } else {
            return nil
        }
        return proposed
    }

    public func cancel(draftID: UUID) {
        guard let ticket = tickets.removeValue(forKey: draftID) else { return }
        jobs[ticket]?.cancel()
        importingDrafts.remove(draftID); runtime.busyDraftIDs.remove(draftID)
    }

    public func removeImage(_ id: UUID, draftID: UUID) {
        guard valid, var draft = runtime.ownedDraft(id: draftID) else { return }
        draft.images.removeAll { $0.id == id }; _ = runtime.updateOwnedDraft(draft)
    }

    public func report(_ value: ImageImportIssue) { if valid { issue = value } }
    public func invalidate() {
        valid = false
        for owner in Array(tickets.keys) { cancel(draftID: owner) }
        issue = nil
    }

    private func finish(draftID: UUID, ticket: UUID) {
        jobs.removeValue(forKey: ticket)
        guard tickets[draftID] == ticket else { return }
        tickets.removeValue(forKey: draftID)
        importingDrafts.remove(draftID); runtime.busyDraftIDs.remove(draftID)
    }
}
