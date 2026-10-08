import Foundation

struct MessageCodeToken: Sendable, Equatable {
    enum Kind: Sendable { case plain, keyword, string, comment, number }
    let text: String
    let kind: Kind
}

/// Linear lexical highlighting; no JavaScript, regex engine, URLs or process-wide cache.
enum MessageCodeHighlighter {
    static func tokens(_ source: String, language: String, byteLimit: Int = 65_536) -> [MessageCodeToken] {
        let language = language.lowercased()
        let supported: Set<String> = [
            "swift", "rust", "rs", "javascript", "js", "typescript", "ts",
            "tsx", "jsx", "python", "py", "go", "json", "c", "cpp", "java",
            "kotlin", "sh", "bash", "zsh", "ruby", "rb", "sql", "css",
        ]
        guard source.utf8.count <= byteLimit, supported.contains(language) else {
            return [.init(text: source, kind: .plain)]
        }
        let keywords: Set<String> = [
            "actor", "as", "async", "await", "break", "case", "catch", "class",
            "const", "continue", "def", "defer", "delete", "do", "else", "enum", "export", "extends",
            "false", "fn", "for", "from", "func", "function", "guard", "if", "impl", "import", "in",
            "interface", "is", "let", "match", "mut", "new", "nil", "null", "None", "private", "pub",
            "public", "return", "self", "static", "struct", "super", "switch", "throw", "throws",
            "trait", "true", "try", "type", "typeof", "undefined", "use", "var", "void", "while",
            "with", "yield", "SELECT", "FROM", "WHERE", "JOIN", "ON", "AS", "AND", "OR", "NULL",
        ]
        let hashComments = ["python", "py", "sh", "bash", "zsh", "ruby", "rb"].contains(language)
        let chars = Array(source)
        var tokens: [MessageCodeToken] = [], i = 0
        func isWord(_ c: Character) -> Bool { c.isLetter || c.isNumber || c == "_" || c == "$" }
        func append(_ start: Int, _ end: Int, _ kind: MessageCodeToken.Kind) {
            tokens.append(.init(text: String(chars[start..<end]), kind: kind))
        }
        while i < chars.count {
            guard tokens.count < 4_096 else { return [.init(text: source, kind: .plain)] }
            let start = i
            if (hashComments && chars[i] == "#")
                || (chars[i] == "/" && i + 1 < chars.count && chars[i + 1] == "/")
                || (language == "sql" && chars[i] == "-" && i + 1 < chars.count && chars[i + 1] == "-")
            {
                while i < chars.count, chars[i] != "\n" { i += 1 }
                append(start, i, .comment)
            } else if chars[i] == "/", i + 1 < chars.count, chars[i + 1] == "*" {
                i += 2
                while i < chars.count {
                    if chars[i] == "*", i + 1 < chars.count, chars[i + 1] == "/" { i += 2; break }
                    i += 1
                }
                append(start, i, .comment)
            } else if chars[i] == "\"" || chars[i] == "'" || chars[i] == "`" {
                let quote = chars[i]; i += 1
                while i < chars.count {
                    if chars[i] == "\\", i + 1 < chars.count { i += 2; continue }
                    let end = chars[i] == quote; i += 1
                    if end { break }
                }
                append(start, i, .string)
            } else if chars[i].isNumber {
                i += 1
                while i < chars.count, chars[i].isNumber || chars[i] == "." || chars[i] == "_" { i += 1 }
                append(start, i, .number)
            } else if isWord(chars[i]) {
                i += 1
                while i < chars.count, isWord(chars[i]) { i += 1 }
                append(start, i, keywords.contains(String(chars[start..<i])) ? .keyword : .plain)
            } else {
                i += 1
                while i < chars.count, !isWord(chars[i]),
                    !["\"", "'", "`", "/", "#", "-"].contains(chars[i])
                { i += 1 }
                append(start, i, .plain)
            }
        }
        return tokens
    }
}
