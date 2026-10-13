import Foundation

/// Research's newline edit, expressed as the exact native replacement rather
/// than a character diff so dictation insertion anchors retain their meaning.
struct ComposerListEdit: Equatable, Sendable {
    let rangeUTF16: NSRange
    let replacement: String
    let selectionUTF16: NSRange
}

enum ComposerListContinuation {
    private static let unordered = try? NSRegularExpression(pattern: #"^([ \t]*)([-+*])([ \t]+)(.*)$"#)
    private static let taskMarker = try? NSRegularExpression(pattern: #"^\[([ xX])\]([ \t]*)(.*)$"#)
    private static let ordered = try? NSRegularExpression(pattern: #"^([ \t]*)([0-9]+)([.)])([ \t]+)(.*)$"#)
    private static let fenceOpening = try? NSRegularExpression(pattern: #"^[ ]{0,3}(`{3,}|~{3,})"#)
    private static let fenceClosing = try? NSRegularExpression(pattern: #"^[ ]{0,3}(`+|~+)[ \t]*$"#)
    private static let rule = try? NSRegularExpression(pattern: #"^((?:-\s*){3,}|(?:\*\s*){3,})$"#)

    private struct Marker {
        let indentation: String
        let continuation: String
        let item: String
    }

    static func edit(text: String, selectionUTF16 selection: NSRange) -> ComposerListEdit? {
        let source = text as NSString
        guard selection.location != NSNotFound, selection.location >= 0, selection.length >= 0,
            selection.location <= source.length, selection.length <= source.length - selection.location,
            scalarBoundary(selection.location, in: text), scalarBoundary(NSMaxRange(selection), in: text),
            Range(selection, in: text) != nil
        else { return nil }
        let previousBreak = source.range(
            of: "\n", options: .backwards, range: NSRange(location: 0, length: selection.location))
        let lineStart = previousBreak.location == NSNotFound ? 0 : previousBreak.location + 1
        let followingBreak = source.range(
            of: "\n", range: NSRange(location: selection.location, length: source.length - selection.location))
        let lineEnd = followingBreak.location == NSNotFound ? source.length : followingBreak.location
        let line = source.substring(with: NSRange(location: lineStart, length: lineEnd - lineStart))
        guard !horizontalRule(line),
            let marker = listMarker(
                source.substring(with: NSRange(location: lineStart, length: selection.location - lineStart))),
            !insideFence(source.substring(to: lineStart))
        else { return nil }
        if let full = listMarker(line), full.item.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let caret = lineStart + marker.indentation.utf16.count
            return ComposerListEdit(
                rangeUTF16: NSRange(location: caret, length: max(lineEnd, NSMaxRange(selection)) - caret),
                replacement: "", selectionUTF16: NSRange(location: caret, length: 0))
        }
        let replacement = "\n" + marker.continuation
        return ComposerListEdit(
            rangeUTF16: selection, replacement: replacement,
            selectionUTF16: NSRange(location: selection.location + replacement.utf16.count, length: 0))
    }

    private static func scalarBoundary(_ offset: Int, in text: String) -> Bool {
        let index = text.utf16.index(text.utf16.startIndex, offsetBy: offset)
        return index.samePosition(in: text.unicodeScalars) != nil
    }

    private static func listMarker(_ line: String) -> Marker? {
        if let parts = captures(unordered, in: line) {
            if let task = captures(taskMarker, in: parts[3]),
                !task[1].isEmpty || task[2].isEmpty
            {
                return Marker(
                    indentation: parts[0], continuation: parts[0] + parts[1] + parts[2] + "[ ]" + task[1],
                    item: task[2])
            }
            return Marker(indentation: parts[0], continuation: parts[0] + parts[1] + parts[2], item: parts[3])
        }
        guard let parts = captures(ordered, in: line) else { return nil }
        return Marker(
            indentation: parts[0], continuation: parts[0] + increment(parts[1]) + parts[2] + parts[3], item: parts[4])
    }

    // JavaScript's BigInt increments without a machine-integer limit and drops
    // leading zeroes. Keep that behavior for pasted or very long list markers.
    private static func increment(_ decimal: String) -> String {
        var digits = Array(decimal.utf8.drop(while: { $0 == 48 }))
        if digits.isEmpty { return "1" }
        for index in digits.indices.reversed() {
            if digits[index] != 57 {
                digits[index] += 1
                return String(decoding: digits, as: UTF8.self)
            }
            digits[index] = 48
        }
        return "1" + String(decoding: digits, as: UTF8.self)
    }

    private static func insideFence(_ prefix: String) -> Bool {
        var fence: (marker: Character, length: Int)?
        for line in prefix.components(separatedBy: "\n") {
            if let active = fence {
                if let closing = captures(fenceClosing, in: line),
                    closing[0].first == active.marker, closing[0].count >= active.length
                {
                    fence = nil
                }
            } else if let opening = captures(fenceOpening, in: line),
                let marker = opening[0].first
            {
                fence = (marker, opening[0].count)
            }
        }
        return fence != nil
    }

    private static func horizontalRule(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        return captures(rule, in: trimmed) != nil
    }

    private static func captures(_ expression: NSRegularExpression?, in text: String) -> [String]? {
        guard let expression,
            let match = expression.firstMatch(in: text, range: NSRange(location: 0, length: text.utf16.count))
        else { return nil }
        let source = text as NSString
        return (1..<match.numberOfRanges).map { source.substring(with: match.range(at: $0)) }
    }
}
