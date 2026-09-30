import Foundation

public struct PreparedNativeOAuthHandoff: Sendable {
    let id: UUID
    public let sessionID: String
    public let requestID: String
    public let expiresAt: Date
}

/// Show this account for explicit confirmation before installing its credentials.
public struct NativeSignInConfirmation: Sendable {
    let id: UUID
    public let principalID: String
    public let email: String?
}
