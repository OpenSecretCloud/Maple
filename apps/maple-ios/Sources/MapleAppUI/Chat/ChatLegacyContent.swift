import Foundation

/// Presentation-only compatibility with Research's stored document and think
/// tags. Invalid/incomplete documents keep their original selectable text.
enum ChatLegacyContent {
    struct Source: Equatable, Sendable {
        let text: String
        let user: Bool
        let assistant: Bool
        let streaming: Bool
        let generation: UUID

        var requiresPreparation: Bool { assistant || (user && text.contains("{\"document\":")) }
    }

    struct Prepared: Equatable, Sendable {
        let source: Source
        let document: Document?
        let parts: [Part]
    }

    enum Presentation: Equatable, Sendable {
        case ordinary, document(Document), assistant([Part]), pendingDocument, pendingThinking
    }

    struct Document: Equatable, Sendable {
        let filename: String
        let text: String
        let before: String
        let after: String
    }

    struct Part: Equatable, Identifiable, Sendable {
        enum Kind: Equatable, Sendable { case text, thinking }
        let id: Int
        let kind: Kind
        let text: String
        let isThinking: Bool
    }

    static func prepare(_ source: Source) -> Prepared {
        Prepared(
            source: source, document: source.user ? document(in: source.text) : nil,
            parts: source.assistant ? assistantParts(source.text, streaming: source.streaming) : [])
    }

    /// Pending compatibility work must not expose document JSON or opening
    /// think tags as ordinary text. Streaming appends may keep their last rich
    /// presentation, but only an exact source can publish preparation receipts.
    static func presentation(source: Source, prepared: Prepared?) -> Presentation {
        guard source.requiresPreparation else { return .ordinary }
        if let prepared, prepared.source == source {
            if source.assistant { return .assistant(prepared.parts) }
            return prepared.document.map(Presentation.document) ?? .ordinary
        }
        if source.assistant {
            if let prepared, prepared.source.assistant, prepared.source.streaming, source.streaming,
                prepared.source.generation == source.generation, source.text.hasPrefix(prepared.source.text)
            {
                return .assistant(prepared.parts)
            }
            return .pendingThinking
        }
        return .pendingDocument
    }

    static func document(in text: String) -> Document? {
        guard let start = text.range(of: "{\"document\":"), text.utf8.count <= 1_048_576 else { return nil }
        var depth = 0, inString = false, escaped = false
        let scalars = text.unicodeScalars
        for index in scalars[start.lowerBound...].indices {
            let character = scalars[index]
            if escaped { escaped = false; continue }
            if character == "\\" { escaped = true; continue }
            if character == "\"" { inString.toggle(); continue }
            guard !inString else { continue }
            if character == "{" { depth += 1 }
            if character == "}" {
                depth -= 1
                guard depth == 0 else { continue }
                let end = scalars.index(after: index)
                struct Payload: Decodable {
                    struct Document: Decodable {
                        let filename: String
                        let textContent: String
                        enum CodingKeys: String, CodingKey { case filename, textContent = "text_content" }
                    }
                    let document: Document
                }
                guard
                    let value = try? JSONDecoder().decode(
                        Payload.self, from: Data(text[start.lowerBound..<end].utf8))
                else { return nil }
                return Document(
                    filename: value.document.filename, text: value.document.textContent,
                    before: String(text[..<start.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines),
                    after: String(text[end...]).trimmingCharacters(in: .whitespacesAndNewlines))
            }
        }
        return nil
    }

    static func assistantParts(_ text: String, streaming: Bool) -> [Part] {
        // Research recovers a completed response whose standalone opening tag
        // never closed by showing the following answer as ordinary content.
        if !streaming, !text.contains("</think>"),
            let emptyOpening = try? NSRegularExpression(pattern: "^<think>\\s*$", options: .anchorsMatchLines),
            emptyOpening.firstMatch(in: text, range: NSRange(location: 0, length: (text as NSString).length)) != nil,
            let stripping = try? NSRegularExpression(pattern: "^<think>\\s*", options: .anchorsMatchLines)
        {
            guard
                let match = stripping.firstMatch(
                    in: text,
                    range: NSRange(location: 0, length: (text as NSString).length)),
                let range = Range(match.range, in: text)
            else { return [] }
            var value = text
            value.removeSubrange(range)
            return value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? [] : [Part(id: 0, kind: .text, text: value, isThinking: false)]
        }
        let source = text as NSString
        guard let pattern = try? NSRegularExpression(pattern: "<think>([\\s\\S]*?)(?:</think>|$)") else { return [] }
        var parts: [Part] = [], previousEnd = 0
        let matches = pattern.matches(in: text, range: NSRange(location: 0, length: source.length))
        func appendText(_ range: NSRange) {
            let value = source.substring(with: range)
            guard !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
            parts.append(Part(id: range.location, kind: .text, text: value, isThinking: false))
        }
        for match in matches {
            if match.range.location > previousEnd {
                appendText(NSRange(location: previousEnd, length: match.range.location - previousEnd))
            }
            let value = source.substring(with: match.range(at: 1))
            if streaming || !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let closed = source.substring(with: match.range).hasSuffix("</think>")
                parts.append(
                    Part(id: match.range.location, kind: .thinking, text: value, isThinking: streaming && !closed))
            }
            previousEnd = NSMaxRange(match.range)
        }
        if previousEnd < source.length {
            appendText(NSRange(location: previousEnd, length: source.length - previousEnd))
        }
        return parts
    }
}
