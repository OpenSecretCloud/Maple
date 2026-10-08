import Foundation
import Observation
import Synchronization
import Testing
@testable import MapleAppCore

private final class ListeningObservationChanges: Sendable {
    private let count = Mutex(0)
    var value: Int { count.withLock { $0 } }
    func record() { count.withLock { $0 += 1 } }
}

private actor FailingPresentationPreparer: SpeechPreparing {
    func prepare(_ reply: SpeechReply, voice: MapleVoice, owner: SpeechOwner) throws -> PreparedSpeech {
        throw SpeechIssue.invalidAudio
    }
    func discard(_ speech: PreparedSpeech) {}
    func retire(_ owner: SpeechOwner) {}
}

@MainActor struct ListeningObservationTests {
    @Test func closedPlayerObservesPresentationBeforeItReadsPlaybackState() async throws {
        let (session, _, _) = listeningFixture()
        let opening = ListeningObservationChanges()
        let initiallyPresented = withObservationTracking {
            // The closed SwiftUI branch cannot observe phase or current yet.
            session.isPresented
        } onChange: {
            opening.record()
        }
        #expect(!initiallyPresented)
        session.playReply(speechReply("manual"))
        #expect(opening.value == 1)
        #expect(session.isPresented && session.phase == .preparing)
        await session.waitForPreparation()
        #expect(session.phase == .playing)

        let closing = ListeningObservationChanges()
        let presented = withObservationTracking {
            session.isPresented
        } onChange: {
            closing.record()
        }
        #expect(presented)
        session.close()
        #expect(closing.value == 1)
        #expect(!session.isPresented && session.phase == .closed)
        try await session.waitForCleanup()
    }

    @Test func preparationFailureStillPublishesAnActionablePlayer() async throws {
        let session = ReplyListeningSession(
            scope: accountScope(), preparer: FailingPresentationPreparer(), player: ListeningPlayer())
        let changes = ListeningObservationChanges()
        _ = withObservationTracking {
            session.isPresented
        } onChange: {
            changes.record()
        }
        session.playReply(speechReply("invalid-audio"))
        await session.waitForPreparation()
        #expect(changes.value == 1)
        #expect(session.isPresented)
        #expect(session.phase == .failed(.invalidAudio) && session.issue == .invalidAudio)
        #expect(session.current?.responseID == "invalid-audio")
        session.close()
        try await session.waitForCleanup()
    }
}
