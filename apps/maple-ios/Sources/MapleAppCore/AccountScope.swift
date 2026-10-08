import Foundation

/// A principal namespace plus a revocable app-session lease. Signing into the
/// same principal again creates a new generation; old work cannot adopt it.
public struct AccountScope: Codable, Hashable, Sendable {
    public let apiOrigin: String
    public let principalID: String
    public let generation: UUID

    public init(apiOrigin: String, principalID: String, generation: UUID = UUID()) {
        self.apiOrigin = apiOrigin
        self.principalID = principalID
        self.generation = generation
    }
}
