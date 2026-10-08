import Foundation

/// Holds at most one parsed account route while cold restoration is unresolved.
/// OAuth grants and URL-supplied reset secrets never enter this memory-only inbox.
@MainActor public final class AccountLinkInbox {
    private struct Pending {
        let link: IncomingAccountLink
        let scope: AccountScope?
        let deadline: TimeInterval
    }
    private var pending: Pending?
    private let monotonicNow: @Sendable () -> TimeInterval

    public init(monotonicNow: @escaping @Sendable () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.monotonicNow = monotonicNow
    }

    /// A newer valid route supersedes the prior one. Callers parse exact origins
    /// before offering, so invalid input cannot erase a waiting valid route.
    public func offer(_ link: IncomingAccountLink, phase: SessionPhase, scope: AccountScope?) -> IncomingAccountLink? {
        if (phase == .signedIn && scope != nil) || (phase == .signedOut && scope == nil) {
            pending = nil; return link
        }
        switch phase {
        case .restoring:
            pending = Pending(link: link, scope: scope, deadline: monotonicNow() + 300)
        case .authenticating, .cleaningUp:
            retire()
        default: break
        }
        return nil
    }

    /// A cold, unbound route can use the first resolved account state. A route
    /// captured under a known account cannot follow that account's replacement.
    public func take(phase: SessionPhase, scope: AccountScope?) -> IncomingAccountLink? {
        if phase == .authenticating || phase == .cleaningUp { retire(); return nil }
        guard let pending else { return nil }
        guard pending.deadline > monotonicNow() else { self.pending = nil; return nil }
        guard (phase == .signedIn && scope != nil) || (phase == .signedOut && scope == nil) else { return nil }
        self.pending = nil
        guard pending.scope == nil || pending.scope == scope else { return nil }
        return pending.link
    }

    public func retire() { pending = nil }
}
