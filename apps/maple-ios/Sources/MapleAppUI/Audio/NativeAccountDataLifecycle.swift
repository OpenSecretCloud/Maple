#if os(iOS)
    import MapleAppCore

    /// Startup and sign-out clear generated audio before a new account can use the
    /// app. Chat persistence remains independently protected by its own actor.
    actor NativeAccountDataLifecycle: AccountDataLifecycle {
        let drafts: ProtectedChatStore
        let speech: TemporarySpeechStore
        init(drafts: ProtectedChatStore, speech: TemporarySpeechStore) {
            self.drafts = drafts; self.speech = speech
        }
        func activate(_ scope: AccountScope) async throws {
            try await speech.startupCleanup()
            try await drafts.activate(scope)
        }
        func retire(_ scope: AccountScope) async throws { try await drafts.retire(scope) }
        func clearSignedOutData() async throws {
            var failed = false
            do { try await drafts.clearSignedOutData() } catch { failed = true }
            do { try await speech.startupCleanup() } catch { failed = true }
            if failed { throw ChatStorageError.cleanupRequired }
        }
    }
#endif
