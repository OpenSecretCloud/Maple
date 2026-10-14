import MapleSDK
import Testing
@testable import MapleAppUI

@Suite struct ChatTranscriptHistoryAnchorTests {
    @Test(arguments: [
        (call: "function_call", output: "function_call_output"),
        (call: "tool_call", output: "tool_output"),
    ])
    func anOutputAnchorSurvivesTheEarlierPageRevealingItsCall(types: (call: String, output: String)) throws {
        let call = ConversationItem(id: "call", type: types.call, status: "completed", callId: "search")
        let output = ConversationItem(
            id: "output", type: types.output, status: "completed", callId: "search",
            output: .string("Synthetic search result"))
        let reply = ConversationItem(id: "reply", type: "message", status: "completed", role: "assistant")
        let before = try #require(ChatTranscriptRow.rows([output, reply]).first)
        #expect(before.id == output.id && before.toolResult == nil)
        #expect(before.historyAnchorIDs.contains(output.id))

        let after = ChatTranscriptRow.rows([call, output, reply])
        let restored = try #require(after.first { $0.historyAnchorIDs.contains(output.id) })
        #expect(restored.id == call.id && restored.toolResult == output)
        #expect(restored.historyAnchorIDs.contains(call.id))
        #expect(after.filter { $0.historyAnchorIDs.contains(output.id) }.count == 1)
        #expect(after.map(\.id) == [call.id, reply.id])
    }

    @Test func existingReasoningAndAnswerAnchorsSurviveAPageStartingMidTurn() throws {
        let user = ConversationItem(id: "user", type: "message", status: "completed", role: "user")
        let reasoning = ConversationItem(id: "reasoning", type: "reasoning", status: "completed")
        let answer = ConversationItem(id: "answer", type: "message", status: "completed", role: "assistant")
        let before = ChatTranscriptRow.rows([reasoning, answer])
        let after = ChatTranscriptRow.rows([user, reasoning, answer])

        // The older page supplies the user that owns these already visible
        // items. Restoration must still resolve each persisted item directly.
        for row in before {
            let restored = try #require(after.first { $0.historyAnchorIDs.contains(row.item.id) })
            #expect(restored.id == row.id && restored.item == row.item)
            #expect(restored.historyAnchorIDs == row.historyAnchorIDs)
        }
        #expect(!after[0].historyAnchorIDs.contains(reasoning.id))
        #expect(!after[0].historyAnchorIDs.contains(answer.id))
    }
}
