import Foundation
import MapleSDK

enum ChatRequestBudget {
    enum Assessment { case fits, exceeds, needsEncoding }
    static let maximum = 50 * 1_024 * 1_024

    static func assess(_ draft: ChatDraft, options: ChatSendOptions, conversationID: String?) -> Assessment {
        let text = draft.text.utf8.count + options.model.utf8.count + (conversationID?.utf8.count ?? 128)
        guard text <= maximum else { return .exceeds }
        var base64 = 0
        for image in draft.images {
            guard image.data.count <= maximum else { return .exceeds }
            base64 += 4 * ((image.data.count + 2) / 3)
        }
        guard text + base64 <= maximum else { return .exceeds }
        // Every JSON string source byte is at most six escaped bytes. Each
        // base64 slash may add one byte. Ambiguity is resolved off the UI actor.
        return 64 * 1_024 + 6 * text + 2 * base64 <= maximum ? .fits : .needsEncoding
    }

    static func exactFits(_ draft: ChatDraft, options: ChatSendOptions, conversationID: String?) async throws -> Bool {
        let task = Task.detached {
            try Task.checkCancellation()
            var content: [ResponseInputContent] = []
            if !draft.text.isEmpty { content.append(.text(draft.text)) }
            for image in draft.images {
                try Task.checkCancellation()
                content.append(.image(url: "data:\(image.contentType);base64," + image.data.base64EncodedString()))
            }
            let request = ResponsesCreateRequest(
                model: options.model, input: .messages([ResponseInputMessage(content: content)]),
                conversation: conversationID ?? String(repeating: "x", count: 128),
                metadata: ["internal_message_id": .string("00000000-0000-0000-0000-000000000000")],
                webSearch: options.webSearch, reasoningEffort: options.reasoningEffort)
            let encoder = JSONEncoder()
            // Matches SDK APIJSON.encode, including its default slash escaping.
            encoder.keyEncodingStrategy = .convertToSnakeCase
            let count = try encoder.encode(request).count
            try Task.checkCancellation()
            return count <= maximum
        }
        return try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }
}
