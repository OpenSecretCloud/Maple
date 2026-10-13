import Foundation
import MapleAppCore
import MapleSDK
import Testing
@testable import MapleAppUI

@Suite struct ComposerFlowTests {
    @Test func nativeTextHeightIncludesPaddingAndPreservesAccessibilityLineHeight() {
        #expect(ComposerEditorMetrics.preferredHeight(lineHeight: 22, contentHeight: 34, compact: false) == 44)
        #expect(ComposerEditorMetrics.preferredHeight(lineHeight: 22, contentHeight: 1_000, compact: false) == 188)
        #expect(ComposerEditorMetrics.preferredHeight(lineHeight: 52, contentHeight: 0, compact: true) == 64)
        #expect(ComposerEditorMetrics.preferredHeight(lineHeight: 52, contentHeight: 0, compact: false) == 64)
        #expect(ComposerEditorMetrics.topInset(height: 44, lineHeight: 22, compact: true) == 11)
        #expect(ComposerEditorMetrics.topInset(height: 64, lineHeight: 52, compact: true) == 6)
        #expect(ComposerEditorMetrics.topInset(height: 110, lineHeight: 22, compact: false) == 6)
    }

    @Test func optionsAndMicrophoneKeepFullTargetsInCompactAndNarrowComposers() {
        let sizes = [44, 180, 44, 44, 44].map { CGSize(width: CGFloat($0), height: 44) }
        for width in [CGFloat(180), 200, 320, 350, 850] {
            for expanded in [false, true] {
                let plan = ComposerInputPlan(
                    sizes: sizes, editorIndex: 1, width: width, expanded: expanded, trailingIndices: [3, 4])
                let frames = plan.frames.compactMap { $0 }
                #expect(frames.count == sizes.count)
                #expect(plan.frames[2]?.width == 44 && plan.frames[3]?.width == 44)
                #expect(plan.frames[3]?.minY == plan.frames[4]?.minY)
                #expect(plan.frames[4]?.maxX == width)
                for (index, frame) in frames.enumerated() {
                    #expect(frame.width >= 44 && frame.height >= 44)
                    #expect(frame.minX >= 0 && frame.maxX <= width)
                    for next in frames.dropFirst(index + 1) { #expect(!frame.intersects(next)) }
                }
            }
        }
    }

    @Test func compactActionsWrapWithoutOverlapOrDroppingIdentity() {
        let widths: [CGFloat] = [44, 128, 44, 44, 44, 82, 44]
        let sizes = widths.map { CGSize(width: $0, height: 44) }
        let plan = ComposerFlowPlan(sizes: sizes, width: 350)
        #expect(plan.frames.count == sizes.count)
        #expect(plan.frames.compactMap { $0?.minY } == [0, 0, 0, 0, 0, 52, 52])
        #expect(plan.height == 96)
        let frames = plan.frames.compactMap { $0 }
        for (index, frame) in frames.enumerated() {
            #expect(frame.minX >= 0 && frame.maxX <= 350)
            for next in frames.dropFirst(index + 1) { #expect(!frame.intersects(next)) }
        }
    }

    @Test func accessibilityHeightAndEmptyConditionalDoNotCreatePhantomRows() {
        let plan = ComposerFlowPlan(
            sizes: [.init(width: 44, height: 44), .init(width: 300, height: 112), .zero, .init(width: 44, height: 44)],
            width: 300)
        #expect(plan.frames[0]?.origin == .zero)
        #expect(plan.frames[1]?.minY == 52)
        #expect(plan.frames[2] == nil)
        #expect(plan.frames[3]?.minY == 172)
        #expect(plan.height == 216)
        #expect(ComposerFlowPlan(sizes: [.zero], width: 300).height == 0)
    }

    @Test func exactFitAndWideTabletKeepOneRow() {
        let sizes = [CGSize(width: 44, height: 44), CGSize(width: 44, height: 44)]
        #expect(ComposerFlowPlan(sizes: sizes, width: 96).height == 44)
        #expect(ComposerFlowPlan(sizes: sizes, width: 95).height == 96)
        #expect(ComposerFlowPlan(sizes: Array(repeating: sizes[0], count: 7), width: 850).height == 44)
    }

    @Test func sendUsesTrailingRowSpaceWithoutMovingOrOverlappingOtherControls() {
        let sizes = [CGSize(width: 44, height: 44), CGSize(width: 128, height: 44), CGSize(width: 44, height: 44)]
        let oneRow = ComposerFlowPlan(sizes: sizes, width: 350, trailingIndices: [2])
        #expect(oneRow.frames[0]?.minX == 0 && oneRow.frames[1]?.minX == 52)
        #expect(oneRow.frames[2]?.maxX == 350)
        let wrapped = ComposerFlowPlan(sizes: sizes, width: 200, trailingIndices: [2])
        #expect(wrapped.frames[2]?.minY == 52 && wrapped.frames[2]?.maxX == 200)
        #expect(wrapped.height == 96)
    }

    @Test func collapsedPillKeepsEditorBetweenAttachmentAndTrailingActions() {
        let sizes = [44, 180, 80, 44, 44, 44].map { CGSize(width: CGFloat($0), height: 44) }
        let plan = ComposerInputPlan(
            sizes: sizes, editorIndex: 1, width: 350, expanded: false,
            hiddenIndices: [2, 3], trailingIndices: [3, 4, 5])
        #expect(plan.height == 44)
        #expect(plan.frames[2] == nil && plan.frames[3] == nil)
        #expect(plan.frames[0]?.minX == 0)
        #expect(plan.frames[1]?.minX == 48 && plan.frames[1]?.width == 206)
        #expect(plan.frames[4]?.minX == 258 && plan.frames[5]?.maxX == 350)
        let frames = plan.frames.compactMap { $0 }
        for (index, frame) in frames.enumerated() {
            #expect(frame.minX >= 0 && frame.maxX <= 350)
            for next in frames.dropFirst(index + 1) { #expect(!frame.intersects(next)) }
        }
    }

    @Test func expandedEditorKeepsProjectMicrophoneAndActionTogetherAtTrailingEdge() {
        var sizes = [44, 180, 80, 44, 44, 44].map { CGSize(width: CGFloat($0), height: 44) }
        sizes[1].height = 110
        let plan = ComposerInputPlan(
            sizes: sizes, editorIndex: 1, width: 350, expanded: true, trailingIndices: [3, 4, 5])
        #expect(plan.frames[1] == CGRect(x: 0, y: 0, width: 350, height: 110))
        #expect(plan.frames[3]?.minX == 210 && plan.frames[5]?.maxX == 350)
        #expect(plan.frames[0]?.minY == 114 && plan.height == 158)
        let narrow = ComposerInputPlan(
            sizes: sizes, editorIndex: 1, width: 200, expanded: true, trailingIndices: [3, 4, 5])
        #expect(narrow.frames[3]?.minY == narrow.frames[4]?.minY)
        #expect(narrow.frames[4]?.minY == narrow.frames[5]?.minY)
        #expect(narrow.frames[5]?.maxX == 200)
        #expect(narrow.height == 206)
    }

    @Test func narrowPillUsesRowsInsteadOfSqueezingNativeEditorOrDroppingActions() {
        let sizes = Array(repeating: CGSize(width: 44, height: 44), count: 4)
        let plan = ComposerInputPlan(
            sizes: sizes, editorIndex: 1, width: 180, expanded: false, trailingIndices: [2, 3])
        #expect(plan.frames[1] == CGRect(x: 0, y: 0, width: 180, height: 44))
        #expect(plan.height == 92)
        #expect(plan.frames.compactMap { $0 }.count == 4)
        for frame in plan.frames.compactMap({ $0 }) {
            #expect(frame.width >= 44 && frame.height >= 44)
            #expect(frame.minX >= 0 && frame.maxX <= 180)
        }
    }

    @Test func keyboardDismissalCollapsesDespiteRetainedHardwareFocus() {
        #expect(
            ChatComposerPresentation.expanded(
                newConversation: false, editingQueue: false, focused: true, keyboardVisible: nil))
        #expect(
            ChatComposerPresentation.expanded(
                newConversation: false, editingQueue: false, focused: true, keyboardVisible: true))
        #expect(
            !ChatComposerPresentation.expanded(
                newConversation: false, editingQueue: false, focused: true, keyboardVisible: false))
        #expect(
            !ChatComposerPresentation.expanded(
                newConversation: false, editingQueue: false, focused: false, keyboardVisible: true))
        #expect(
            ChatComposerPresentation.expanded(
                newConversation: true, editingQueue: false, focused: false, keyboardVisible: false))
        #expect(
            ChatComposerPresentation.expanded(
                newConversation: false, editingQueue: true, focused: false, keyboardVisible: false))
    }

    @Test func toolCallAndExactResultKeepOneStableRowAndFailureDetails() {
        let call = ConversationItem(
            id: "call", type: "function_call", callId: "same", name: "web_search", arguments: .string("query"))
        let result = ConversationItem(
            id: "result", type: "function_call_output", status: "failed", callId: "same",
            output: .string("failure details"))
        let reply = ConversationItem(id: "reply", type: "message", role: "assistant")
        let before = ChatTranscriptRow.rows([call, reply])
        let after = ChatTranscriptRow.rows([call, result, reply])
        #expect(before.map(\.id) == ["call", "reply"] && after.map(\.id) == before.map(\.id))
        #expect(after[0].item == call && after[0].toolResult == result)
        #expect(after[1].toolResult == nil)
    }

    @Test func ambiguousUnmatchedAndOutOfOrderToolResultsAreNeverHidden() {
        let call = ConversationItem(id: "call", type: "function_call", callId: "same")
        let first = ConversationItem(
            id: "first", type: "function_call_output", callId: "same", output: .string("first"))
        let second = ConversationItem(
            id: "second", type: "function_call_output", callId: "same", output: .string("second"))
        #expect(ChatTranscriptRow.rows([call, first, second]).map(\.id) == ["call", "first", "second"])
        #expect(ChatTranscriptRow.rows([first, call]).map(\.id) == ["first", "call"])
        #expect(ChatTranscriptRow.rows([first]).first?.toolResult == nil)
        let otherCall = ConversationItem(id: "other-call", type: "function_call", callId: "same")
        #expect(ChatTranscriptRow.rows([call, otherCall, first]).map(\.id) == ["call", "other-call", "first"])
        let emptyCall = ConversationItem(id: "empty-call", type: "function_call", callId: "")
        let emptyResult = ConversationItem(id: "empty-result", type: "function_call_output", callId: "")
        #expect(ChatTranscriptRow.rows([emptyCall, emptyResult]).count == 2)
    }

    @Test func decodedConversationAndRetrievedResponseToolRepresentationsKeepIdenticalPairedRows() throws {
        let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
        let canonical = #"""
            [
              {"id":"call","response_id":"response","type":"function_call","status":"completed",
               "call_id":"same","name":"open_urls","arguments":"{\"urls\":[\"https://example.com\"]}"},
              {"id":"result","response_id":"response","type":"function_call_output","status":"completed",
               "call_id":"same","output":"Synthetic opened page"},
              {"id":"web","type":"web_search_call","status":"completed","role":"assistant"}
            ]
            """#
        let retrieved = canonical.replacingOccurrences(of: "function_call_output", with: "tool_output")
            .replacingOccurrences(of: "function_call", with: "tool_call")
        let pageItems = try decoder.decode([ConversationItem].self, from: Data(canonical.utf8))
        let responseItems = try decoder.decode([ConversationItem].self, from: Data(retrieved.utf8))
        let pageRows = ChatTranscriptRow.rows(pageItems), responseRows = ChatTranscriptRow.rows(responseItems)
        #expect(pageRows.map(\.id) == ["call", "web"] && responseRows.map(\.id) == pageRows.map(\.id))
        #expect(responseRows[0].item.type == "tool_call" && responseRows[0].toolResult?.type == "tool_output")
        #expect(responseRows[0].item.callId == pageRows[0].item.callId)
        #expect(responseRows[0].item.arguments == pageRows[0].item.arguments)
        #expect(responseRows[0].toolResult?.output == pageRows[0].toolResult?.output)
        #expect(responseRows[0].item.responseId == "response" && responseRows[0].toolResult?.responseId == "response")
        #expect(responseItems.allSatisfy(ChatTranscriptRow.isTool))
        var assistantCall = responseItems[0]; assistantCall.role = "assistant"
        #expect(ChatTranscriptRow.isTool(assistantCall) && assistantCall.content == nil)

        // A duplicate call across the two representations is ambiguous. An
        // unmatched persisted output also keeps its own visible details row.
        #expect(ChatTranscriptRow.rows([pageItems[0], responseItems[0], responseItems[1]]).count == 3)
        let orphan = ChatTranscriptRow.rows([responseItems[1]])
        #expect(orphan.count == 1 && orphan[0].item.type == "tool_output" && orphan[0].toolResult == nil)
    }

    @Test func pendingIndicatorAppearsOnlyBeforeAssistantOutputDuringGeneration() {
        let user = ConversationItem(id: "user", type: "message", role: "user")
        let assistant = ConversationItem(id: "assistant", type: "message", status: "in_progress", role: "assistant")
        let reasoning = ConversationItem(id: "reasoning", type: "reasoning")
        let tool = ConversationItem(id: "tool", type: "web_search_call", status: "in_progress")
        #expect(ChatTranscriptPresentation.showsPending(phase: .sending, items: [user]))
        #expect(ChatTranscriptPresentation.showsPending(phase: .streaming, items: [user]))
        for phase: ChatRunPhase? in [nil, .preparing, .stopping, .recovering] {
            #expect(!ChatTranscriptPresentation.showsPending(phase: phase, items: [user]))
        }
        for output in [assistant, reasoning, tool] {
            #expect(!ChatTranscriptPresentation.showsPending(phase: .streaming, items: [user, output]))
        }
    }

    @Test func reasoningDurationMatchesResearchRoundingAndWhitespaceRules() {
        let english = Locale(identifier: "en")
        #expect(ChatTranscriptPresentation.thoughtDuration("") == 1)
        #expect(ChatTranscriptPresentation.thoughtDuration(" \n\t one word \n") == 1)
        #expect(
            ChatTranscriptPresentation.thoughtDuration(Array(repeating: "word", count: 38).joined(separator: " ")) == 1)
        #expect(
            ChatTranscriptPresentation.thoughtDuration(Array(repeating: "word", count: 39).joined(separator: "\t")) == 2
        )
        #expect(ChatTranscriptPresentation.thinkingLabel(text: "", streaming: true, locale: english) == "Thinking")
        #expect(
            ChatTranscriptPresentation.thinkingLabel(text: "one", streaming: false, locale: english)
                == "Thought for 1 seconds")
        #expect(
            ChatTranscriptPresentation.thinkingLabel(
                text: Array(repeating: "word", count: 52).joined(separator: " "), streaming: false, locale: english)
                == "Thought for 2 seconds")
    }

    @Test func serverLimitFeedbackDistinguishesKnownPlansAndMessageLimit() {
        #expect(
            ChatRecoveryPresentation.message(.usageLimit, plan: .free)
                == "You've reached your daily usage limit. Upgrade to Pro for more chats.")
        #expect(
            ChatRecoveryPresentation.message(.usageLimit, plan: .paid(.pro))
                == "You've reached your monthly Pro limit. Upgrade to Max for 10x more usage.")
        for plan: BillingPlan in [.unknown, .paid(.max), .paid(.team)] {
            #expect(
                ChatRecoveryPresentation.message(.usageLimit, plan: plan)
                    == "You've reached your monthly usage limit. Please wait for the next billing cycle.")
        }
        #expect(
            ChatRecoveryPresentation.message(.contextLimit) == "Your message exceeds the context limit for this model.")
        #expect(
            ChatRecoveryPresentation.message(.freeTierTokenLimit)
                == "This conversation is too long for the free tier. Upgrade to Pro for longer conversations.")
        #expect(
            ChatRecoveryPresentation.message(.modelUnavailable)
                == "This model or feature is not available on your current plan.")
        #expect(
            ChatRecoveryPresentation.contextLimitMessage
                == "Your message exceeds the context limit for the current model.\n\nHere's what you can try:\nShorten your message - Try reducing the amount of text or content"
        )
    }

}
