import Foundation
import MapleSDK
@testable import MapleAppCore

actor ChatTestGate {
    var entered = false
    var released = false
    var arrivalWaiters: [CheckedContinuation<Void, Never>] = []
    var releaseWaiters: [CheckedContinuation<Void, Never>] = []
    func hold() async {
        entered = true
        arrivalWaiters.forEach { $0.resume() }; arrivalWaiters.removeAll()
        if !released { await withCheckedContinuation { releaseWaiters.append($0) } }
    }
    func waitForArrival() async {
        if !entered { await withCheckedContinuation { arrivalWaiters.append($0) } }
    }
    func release() {
        released = true; releaseWaiters.forEach { $0.resume() }; releaseWaiters.removeAll()
    }
}

actor ChatFixtureStore: ChatPersistence {
    var values: [String: ChatSnapshot] = [:]
    var saved: [ChatSnapshot] = []
    var fail = false
    var gate: ChatTestGate?
    var retired: Set<UUID> = []
    var removeGate: ChatTestGate?
    func holdRemoval(_ gate: ChatTestGate) { removeGate = gate }
    var dispatchSaves = 0
    var failingDispatchGate: ChatTestGate?
    var failingDispatchNumber: Int?
    func configure(fail: Bool = false, gate: ChatTestGate? = nil) { self.fail = fail; self.gate = gate }
    func seed(_ snapshot: ChatSnapshot) { values[snapshot.storageKey] = snapshot }
    func failDispatchSave(_ number: Int, gate: ChatTestGate) {
        failingDispatchNumber = number; failingDispatchGate = gate
    }
    func load(chatID: String, scope: AccountScope) async throws -> ChatSnapshot? { values[chatID] }
    func loadAll(scope: AccountScope) async throws -> [ChatSnapshot] { Array(values.values) }
    func save(_ snapshot: ChatSnapshot, scope: AccountScope) async throws {
        if snapshot.pendingRun?.dispatched == true, snapshot.pendingRun?.phase == .sending {
            dispatchSaves += 1
            if dispatchSaves == failingDispatchNumber, let failingDispatchGate {
                await failingDispatchGate.hold(); throw ChatIssue.storageUnavailable
            }
        }
        if let gate { await gate.hold() }
        guard !fail, !retired.contains(scope.generation) else { throw ChatIssue.storageUnavailable }
        if (values[snapshot.storageKey]?.revision ?? 0) <= snapshot.revision { values[snapshot.storageKey] = snapshot }
        saved.append(snapshot)
    }
    func remove(chatID: String, scope: AccountScope) async throws {
        if let removeGate { await removeGate.hold() }
        values.removeValue(forKey: chatID)
    }
    func rekey(from: String, to snapshot: ChatSnapshot, scope: AccountScope) async throws {
        guard !fail, !retired.contains(scope.generation) else { throw ChatIssue.storageUnavailable }
        values.removeValue(forKey: from); values[snapshot.storageKey] = snapshot; saved.append(snapshot)
    }
    func retire(_ scope: AccountScope) { retired.insert(scope.generation); values.removeAll() }
    func recorded(_ key: String) -> ChatSnapshot? { values[key] }
    func hasDurableRequest(_ request: ResponsesCreateRequest) -> Bool {
        guard let run = values[request.conversation]?.pendingRun,
            request.metadata?["internal_message_id"] == .string(run.messageID)
        else { return false }
        return run.dispatched && UUID(uuidString: run.messageID) != nil
    }
}

actor ChatFixtureService: ChatServing {
    let store: ChatFixtureStore
    var creationGate: ChatTestGate?
    var creationError: (any Error)?
    var createdMetadata: [String: JSONValue]?
    var listedConversations: [APIPage<Conversation>] = []
    var responseGate: ChatTestGate?
    var responseErrors: [any Error] = []
    var script: [ResponseStreamEvent]?
    var continuation: AsyncThrowingStream<ResponseStreamEvent, Error>.Continuation?
    var posts: [ResponsesCreateRequest] = []
    var budgets: [Int] = []
    var durableAtDispatch: [Bool] = []
    var creations = 0
    var itemRequests: [String] = []
    var retrievals: [String] = []
    var cancellations: [String] = []
    var exactItem: ConversationItem?
    var retrieved = chatResponse("response", .completed)
    var cancelError: (any Error)?
    var page = APIPage<ConversationItem>(data: [])
    var pageError: (any Error)?
    var pageRequests: [ListParameters] = []
    var pageGate: ChatTestGate?
    func setPageGate(_ gate: ChatTestGate?) { pageGate = gate }
    var postWaiters: [(Int, CheckedContinuation<Void, Never>)] = []
    var cancellationWaiters: [CheckedContinuation<Void, Never>] = []
    var itemWaiters: [CheckedContinuation<Void, Never>] = []

    init(store: ChatFixtureStore) { self.store = store }
    func setCreationGate(_ value: ChatTestGate) { creationGate = value }
    func setResponseGate(_ value: ChatTestGate) { responseGate = value }
    func setResponseErrors(_ values: [any Error]) { responseErrors = values }
    func setScript(_ value: [ResponseStreamEvent]?) { script = value }
    func setExactItem(_ value: ConversationItem?) { exactItem = value }
    func setRetrieved(_ value: ResearchResponse) { retrieved = value }
    func setCancelError(_ value: (any Error)?) { cancelError = value }
    func setPage(_ value: APIPage<ConversationItem>) { page = value }
    func setPageError(_ value: (any Error)?) { pageError = value }

    func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) async throws
        -> Conversation
    {
        creations += 1
        if let creationGate { await creationGate.hold() }
        createdMetadata = metadata
        if let creationError { throw creationError }
        return Conversation(
            id: "conversation", object: "conversation", createdAt: 0, metadata: metadata, projectId: projectId,
            pinned: false, lastActivityAt: 0)
    }
    func listConversations(_ parameters: ConversationsListParameters) async throws -> APIPage<Conversation> {
        if !listedConversations.isEmpty { return listedConversations.removeFirst() }
        return APIPage(data: [])
    }
    func configureCreationFailure(_ error: any Error) { creationError = error }
    func configureConversationPages(_ pages: [APIPage<Conversation>]) { listedConversations = pages }
    func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) async throws
        -> AsyncThrowingStream<ResponseStreamEvent, Error>
    {
        durableAtDispatch.append(await store.hasDurableRequest(request))
        posts.append(request); budgets.append(maximumInferenceSends)
        let pair = AsyncThrowingStream<ResponseStreamEvent, Error>.makeStream()
        continuation = pair.continuation
        let ready = postWaiters.filter { $0.0 <= posts.count }; postWaiters.removeAll { $0.0 <= posts.count }
        ready.forEach { $0.1.resume() }
        if let responseGate { await responseGate.hold() }
        if !responseErrors.isEmpty { throw responseErrors.removeFirst() }
        if let script { for event in script { pair.continuation.yield(event) }; pair.continuation.finish() }
        return pair.stream
    }
    func retrieveResponse(id: String) async throws -> ResearchResponse { retrievals.append(id); return retrieved }
    func cancelResponse(id: String) async throws -> ResearchResponse {
        cancellations.append(id); cancellationWaiters.forEach { $0.resume() }; cancellationWaiters.removeAll()
        if let cancelError { throw cancelError }
        return chatResponse(id, .cancelled)
    }
    func getConversationItem(conversationId: String, itemId: String) async throws -> ConversationItem {
        itemRequests.append(itemId)
        itemWaiters.forEach { $0.resume() }; itemWaiters.removeAll()
        guard let exactItem else { throw ResearchAPIError.http(status: 404, reason: .notFound) }
        return exactItem
    }
    func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
        ConversationItem
    > {
        pageRequests.append(parameters)
        let captured = page
        if let pageGate { await pageGate.hold() }
        if let pageError { throw pageError }
        return captured
    }
    func emit(_ event: ResponseStreamEvent) { continuation?.yield(event) }
    func finish(error: (any Error)? = nil) {
        if let error { continuation?.finish(throwing: error) } else { continuation?.finish() }
    }
    func waitForPosts(_ count: Int = 1) async {
        if posts.count < count { await withCheckedContinuation { postWaiters.append((count, $0)) } }
    }
    func waitForCancellation() async {
        if cancellations.isEmpty { await withCheckedContinuation { cancellationWaiters.append($0) } }
    }
    func waitForItemRequest() async {
        if itemRequests.isEmpty { await withCheckedContinuation { itemWaiters.append($0) } }
    }
}

actor ChatFailingRekeyStore: ChatPersistence {
    let underlying: ProtectedChatStore
    let gate: ChatTestGate
    let commitBeforeFailure: Bool
    var failed = false
    init(underlying: ProtectedChatStore, gate: ChatTestGate, commitBeforeFailure: Bool) {
        self.underlying = underlying; self.gate = gate; self.commitBeforeFailure = commitBeforeFailure
    }
    func load(chatID: String, scope: AccountScope) async throws -> ChatSnapshot? {
        try await underlying.load(chatID: chatID, scope: scope)
    }
    func loadAll(scope: AccountScope) async throws -> [ChatSnapshot] { try await underlying.loadAll(scope: scope) }
    func save(_ snapshot: ChatSnapshot, scope: AccountScope) async throws {
        try await underlying.save(snapshot, scope: scope)
    }
    func remove(chatID: String, scope: AccountScope) async throws {
        try await underlying.remove(chatID: chatID, scope: scope)
    }
    func rekey(from: String, to snapshot: ChatSnapshot, scope: AccountScope) async throws {
        if snapshot.conversationID != nil, !failed {
            failed = true
            if commitBeforeFailure { try await underlying.rekey(from: from, to: snapshot, scope: scope) }
            await gate.hold()
            throw ChatIssue.storageUnavailable
        }
        try await underlying.rekey(from: from, to: snapshot, scope: scope)
    }
}

func chatResponse(_ id: String, _ status: ResponseStatus, output: [ConversationItem]? = nil) -> ResearchResponse {
    ResearchResponse(id: id, object: "response", createdAt: 0, status: status, model: "auto:quick", output: output)
}

func chatLifecycle(_ response: ResearchResponse) throws -> ResponseLifecycleEvent {
    struct Wrapper: Encodable { var response: ResearchResponse }
    let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
    let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(ResponseLifecycleEvent.self, from: encoder.encode(Wrapper(response: response)))
}

func chatDelta(_ text: String, item: String = "assistant", index: Int = 0) throws -> ResponseTextEvent {
    let json: [String: Any] = ["item_id": item, "output_index": 0, "content_index": index, "delta": text]
    let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(ResponseTextEvent.self, from: JSONSerialization.data(withJSONObject: json))
}
