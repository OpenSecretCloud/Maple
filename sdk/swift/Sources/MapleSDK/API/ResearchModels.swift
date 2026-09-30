import Foundation

public enum ModelAliasId: String, Codable, Sendable {
    case quick = "auto:quick", powerful = "auto:powerful"
}

public enum ResponseStatus: String, Codable, Sendable {
    case queued, inProgress = "in_progress", completed, failed, cancelled, incomplete
}

public struct LoginResponse: Codable, Sendable, Equatable {
    public var id: String
    public var email: String?
    public var accessToken: String
    public var refreshToken: String

    public init(id: String, email: String? = nil, accessToken: String, refreshToken: String) {
        self.id = id
        self.email = email
        self.accessToken = accessToken
        self.refreshToken = refreshToken
    }
}

public struct UserResponse: Codable, Sendable, Equatable {
    public var user: MapleUser

    public init(user: MapleUser) {
        self.user = user
    }
}

public struct MapleUser: Codable, Sendable, Equatable {
    public var id: String
    public var name: String?
    public var email: String?
    public var emailVerified: Bool
    public var loginMethod: String
    public var createdAt: String
    public var updatedAt: String

    public init(
        id: String, name: String? = nil, email: String? = nil, emailVerified: Bool, loginMethod: String,
        createdAt: String, updatedAt: String
    ) {
        self.id = id
        self.name = name
        self.email = email
        self.emailVerified = emailVerified
        self.loginMethod = loginMethod
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

public struct CredentialUpdateResponse: Codable, Sendable, Equatable {
    public var message: String
    public var accessToken: String?
    public var refreshToken: String?

    public init(message: String, accessToken: String? = nil, refreshToken: String? = nil) {
        self.message = message
        self.accessToken = accessToken
        self.refreshToken = refreshToken
    }
}

public struct ThirdPartyTokenResponse: Codable, Sendable, Equatable {
    public var token: String

    public init(token: String) {
        self.token = token
    }
}

public struct OAuthInitiationResponse: Codable, Sendable, Equatable {
    public var authUrl: String
    public var csrfToken: String?
    public var state: String?

    public init(authUrl: String, csrfToken: String? = nil, state: String? = nil) {
        self.authUrl = authUrl
        self.csrfToken = csrfToken
        self.state = state
    }
}

public struct Conversation: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var createdAt: Int64
    public var metadata: [String: JSONValue]?
    public var projectId: String?
    public var pinned: Bool
    public var lastActivityAt: Int64

    public init(
        id: String, object: String, createdAt: Int64, metadata: [String: JSONValue]? = nil, projectId: String? = nil,
        pinned: Bool, lastActivityAt: Int64
    ) {
        self.id = id
        self.object = object
        self.createdAt = createdAt
        self.metadata = metadata
        self.projectId = projectId
        self.pinned = pinned
        self.lastActivityAt = lastActivityAt
    }
}

public struct ConversationItem: Codable, Sendable, Equatable {
    public var id: String
    public var responseId: String?
    public var createdAt: Int64?
    public var type: String
    public var status: String?
    public var role: String?
    public var content: [MessageContent]?
    public var callId: String?
    public var name: String?
    public var arguments: JSONValue?
    public var output: JSONValue?

    public init(
        id: String, responseId: String? = nil, createdAt: Int64? = nil, type: String, status: String? = nil,
        role: String? = nil, content: [MessageContent]? = nil, callId: String? = nil, name: String? = nil,
        arguments: JSONValue? = nil, output: JSONValue? = nil
    ) {
        self.id = id
        self.responseId = responseId
        self.createdAt = createdAt
        self.type = type
        self.status = status
        self.role = role
        self.content = content
        self.callId = callId
        self.name = name
        self.arguments = arguments
        self.output = output
    }
}

public struct MessageContent: Codable, Sendable, Equatable {
    public var type: String
    public var text: String?
    public var filename: String?
    public var audio: String?
    public var transcript: String?
    public var id: String?
    public var imageUrl: JSONValue?
    public var detail: String?
    public var annotations: [JSONValue]?

    public init(
        type: String, text: String? = nil, filename: String? = nil, audio: String? = nil, transcript: String? = nil,
        id: String? = nil, imageUrl: JSONValue? = nil, detail: String? = nil, annotations: [JSONValue]? = nil
    ) {
        self.type = type
        self.text = text
        self.filename = filename
        self.audio = audio
        self.transcript = transcript
        self.id = id
        self.imageUrl = imageUrl
        self.detail = detail
        self.annotations = annotations
    }
}

public struct DeletedObject: Codable, Sendable, Equatable {
    public var id: String?
    public var object: String
    public var deleted: Bool

    public init(id: String? = nil, object: String, deleted: Bool) {
        self.id = id
        self.object = object
        self.deleted = deleted
    }
}

public struct BatchDeleteItem: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var deleted: Bool
    public var error: String?

    public init(id: String, object: String, deleted: Bool, error: String? = nil) {
        self.id = id
        self.object = object
        self.deleted = deleted
        self.error = error
    }
}

public struct BatchDeleteResponse: Codable, Sendable, Equatable {
    public var object: String
    public var data: [BatchDeleteItem]

    public init(object: String, data: [BatchDeleteItem]) {
        self.object = object
        self.data = data
    }
}

public struct BatchUpdateProjectResponse: Codable, Sendable, Equatable {
    public var success: Bool

    public init(success: Bool) {
        self.success = success
    }
}

public struct ConversationProject: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var name: String
    public var instructions: String?
    public var createdAt: Int64
    public var updatedAt: Int64

    public init(
        id: String, object: String, name: String, instructions: String? = nil, createdAt: Int64, updatedAt: Int64
    ) {
        self.id = id
        self.object = object
        self.name = name
        self.instructions = instructions
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

public struct ConversationProjectListItem: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var name: String
    public var createdAt: Int64
    public var updatedAt: Int64

    public init(id: String, object: String, name: String, createdAt: Int64, updatedAt: Int64) {
        self.id = id
        self.object = object
        self.name = name
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

public struct Instruction: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var name: String
    public var prompt: String
    public var promptTokens: Int
    public var isDefault: Bool
    public var createdAt: Int64
    public var updatedAt: Int64

    public init(
        id: String, object: String, name: String, prompt: String, promptTokens: Int, isDefault: Bool, createdAt: Int64,
        updatedAt: Int64
    ) {
        self.id = id
        self.object = object
        self.name = name
        self.prompt = prompt
        self.promptTokens = promptTokens
        self.isDefault = isDefault
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

public struct Model: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var created: Int64
    public var ownedBy: String

    public init(id: String, object: String, created: Int64, ownedBy: String) {
        self.id = id
        self.object = object
        self.created = created
        self.ownedBy = ownedBy
    }
}

public struct ModelCapabilities: Codable, Sendable, Equatable {
    public var chat: Bool
    public var vision: Bool
    public var reasoning: Bool
    public var toolUse: Bool

    public init(chat: Bool, vision: Bool, reasoning: Bool, toolUse: Bool) {
        self.chat = chat
        self.vision = vision
        self.reasoning = reasoning
        self.toolUse = toolUse
    }
}

public struct ModelCatalogItem: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var created: Int64
    public var ownedBy: String
    public var provider: String?
    public var providerId: String?
    public var displayName: String
    public var shortName: String
    public var description: String?
    public var contextWindow: Int
    public var maxContextTokens: Int
    public var access: String
    public var capabilities: ModelCapabilities
    public var tasks: [String]?
    public var badges: [String]?
    public var enabled: Bool
    public var deprecated: Bool
    public var sortOrder: Int?

    public init(
        id: String, object: String, created: Int64, ownedBy: String, provider: String? = nil, providerId: String? = nil,
        displayName: String, shortName: String, description: String? = nil, contextWindow: Int, maxContextTokens: Int,
        access: String, capabilities: ModelCapabilities, tasks: [String]? = nil, badges: [String]? = nil, enabled: Bool,
        deprecated: Bool, sortOrder: Int? = nil
    ) {
        self.id = id
        self.object = object
        self.created = created
        self.ownedBy = ownedBy
        self.provider = provider
        self.providerId = providerId
        self.displayName = displayName
        self.shortName = shortName
        self.description = description
        self.contextWindow = contextWindow
        self.maxContextTokens = maxContextTokens
        self.access = access
        self.capabilities = capabilities
        self.tasks = tasks
        self.badges = badges
        self.enabled = enabled
        self.deprecated = deprecated
        self.sortOrder = sortOrder
    }
}

public struct ModelAlias: Codable, Sendable, Equatable {
    public var id: ModelAliasId
    public var label: String
    public var shortName: String
    public var description: String
    public var targetModel: String
    public var access: String
    public var capabilities: ModelCapabilities

    public init(
        id: ModelAliasId, label: String, shortName: String, description: String, targetModel: String, access: String,
        capabilities: ModelCapabilities
    ) {
        self.id = id
        self.label = label
        self.shortName = shortName
        self.description = description
        self.targetModel = targetModel
        self.access = access
        self.capabilities = capabilities
    }
}

public struct ModelDefaults: Codable, Sendable, Equatable {
    public var quick: ModelAliasId
    public var powerful: ModelAliasId

    public init(quick: ModelAliasId, powerful: ModelAliasId) {
        self.quick = quick
        self.powerful = powerful
    }
}

public struct AudioModelAvailability: Codable, Sendable, Equatable {
    public var available: Bool
    public var model: String
    public var displayName: String?

    public init(available: Bool, model: String, displayName: String? = nil) {
        self.available = available
        self.model = model
        self.displayName = displayName
    }
}

public struct AudioModelCatalog: Codable, Sendable, Equatable {
    public var transcription: AudioModelAvailability?
    public var speech: AudioModelAvailability?

    public init(transcription: AudioModelAvailability? = nil, speech: AudioModelAvailability? = nil) {
        self.transcription = transcription
        self.speech = speech
    }
}

public struct ModelCatalog: Codable, Sendable, Equatable {
    public var object: String
    public var data: [ModelCatalogItem]
    public var aliases: [ModelAlias]
    public var defaults: ModelDefaults
    public var audio: AudioModelCatalog?

    public init(
        object: String, data: [ModelCatalogItem], aliases: [ModelAlias], defaults: ModelDefaults,
        audio: AudioModelCatalog? = nil
    ) {
        self.object = object
        self.data = data
        self.aliases = aliases
        self.defaults = defaults
        self.audio = audio
    }
}

public struct TranscriptionResponse: Codable, Sendable, Equatable {
    public var text: String

    public init(text: String) {
        self.text = text
    }
}

public struct ResponseUsage: Codable, Sendable, Equatable {
    public var inputTokens: Int
    public var inputTokensDetails: InputTokenDetails?
    public var outputTokens: Int
    public var outputTokensDetails: OutputTokenDetails?
    public var totalTokens: Int

    public init(
        inputTokens: Int, inputTokensDetails: InputTokenDetails? = nil, outputTokens: Int,
        outputTokensDetails: OutputTokenDetails? = nil, totalTokens: Int
    ) {
        self.inputTokens = inputTokens
        self.inputTokensDetails = inputTokensDetails
        self.outputTokens = outputTokens
        self.outputTokensDetails = outputTokensDetails
        self.totalTokens = totalTokens
    }
}

public struct InputTokenDetails: Codable, Sendable, Equatable {
    public var cachedTokens: Int

    public init(cachedTokens: Int) {
        self.cachedTokens = cachedTokens
    }
}

public struct OutputTokenDetails: Codable, Sendable, Equatable {
    public var reasoningTokens: Int

    public init(reasoningTokens: Int) {
        self.reasoningTokens = reasoningTokens
    }
}

public struct ResponseFailure: Codable, Sendable, Equatable {
    public var code: String
    public var message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }
}

public struct ResearchResponse: Codable, Sendable, Equatable {
    public var id: String
    public var object: String
    public var createdAt: Int64
    public var status: ResponseStatus
    public var model: String
    public var usage: ResponseUsage?
    public var output: [ConversationItem]?
    public var error: ResponseFailure?
    public var metadata: [String: JSONValue]?

    public init(
        id: String, object: String, createdAt: Int64, status: ResponseStatus, model: String,
        usage: ResponseUsage? = nil, output: [ConversationItem]? = nil, error: ResponseFailure? = nil,
        metadata: [String: JSONValue]? = nil
    ) {
        self.id = id
        self.object = object
        self.createdAt = createdAt
        self.status = status
        self.model = model
        self.usage = usage
        self.output = output
        self.error = error
        self.metadata = metadata
    }
}

public struct APIPage<Element: Codable & Sendable & Equatable>: Codable, Sendable, Equatable {
    public let object: String
    public let data: [Element]
    public let firstId: String?
    public let lastId: String?
    public let hasMore: Bool

    public init(
        object: String = "list", data: [Element], firstId: String? = nil,
        lastId: String? = nil, hasMore: Bool = false
    ) {
        self.object = object
        self.data = data
        self.firstId = firstId
        self.lastId = lastId
        self.hasMore = hasMore
    }
}

public enum ListOrder: String, Codable, Sendable { case asc, desc }

public struct ListParameters: Sendable {
    public var limit: Int?
    public var after: String?
    public var before: String?
    public var order: ListOrder?

    public init(limit: Int? = nil, after: String? = nil, before: String? = nil, order: ListOrder? = nil) {
        self.limit = limit; self.after = after; self.before = before; self.order = order
    }

    internal func query() throws -> [(String, String)] {
        if let limit, !(1...100).contains(limit) { throw ResearchAPIError.invalidArgument }
        var result: [(String, String)] = []
        if let limit { result.append(("limit", String(limit))) }
        if let after { result.append(("after", after)) }
        if let before { result.append(("before", before)) }
        if let order { result.append(("order", order.rawValue)) }
        return result
    }
}

public struct ConversationsListParameters: Sendable {
    public var page: ListParameters
    public var projectId: String?
    public var unassignedProject: Bool?
    public var pinned: Bool?

    public init(
        page: ListParameters = .init(), projectId: String? = nil,
        unassignedProject: Bool? = nil, pinned: Bool? = nil
    ) {
        self.page = page; self.projectId = projectId
        self.unassignedProject = unassignedProject; self.pinned = pinned
    }
}
