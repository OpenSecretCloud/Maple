import Foundation
import MapleSDK
import Observation

public struct SessionIdentity: Sendable, Equatable {
    public let origin: String
    public let principal: String
    public let generation: UUID
    public init(origin: String, principal: String, generation: UUID) {
        self.origin = origin; self.principal = principal; self.generation = generation
    }
}

public protocol SessionServing: Sendable {
    func identity() async throws -> SessionIdentity?
    func user() async throws -> MapleUser
    func signIn(email: String, password: String) async throws
    func signInGuest(id: String, password: String) async throws
    func signUp(email: String, password: String, name: String?) async throws
    func signOut() async throws
    func signOut(ifOwnedBy identity: SessionIdentity?) async throws
    func authorityChanges() async -> AsyncStream<CredentialAuthorityChange>?
    func authorityState() async -> CredentialAuthorityChange?
    func completePendingCleanup() async throws
}

extension SessionServing {
    public func signOut(ifOwnedBy identity: SessionIdentity?) async throws {
        guard let identity, try await self.identity() == identity else { return }
        try await signOut()
    }
    public func authorityChanges() async -> AsyncStream<CredentialAuthorityChange>? { nil }
    public func authorityState() async -> CredentialAuthorityChange? { nil }
    public func completePendingCleanup() async throws {}
}

public protocol AccountDataLifecycle: Sendable {
    func activate(_ scope: AccountScope) async throws
    func retire(_ scope: AccountScope) async throws
    func clearSignedOutData() async throws
}

extension ProtectedChatStore: AccountDataLifecycle {}

public struct AccountSession: Sendable, Equatable {
    public let scope: AccountScope
    public var user: MapleUser
}

public enum SessionPhase: Sendable { case restoring, signedOut, authenticating, signedIn, unavailable, cleaningUp }
public enum SessionIssue: Sendable, Equatable {
    case connection, credentials, emailAlreadyRegistered, invalidInput, storage, cleanup
    case clock(AttestationClockIssue)
}

/// The account lease is revoked synchronously before any asynchronous teardown.
/// A network failure preserves credentials and protected drafts for an explicit
/// retry; only confirmed absence of credential authority enters signed-out state.
@MainActor @Observable public final class SessionController {
    public private(set) var phase: SessionPhase = .restoring
    public private(set) var account: AccountSession?
    public private(set) var issue: SessionIssue?
    @ObservationIgnored private let service: any SessionServing
    @ObservationIgnored private let storage: any AccountDataLifecycle
    @ObservationIgnored private var operation = UUID()
    @ObservationIgnored private var authorityEvent = UUID()
    @ObservationIgnored private var cleanupScope: AccountScope?
    @ObservationIgnored private var accountIdentity: SessionIdentity?
    @ObservationIgnored private var resolvingIdentity: SessionIdentity?
    @ObservationIgnored private var cleanupIntent: CleanupIntent?
    @ObservationIgnored private var cleanupIdentity: SessionIdentity?
    @ObservationIgnored private var revocationCompleted = false
    @ObservationIgnored private var observation: Task<Void, Never>?
    private enum CleanupIntent { case signOut, replacement }
    @ObservationIgnored public var onRetire: (@MainActor (AccountScope) async throws -> Void)?
    @ObservationIgnored public var onCancelExternal: (@MainActor () async -> Void)?

    public init(service: any SessionServing, storage: any AccountDataLifecycle) {
        self.service = service; self.storage = storage
    }

    deinit { observation?.cancel() }

    /// One lifetime subscription. The task does not retain the controller while
    /// awaiting events, and cancelling it unregisters the SDK observer.
    public func startObservingAuthority() {
        guard observation == nil else { return }
        let service = service
        observation = Task { [weak self] in
            guard let changes = await service.authorityChanges() else { return }
            for await change in changes {
                guard !Task.isCancelled else { return }
                await self?.authorityChanged(change)
            }
        }
    }

    public func restore() async {
        startObservingAuthority()
        guard account == nil, phase != .authenticating, phase != .cleaningUp else { return }
        if let intent = cleanupIntent {
            await cleanUp(intent, retryCredentials: true)
            return
        }
        let ticket = UUID(); operation = ticket; phase = .restoring; issue = nil
        let state = await service.authorityState()
        guard owns(ticket) else { return }
        if state?.requiresCleanup == true {
            await cleanUp(.replacement, retryCredentials: true)
            return
        }
        await resolve(ticket)
    }

    /// Delivery may be delayed or coalesced. Compare with the actual authority
    /// before retiring any app generation; an old event cannot sign out a new one.
    func authorityChanged(_ change: CredentialAuthorityChange) async {
        authorityEvent = UUID()
        guard phase != .cleaningUp, cleanupIntent == nil else { return }
        let ticket = operation
        guard await service.authorityState() == change, owns(ticket), phase != .cleaningUp else { return }
        let identity = accountIdentity ?? resolvingIdentity
        if let identity, identity.origin == change.apiOrigin,
            identity.generation != change.generation || change.requiresCleanup
        {
            await cleanUp(.replacement, retryCredentials: false)
        } else if identity == nil, change.requiresCleanup {
            await cleanUp(.replacement, retryCredentials: false)
        } else if phase == .signedOut {
            // For example, another client sharing the authority completed auth.
            do {
                guard try await service.identity() != nil, owns(ticket), phase == .signedOut else { return }
                operation = UUID(); phase = .restoring
                await resolve(operation)
            } catch { /* A durable failure is handled by its authority event. */  }
        }
    }

    public func signIn(email: String, password: String) async {
        await authenticate { try await self.service.signIn(email: email, password: password) }
    }

    public func signInGuest(id: String, password: String) async {
        await authenticate { try await self.service.signInGuest(id: id, password: password) }
    }

    public func signUp(email: String, password: String, name: String?) async {
        await authenticate { try await self.service.signUp(email: email, password: password, name: name) }
    }

    public func beginExternalAuthentication() -> UUID? {
        guard phase == .signedOut, cleanupScope == nil, cleanupIntent == nil else { return nil }
        let ticket = UUID(); operation = ticket; phase = .authenticating; issue = nil
        return ticket
    }

    public func finishExternalAuthentication(_ ticket: UUID) async {
        guard owns(ticket), phase == .authenticating else { return }
        await resolve(ticket)
    }

    /// The ceremony cancels its provider task and SDK reservation before this
    /// check. If installation already committed, resolve its owned authority;
    /// never present sign-in while hidden credentials remain installed.
    public func cancelExternalAuthentication(_ ticket: UUID) async {
        guard owns(ticket), phase == .authenticating else { return }
        await onCancelExternal?()
        guard owns(ticket) else { return }
        await resolve(ticket)
    }

    private func authenticate(_ action: () async throws -> Void) async {
        guard phase == .signedOut, cleanupScope == nil, cleanupIntent == nil else { return }
        let ticket = UUID(); operation = ticket; phase = .authenticating; issue = nil
        do {
            try await action()
            guard owns(ticket) else { return }
            await resolve(ticket)
        } catch {
            guard owns(ticket) else { return }
            // A request may have installed authority before a later step failed.
            // Never enable a second sign-in without checking that authority.
            do {
                let current = try await service.identity()
                guard owns(ticket) else { return }
                phase = current == nil ? .signedOut : .unavailable
                issue = Self.present(error)
            } catch {
                guard owns(ticket) else { return }
                phase = .unavailable; issue = .storage
            }
        }
    }

    private func resolve(_ ticket: UUID) async {
        do {
            guard let identity = try await service.identity() else {
                guard owns(ticket) else { return }
                try await storage.clearSignedOutData()
                guard owns(ticket) else { return }
                // An install may have happened while protected cleanup awaited.
                // Its stream event can already have been consumed during restore.
                let observed = authorityEvent
                let current = try await service.identity()
                guard owns(ticket) else { return }
                if current != nil || authorityEvent != observed { await resolve(ticket); return }
                resolvingIdentity = nil; phase = .signedOut; issue = nil
                return
            }
            guard owns(ticket) else { return }
            resolvingIdentity = identity
            let user = try await service.user()
            guard owns(ticket) else { return }
            let confirmed = try await service.identity()
            guard owns(ticket) else { return }
            guard confirmed == identity else {
                await cleanUp(.replacement, retryCredentials: false); return
            }
            guard user.id == identity.principal else {
                resolvingIdentity = nil; phase = .unavailable; issue = .credentials; return
            }
            let scope = AccountScope(apiOrigin: identity.origin, principalID: identity.principal)
            cleanupScope = scope
            try await storage.activate(scope)
            guard owns(ticket) else {
                try? await storage.retire(scope)
                return
            }
            guard try await service.identity() == identity, owns(ticket) else {
                if owns(ticket) { await cleanUp(.replacement, retryCredentials: false) }
                return
            }
            cleanupScope = nil; resolvingIdentity = nil
            account = AccountSession(scope: scope, user: user)
            accountIdentity = identity
            phase = .signedIn; issue = nil
        } catch {
            guard owns(ticket) else { return }
            if cleanupScope != nil { cleanupIntent = .replacement }
            phase = .unavailable
            issue = error is ChatStorageError ? .storage : Self.present(error)
        }
    }

    public func signOut() async { await cleanUp(.signOut, retryCredentials: true) }

    private func cleanUp(_ intent: CleanupIntent, retryCredentials: Bool) async {
        guard phase != .cleaningUp else { return }
        let ticket = UUID(); operation = ticket
        let scope = account?.scope ?? cleanupScope
        let priorIntent = cleanupIntent
        if intent == .signOut, priorIntent != .signOut {
            cleanupIdentity = accountIdentity ?? resolvingIdentity
            revocationCompleted = false
        }
        account = nil; accountIdentity = nil; resolvingIdentity = nil
        cleanupScope = scope; cleanupIntent = intent; phase = .cleaningUp; issue = nil
        var failed = false
        if intent == .signOut, priorIntent != .signOut, cleanupIdentity == nil {
            do { cleanupIdentity = try await service.identity() } catch { failed = true }
        }
        guard owns(ticket) else { return }
        await onCancelExternal?()
        if let scope {
            do { try await onRetire?(scope) } catch { failed = true }
        }
        guard owns(ticket) else { return }
        // A replaced SDK authority belongs to its new owner. Automatic retirement
        // cleans only app-owned work; it never signs that replacement account out.
        do {
            if retryCredentials { try await service.completePendingCleanup() }
            if intent == .signOut, !revocationCompleted {
                try await service.signOut(ifOwnedBy: cleanupIdentity)
                revocationCompleted = true
            }
            if await service.authorityState()?.requiresCleanup == true { failed = true }
        } catch { failed = true }
        if let scope {
            do { try await storage.retire(scope) } catch { failed = true }
        }
        do { try await storage.clearSignedOutData() } catch { failed = true }
        guard owns(ticket) else { return }
        if failed {
            phase = .unavailable; issue = .cleanup
        } else {
            cleanupScope = nil; cleanupIntent = nil; cleanupIdentity = nil; revocationCompleted = false
            // Resolve the actual current authority after all cleanup awaits. A
            // replacement installed during retirement must not inherit old data.
            phase = .restoring
            await resolve(ticket)
        }
    }

    /// Refresh verification/profile data without changing the application's
    /// account generation or taking ownership of another sign-in's result.
    public func refreshUser() async {
        guard phase == .signedIn, let owned = account, let identity = accountIdentity else { return }
        do {
            guard try await service.identity() == identity, account?.scope == owned.scope else { return }
            let user = try await service.user()
            guard try await service.identity() == identity, account?.scope == owned.scope,
                user.id == owned.scope.principalID
            else { return }
            account = AccountSession(scope: owned.scope, user: user)
        } catch {
            // Profile refresh must not revoke authority or discard offline work.
        }
    }

    private func owns(_ ticket: UUID) -> Bool { operation == ticket }

    private static func present(_ error: Error) -> SessionIssue {
        if let clock = AttestationClockIssue.classify(error) { return .clock(clock) }
        if error is ChatStorageError { return .storage }
        if let error = error as? ResearchAPIError {
            switch error {
            case .invalidArgument: return .invalidInput
            case .http(_, .invalidLogin): return .credentials
            case .http(_, .emailAlreadyRegistered): return .emailAlreadyRegistered
            default: break
            }
        }
        return .connection
    }
}
