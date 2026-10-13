import Foundation

public enum AccountServiceError: Error, Equatable, Sendable {
    case sessionChanged, cancelled, unauthorized, unavailable, invalidResponse
}

public enum AccountLoadState<Value: Equatable & Sendable>: Equatable, Sendable {
    case unknown, loading, loaded(Value), failed(AccountServiceError)

    public var value: Value? {
        guard case let .loaded(value) = self else { return nil }
        return value
    }
}

/// Server renewal dates may be epoch seconds or an ISO 8601 string. Unknown
/// string formats remain unreadable; they are never interpreted as local dates.
public enum BillingDate: Decodable, Equatable, Sendable {
    case seconds(Double), text(String)

    public init(from decoder: any Decoder) throws {
        let value = try decoder.singleValueContainer()
        if let seconds = try? value.decode(Double.self), seconds.isFinite {
            self = .seconds(seconds)
        } else {
            self = .text(try value.decode(String.self))
        }
    }

    public var date: Date? {
        switch self {
        case let .seconds(value):
            guard value.isFinite, (0...253_402_300_799).contains(value) else { return nil }
            return Date(timeIntervalSince1970: value)
        case let .text(value):
            // Deliberately do not treat numeric strings as epoch seconds.
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            return formatter.date(from: value) ?? ISO8601DateFormatter().date(from: value)
        }
    }
}

public struct BillingSubscription: Decodable, Equatable, Sendable {
    public let provider: String
    public let plan: String
    public let productId: String?
    public let state: String
    public let renewsAt: Double?
    /// Inert server data. It must never be interpreted as a URL or route.
    public let manage: String
    public let selected: Bool?
    public let environment: String?
    public let autoRenewEnabled: Bool?
}

public enum BillingPlan: Equatable, Sendable {
    public enum Paid: Equatable, Sendable { case pro, max, team }
    case unknown, free, paid(Paid)

    public var isPaid: Bool {
        if case .paid = self { return true }
        return false
    }
}

public struct BillingConflict: Decodable, Equatable, Sendable {
    public let otherProvider: String
    /// Inert server data. It must never be interpreted as a route or action.
    public let action: String
}

public struct BillingPendingPlanChange: Decodable, Equatable, Sendable {
    public let id: String
    public let type: String
    public let targetPlanName: String
    public let status: String
    public let expiresAt: String
}

public struct BillingStatus: Decodable, Equatable, Sendable {
    public let isSubscribed: Bool
    public let stripeCustomerId: String?
    public let productId: String
    public let productName: String?
    public let subscriptionStatus: String?
    public let currentPeriodEnd: BillingDate?
    public let canChat: Bool
    public let chatsRemaining: Int?
    public let paymentProvider: String?
    public let totalTokens: Int?
    public let usedTokens: Int?
    public let usageResetDate: String?
    public let apiCreditBalance: Double?
    public let iosIapEnabled: Bool?
    public let iosUsExternalLinkEnabled: Bool?
    public let subscriptions: [BillingSubscription]?
    public let conflict: BillingConflict?
    public let pendingPlanChange: BillingPendingPlanChange?

    public var plan: BillingPlan {
        guard let name = productName?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() else {
            return .unknown
        }
        if name == "free" { return .free }
        if name.contains("team") { return .paid(.team) }
        if name.contains("max") { return .paid(.max) }
        if name.contains("pro") { return .paid(.pro) }
        return .unknown
    }

    func validate() throws {
        guard [chatsRemaining, totalTokens, usedTokens].allSatisfy({ $0.map { $0 >= 0 } ?? true }),
            subscriptions?.allSatisfy({ $0.renewsAt.map { $0.isFinite && $0 >= 0 } ?? true }) ?? true
        else { throw AccountServiceError.invalidResponse }
    }
}

public struct TeamSeatMismatch: Equatable, Sendable {
    public let memberCount: Int?
    public let billedSeatCount: Int?
    public var hasExactCounts: Bool { memberCount != nil && billedSeatCount != nil }
}

public struct TeamStatus: Decodable, Equatable, Sendable {
    public let hasTeamSubscription: Bool
    public let teamCreated: Bool
    public let role: String?
    public let isTeamAdmin: Bool?
    public let seatsPurchased: Int?
    public let seatsUsed: Int?
    public let seatsAvailable: Int?
    public let membersCount: Int?
    public let billedSeatCount: Int?
    public let teamMemberCount: Int?
    public let seatLimitExceeded: Bool?

    public var memberCount: Int? { teamMemberCount ?? membersCount ?? seatsUsed }
    public var paidSeatCount: Int? { billedSeatCount ?? seatsPurchased }
    public var availableSeatCount: Int? { seatsAvailable.map { max(0, $0) } }
    public var seatMismatch: TeamSeatMismatch? {
        guard teamCreated else { return nil }
        let exactMismatch = memberCount.flatMap { members in paidSeatCount.map { members > $0 } } ?? false
        guard exactMismatch || seatLimitExceeded == true else { return nil }
        return TeamSeatMismatch(memberCount: memberCount, billedSeatCount: paidSeatCount)
    }

    func validate() throws {
        guard
            [seatsPurchased, seatsUsed, membersCount, billedSeatCount, teamMemberCount]
                .allSatisfy({ $0.map { $0 >= 0 } ?? true })
        else { throw AccountServiceError.invalidResponse }
    }
}

public protocol BillingServing: Sendable {
    func subscriptionStatus() async throws -> BillingStatus
    func teamStatus() async throws -> TeamStatus
    func retire() async
}

public protocol BillingTokenSource: Sendable {
    func mintToken(audience: String) async throws -> String
}

public struct BillingHTTPResponse: Sendable {
    public let status: Int
    public let body: Data
    public init(status: Int, body: Data) { self.status = status; self.body = body }
}

public protocol BillingHTTP: Sendable {
    func get(url: URL, bearerToken: String) async throws -> BillingHTTPResponse
}
