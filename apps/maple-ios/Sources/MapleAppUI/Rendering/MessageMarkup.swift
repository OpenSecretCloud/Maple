import Foundation

/// Resource limits apply to formatting work. Original message text is never truncated.
public struct MessageRenderLimits: Sendable, Equatable {
    public var documentBytes = 262_144
    public var formulaBytes = 16_384
    public var formulaCount = 128
    public var formulaDepth = 64
    public var highlightedCodeBytes = 65_536
    public init() {}
}

struct MessageFormula: Sendable, Equatable, Identifiable {
    let id: String
    let latex: String
    let original: String
    let display: Bool
    let isWithinBudget: Bool
}

struct MessageLiteral: Sendable { let id: String; let source: String }

struct MessageCodeBlock: Sendable, Equatable {
    let language: String
    let source: String
}

struct MessageMarkup: Sendable {
    let original: String
    let markdown: String
    let formulas: [MessageFormula]
    let codeBlocks: [String: MessageCodeBlock]
    let isPlainText: Bool
    var literals: [MessageLiteral] = []
}

/// Runs on its own executor, never during SwiftUI layout. No document cache or logging.
actor MessageMarkupParser {
    func parse(_ text: String, limits: MessageRenderLimits = .init()) throws -> MessageMarkup {
        try Task.checkCancellation()
        // These sentinels are internal placeholders, not a second user-facing syntax.
        guard text.utf8.count <= limits.documentBytes, !text.contains("\u{E000}maple-") else {
            return .init(original: text, markdown: text, formulas: [], codeBlocks: [:], isPlainText: true)
        }
        var lexer = MarkupLexer(limits: limits)
        let markdown = try lexer.process(text)
        return .init(
            original: text, markdown: markdown, formulas: lexer.formulas,
            codeBlocks: lexer.codeBlocks, isPlainText: lexer.requiresRawFallback, literals: lexer.literals)
    }
}

private struct Fence {
    let marker: Character
    let length: Int
    let language: String
    let indent: String

    init?(_ line: String) {
        let indentCount = line.prefix(while: { $0 == " " }).count
        guard indentCount <= 3 else { return nil }
        let body = line.dropFirst(indentCount)
        guard let marker = body.first, marker == "`" || marker == "~" else { return nil }
        let length = body.prefix(while: { $0 == marker }).count
        guard length >= 3 else { return nil }
        let suffix = body.dropFirst(length).trimmingCharacters(in: .whitespacesAndNewlines)
        if marker == "`", suffix.contains("`") { return nil }
        self.marker = marker
        self.length = length
        self.language = String(suffix.split(whereSeparator: \.isWhitespace).first ?? "")
        self.indent = String(repeating: " ", count: indentCount)
    }

    func closes(_ line: String) -> Bool {
        let body = line.drop(while: { $0 == " " })
        return line.count - body.count <= 3
            && body.prefix(while: { $0 == marker }).count >= length
            && body.drop(while: { $0 == marker }).allSatisfy(\.isWhitespace)
    }
}

private struct MarkupLexer {
    let limits: MessageRenderLimits
    var formulas: [MessageFormula] = []
    var codeBlocks: [String: MessageCodeBlock] = [:]
    var literals: [MessageLiteral] = []
    var requiresRawFallback = false

    mutating func process(_ text: String) throws -> String {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        var result = "", prose = "", code = ""
        var active: Fence?
        var token = ""
        for (index, slice) in lines.enumerated() {
            if index.isMultiple(of: 64) { try Task.checkCancellation() }
            let line = String(slice), suffix = index + 1 < lines.count ? "\n" : ""
            if let fence = active {
                if fence.closes(line) {
                    codeBlocks[token] = .init(language: fence.language, source: code.droppingFinalLineFeed)
                    result += code + line + suffix
                    active = nil
                    code = ""
                } else {
                    code += line + suffix
                }
            } else if let fence = Fence(line) {
                result += inline(prose)
                prose = ""
                token = "maplecode\(codeBlocks.count)"
                // Every fenced block uses our bounded highlighter. In particular a `math`
                // fence must not enter a library's built-in equation/cache path.
                result +=
                    fence.indent + String(repeating: String(fence.marker), count: fence.length)
                    + token + suffix
                active = fence
            } else {
                prose += line + suffix
            }
        }
        if let active {
            codeBlocks[token] = .init(language: active.language, source: code.droppingFinalLineFeed)
            result += code
        }
        result += inline(prose)
        return result
    }

    mutating func inline(_ source: String) -> String {
        let chars = Array(source)
        var result = "", i = 0
        func matches(_ token: [Character], at index: Int) -> Bool {
            index + token.count <= chars.count && Array(chars[index..<(index + token.count)]) == token
        }
        while i < chars.count {
            // Link destinations and explicit autolinks are URL data, not math.
            if matches(["]", "("], at: i) {
                var end = i + 2, depth = 1
                while end < chars.count, depth > 0 {
                    if chars[end] == "\\", end + 1 < chars.count { end += 2; continue }
                    if chars[end] == "(" { depth += 1 }
                    if chars[end] == ")" { depth -= 1 }
                    end += 1
                }
                result += String(chars[i..<end]); i = end; continue
            }
            if matches(Array("<https://"), at: i) || matches(Array("<http://"), at: i) {
                var end = i + 1
                while end < chars.count, chars[end] != ">" { end += 1 }
                end = min(end + 1, chars.count)
                result += String(chars[i..<end]); i = end; continue
            }
            if chars[i] == "`" {
                var length = 1
                while i + length < chars.count, chars[i + length] == "`" { length += 1 }
                var end = i + length
                var closed = false
                while end < chars.count {
                    if chars[end] == "`" {
                        var count = 1
                        while end + count < chars.count, chars[end + count] == "`" { count += 1 }
                        if count == length { end += count; closed = true; break }
                        end += count
                    } else {
                        end += 1
                    }
                }
                if closed { result += String(chars[i..<end]); i = end; continue }
                result += String(chars[i...]); break
            }
            let opening: [Character], closing: [Character], display: Bool
            if matches(["\\", "("], at: i) {
                opening = ["\\", "("]; closing = ["\\", ")"]; display = false
            } else if matches(["\\", "["], at: i) {
                opening = ["\\", "["]; closing = ["\\", "]"]; display = true
            } else if chars[i] == "\\", i + 1 < chars.count {
                result += String(chars[i...i + 1]); i += 2; continue
            } else if matches(["$", "$"], at: i) {
                opening = ["$", "$"]; closing = opening; display = true
            } else if chars[i] == "$", i + 1 < chars.count, !chars[i + 1].isWhitespace, !chars[i + 1].isNumber {
                opening = ["$"]; closing = opening; display = false
            } else {
                result.append(chars[i]); i += 1; continue
            }
            var end = i + opening.count
            while end < chars.count {
                if matches(closing, at: end), end > i + opening.count,
                    (display || !chars[end - 1].isWhitespace)
                {
                    break
                }
                if chars[end] == "\\", closing.first != "\\", end + 1 < chars.count { end += 2 } else { end += 1 }
            }
            guard end < chars.count else {
                // One scan, then keep incomplete streaming input visible. Do not repeatedly
                // rescan a suffix containing many unmatched delimiters.
                result += literal(String(chars[i...])); break
            }
            let latex = String(chars[(i + opening.count)..<end])
            let original = String(chars[i..<(end + closing.count)])
            guard formulas.count < limits.formulaCount else {
                result += literal(String(chars[i...])); break
            }
            let id = "\u{E000}maple-equation-\(formulas.count)\u{E001}"
            formulas.append(
                .init(
                    id: id, latex: latex, original: original, display: display,
                    isWithinBudget: withinFormulaBudget(latex)))
            result += display ? "\n\n" + id + "\n\n" : id
            i = end + closing.count
        }
        return result
    }

    mutating func literal(_ source: String) -> String {
        guard literals.count < limits.formulaCount else { requiresRawFallback = true; return source }
        let id = "\u{E000}maple-literal-\(literals.count)\u{E001}"
        literals.append(.init(id: id, source: source))
        return id
    }

    func withinFormulaBudget(_ source: String) -> Bool {
        guard source.utf8.count <= limits.formulaBytes else { return false }
        var depth = 0, escaped = false, cells = 1
        for scalar in source.unicodeScalars {
            if escaped { escaped = false; continue }
            if scalar == "\\" { escaped = true; continue }
            if scalar == "{" { depth += 1; if depth > limits.formulaDepth { return false } }
            if scalar == "}" { depth -= 1; if depth < 0 { return false } }
            if scalar == "&" { cells += 1; if cells > 4096 { return false } }
        }
        return depth == 0
    }
}

private extension String {
    var droppingFinalLineFeed: String { hasSuffix("\n") ? String(dropLast()) : self }
}
