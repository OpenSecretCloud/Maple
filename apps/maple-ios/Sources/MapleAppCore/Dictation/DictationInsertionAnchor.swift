import Foundation

/// A right-affinity logical insertion point. Its text snapshot is memory-only;
/// caret movement after Stop never changes this position. Edits are compared at
/// extended grapheme boundaries, so emoji and combining marks are not split.
public struct DictationInsertionAnchor: Sendable, Equatable {
    public private(set) var text: String
    public private(set) var offsetUTF16: Int

    public init(text: String, offsetUTF16: Int) {
        self.text = text; self.offsetUTF16 = Self.boundary(in: text, atOrBefore: offsetUTF16)
    }

    public mutating func rebase(to replacement: String) {
        guard text != replacement else { return }
        let old = Array(text); let new = Array(replacement)
        var prefix = 0
        while prefix < min(old.count, new.count), old[prefix] == new[prefix] { prefix += 1 }
        var suffix = 0
        while suffix < min(old.count - prefix, new.count - prefix),
            old[old.count - suffix - 1] == new[new.count - suffix - 1]
        { suffix += 1 }
        let start = old.prefix(prefix).reduce(0) { $0 + String($1).utf16.count }
        let oldEnd = old.prefix(old.count - suffix).reduce(0) { $0 + String($1).utf16.count }
        let newEnd = new.prefix(new.count - suffix).reduce(0) { $0 + String($1).utf16.count }
        if offsetUTF16 >= oldEnd {
            offsetUTF16 += newEnd - oldEnd
        } else if offsetUTF16 >= start {
            offsetUTF16 = newEnd
        }
        text = replacement
        offsetUTF16 = Self.boundary(in: replacement, atOrBefore: offsetUTF16)
    }

    @discardableResult public mutating func rebase(using edit: DictationTextEdit) -> Bool {
        guard edit.beforeText == text, let replacement = edit.resultingText else { return false }
        let start = edit.rangeUTF16.location
        let oldEnd = start + edit.rangeUTF16.length
        let newEnd = start + edit.replacement.utf16.count
        if offsetUTF16 >= oldEnd {
            offsetUTF16 += newEnd - oldEnd
        } else if offsetUTF16 >= start {
            offsetUTF16 = newEnd
        }
        text = replacement
        offsetUTF16 = Self.boundary(in: replacement, atOrBefore: offsetUTF16)
        return true
    }

    public func inserting(_ transcript: String) -> String {
        let index = String.Index(utf16Offset: offsetUTF16, in: text)
        let before = String(text[..<index]); let after = String(text[index...])
        let value = transcript.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return text }
        let left = needsSpace(before.last, value.first) ? " " : ""
        let right = needsSpace(value.last, after.first) ? " " : ""
        return before + left + value + right + after
    }

    private func needsSpace(_ left: Character?, _ right: Character?) -> Bool {
        guard let left, let right, !left.isWhitespace, !right.isWhitespace else { return false }
        return !",.;:!?)]}、。！？".contains(right) && !"([{“‘".contains(left)
    }

    private static func boundary(in text: String, atOrBefore requested: Int) -> Int {
        let limit = max(0, min(requested, text.utf16.count))
        var position = 0
        for character in text {
            let next = position + String(character).utf16.count
            if next > limit { break }
            position = next
        }
        return position
    }
}
