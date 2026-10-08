import Foundation

/// Research enables GFM task markers and remark-breaks, and disables only
/// indented code. Work on Foundation's parsed nodes so links, inline code, and
/// fenced code are not rewritten by a source-wide regular expression.
enum MessageMarkdownParity {
    static func normalize(_ parsed: AttributedString, source: String) throws -> AttributedString {
        var content = parsed
        // CommonMark advances source lines for CRLF, LF, and lone CR. Normalize
        // separators only in this lookup copy; original source bytes stay intact.
        let sourceLines = source.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
        // Reverse order leaves earlier source ranges valid as text lengths change.
        for run in Array(content.runs).reversed() {
            try Task.checkCancellation()
            guard var intent = run.presentationIntent,
                let codeIndex = intent.components.firstIndex(where: {
                    if case .codeBlock = $0.kind { return true }; return false
                }), case .codeBlock(let hint) = intent.components[codeIndex].kind,
                let position = run.markdownSourcePosition,
                let opening = openingLine(at: position, in: sourceLines)
            else { continue }
            let code = String(content[run.range].characters)
            guard !isFence(source: opening, code: code, hint: hint) else { continue }
            var prose = try AttributedString(
                markdown: code.hasSuffix("\n") ? String(code.dropLast()) : code,
                options: .init(
                    interpretedSyntax: .inlineOnlyPreservingWhitespace,
                    failurePolicy: .returnPartiallyParsedIfPossible))
            intent.components[codeIndex].kind = .paragraph
            prose.presentationIntent = intent
            content.replaceSubrange(run.range, with: prose)
        }

        // Only the first run in each real list-item paragraph can be a task marker.
        // Original source distinguishes an escaped \[ ] marker from task syntax.
        var firstParagraphs = Set<Int>()
        var taskRanges: [(Range<AttributedString.Index>, String, PresentationIntent)] = []
        for run in content.runs {
            guard let intent = run.presentationIntent,
                let paragraph = intent.components.first(where: {
                    if case .paragraph = $0.kind { return true }; return false
                }),
                intent.components.contains(where: {
                    if case .listItem = $0.kind { return true }; return false
                }), firstParagraphs.insert(paragraph.identity).inserted,
                run.inlinePresentationIntent?.contains(.code) != true,
                let position = run.markdownSourcePosition,
                let sourceRange = Range(position, in: source)
            else { continue }
            let original = source[sourceRange]
            let text = String(content[run.range].characters)
            let checked: Bool
            if taskPrefix(original) == false, taskPrefix(text) == false {
                checked = false
            } else if taskPrefix(original) == true, taskPrefix(text) == true {
                checked = true
            } else {
                continue
            }
            let end = content.characters.index(run.range.lowerBound, offsetBy: 3)
            taskRanges.append((run.range.lowerBound..<end, checked ? "☑" : "☐", intent))
        }
        for (range, marker, intent) in taskRanges.reversed() {
            var replacement = AttributedString(marker)
            replacement.presentationIntent = intent
            content.replaceSubrange(range, with: replacement)
        }

        for run in Array(content.runs).reversed() {
            guard var inline = run.inlinePresentationIntent, inline.contains(.softBreak) else { continue }
            // Foundation stores a soft break as a space. Both its character and
            // semantic intent must change for native layout and both Copy formats.
            inline.remove(.softBreak); inline.insert(.lineBreak)
            var replacement = AttributedString("\n")
            replacement.setAttributes(run.attributes)
            replacement.inlinePresentationIntent = inline
            content.replaceSubrange(run.range, with: replacement)
        }
        return content
    }

    private static func openingLine(
        at position: AttributedString.MarkdownSourcePosition, in lines: [Substring]
    ) -> String? {
        // Foundation represents a block ending before a blank line with endColumn
        // zero; its full Range initializer then returns nil. Fence classification
        // needs only the valid opening position, not that absent ending offset.
        guard position.startLine > 0, position.startLine <= lines.count, position.startColumn > 0 else { return nil }
        let line = lines[position.startLine - 1]
        guard position.startColumn - 1 <= line.utf8.count else { return nil }
        return String(bytes: line.utf8.dropFirst(position.startColumn - 1), encoding: .utf8)
    }

    private static func taskPrefix<S: StringProtocol>(_ text: S) -> Bool? {
        guard text.hasPrefix("[ ]") || text.hasPrefix("[x]") || text.hasPrefix("[X]") else { return nil }
        if let next = text.dropFirst(3).first, next != " " && next != "\t" { return nil }
        return !text.hasPrefix("[ ]")
    }

    private static func isFence(source: String, code: String, hint: String?) -> Bool {
        // Foundation's source position for fenced code includes its opening fence,
        // even inside a list or blockquote; indented code starts at code content.
        // An indented literal ``` line is content, not an opening fence. Without
        // a language hint, it appears verbatim as the first code line; a real
        // no-language opening fence cannot also be the first body line, since it
        // would close the block. Preserve language-tagged quoted/nested fences.
        let firstSource = source.prefix(while: { !$0.isNewline })
        guard let marker = firstSource.first, marker == "`" || marker == "~",
            firstSource.prefix(while: { $0 == marker }).count >= 3
        else { return false }
        if hint != nil { return true }
        let firstCode = code.prefix(while: { !$0.isNewline })
        return firstSource.trimmingTrailingHorizontalWhitespace != firstCode.trimmingTrailingHorizontalWhitespace
    }
}

extension Substring {
    fileprivate var trimmingTrailingHorizontalWhitespace: Substring {
        var result = self
        while let last = result.last, last == " " || last == "\t" { result = result.dropLast() }
        return result
    }
}
