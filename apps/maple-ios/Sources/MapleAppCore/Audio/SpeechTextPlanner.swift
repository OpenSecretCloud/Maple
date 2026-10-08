import Foundation

/// Bounded native equivalent of the Research speech text preparation rules.
/// Run this synchronous parser away from the main actor for real replies.
public enum SpeechTextPlanner {
    public static func chunks(_ text: String, maximumWords: Int = 300) throws -> [String] {
        guard (1...300).contains(maximumWords) else { return [] }
        let sanitized = try sanitize(text)
        guard !sanitized.isEmpty else { return [] }
        var chunks: [String] = [], current = "", count = 0
        func flush() {
            guard !current.isEmpty else { return }
            chunks.append(normalize(current)); current = ""; count = 0
        }
        func wordPieces(_ word: String) throws -> [String] {
            var result: [String] = [], piece = "", scalars = 0
            for character in word {
                let size = character.unicodeScalars.count
                guard size <= 1_000 else { throw SpeechIssue.capacity }
                if scalars + size > 1_000 { result.append(piece); piece = ""; scalars = 0 }
                piece.append(character); scalars += size
            }
            if !piece.isEmpty { result.append(piece) }
            return result
        }
        func push(_ unit: String) throws {
            let words = unit.split(whereSeparator: \.isWhitespace).map(String.init)
            guard !words.isEmpty else { return }
            if words.count > maximumWords || words.contains(where: { $0.unicodeScalars.count > 1_000 }) {
                flush()
                for word in words {
                    if word.unicodeScalars.count > 1_000 {
                        flush(); chunks += try wordPieces(word).map(normalize)
                    } else {
                        if count == maximumWords
                            || current.unicodeScalars.count + word.unicodeScalars.count + 2 > 100_000
                        {
                            flush()
                        }
                        current += current.isEmpty ? word : " " + word; count += 1
                    }
                }
                flush(); return
            }
            let joined = words.joined(separator: " ")
            if count + words.count > maximumWords
                || current.unicodeScalars.count + joined.unicodeScalars.count + 2 > 100_000
            {
                flush()
            }
            // Many individually legal tokens can exceed a provider request cap.
            if joined.unicodeScalars.count + 1 > 100_000 {
                for word in words {
                    if count == maximumWords || current.unicodeScalars.count + word.unicodeScalars.count + 2 > 100_000 {
                        flush()
                    }
                    current += current.isEmpty ? word : " " + word; count += 1
                }
                flush(); return
            }
            current += current.isEmpty ? joined : " " + joined; count += words.count
        }
        for paragraph in sanitized.components(separatedBy: "\n\n") {
            try Task.checkCancellation()
            if paragraph.split(whereSeparator: \.isWhitespace).count <= maximumWords {
                try push(paragraph)
            } else {
                let regex = try NSRegularExpression(pattern: "[.!?]\\s+")
                let value = paragraph as NSString
                var start = 0
                for match in regex.matches(in: paragraph, range: NSRange(location: 0, length: value.length)) {
                    try push(value.substring(with: NSRange(location: start, length: match.range.location + 1 - start)))
                    start = NSMaxRange(match.range)
                }
                try push(value.substring(from: start))
            }
            guard chunks.count <= SpeechLimits.maximumChunks else { throw SpeechIssue.capacity }
        }
        flush()
        guard chunks.count <= SpeechLimits.maximumChunks else { throw SpeechIssue.capacity }
        return chunks
    }

    public static func sanitize(_ text: String) throws -> String {
        guard text.utf8.count <= SpeechLimits.maximumSourceBytes else { throw SpeechIssue.capacity }
        var lines: [String] = [], fence: Character?, fenceLength = 0
        for line in text.components(separatedBy: .newlines) {
            try Task.checkCancellation()
            let opening = replace(line, #"^\s*(?:>+\s*)?"#, "")
            let first = opening.first
            let repeated = opening.prefix(while: { $0 == first }).count
            if let active = fence {
                if first == active, repeated >= fenceLength,
                    opening.dropFirst(repeated).trimmingCharacters(in: .whitespaces).isEmpty
                {
                    fence = nil
                }
            } else if (first == "`" || first == "~"), repeated >= 3 {
                fence = first; fenceLength = repeated
            } else {
                lines.append(line)
            }
        }
        var value = replace(lines.joined(separator: "\n"), #"(?is)<think>.*?(?:</think>|$)"#, "")
            .precomposedStringWithCanonicalMapping
        let marker = #"^[ \t]*(?:[-+*•‣⁃∙·◦▪▫●○■□]|\d{1,4}[.)]|\(\d{1,4}\))[ \t]+"#
        value = value.components(separatedBy: "\n").map { line in
            let compact = line.filter { !$0.isWhitespace }
            if compact.count >= 3 && ["-", "*", "_"].contains(String(compact.prefix(1))) && Set(compact).count == 1 {
                return ""
            }
            let isList = line.range(of: marker, options: .regularExpression) != nil
            var result = replace(line, marker, "")
            result = replaceDashSeparators(result)
            if isList && !result.isEmpty
                && replace(result, #"[*_~`]+$"#, "").range(of: #"[,.!?;:…]$"#, options: .regularExpression) == nil
            {
                result += "."
            }
            return result
        }.joined(separator: "\n")
        for (pattern, replacement) in [
            (#"\*\*([^*]+)\*\*"#, "$1"), (#"__([^_]+)__"#, "$1"), (#"\*([^*]+)\*"#, "$1"),
            (#"_([^_\s][^_]*)_"#, "$1"), (#"~~([^~]+)~~"#, "$1"), (#"`([^`]+)`"#, "$1"),
            (#"(?m)^\s*#{1,6}\s*"#, ""), (#"</?[A-Za-z][A-Za-z0-9_-]*(?:\s+[^>]*)?>"#, " "),
        ] { value = replace(value, pattern, replacement) }
        value = String(
            String.UnicodeScalarView(
                value.unicodeScalars.filter { scalar in
                    let number = scalar.value
                    return !(0x1F300...0x1FAFF).contains(number) && !(0x2600...0x27BF).contains(number)
                        && !(0x1F1E6...0x1F1FF).contains(number) && !(0xE0020...0xE007F).contains(number)
                        && ![0x200D, 0x20E3, 0xFE0E, 0xFE0F].contains(number)
                }))
        for (from, to) in [
            ("¯", " "), ("_", " "), ("“", "\""), ("”", "\""), ("‘", "'"), ("’", "'"), ("´", "'"), ("`", "'"),
            ("[", " "), ("]", " "), ("|", " "), ("/", " "), ("#", " "), ("→", " "), ("←", " "),
            ("♥", ""), ("☆", ""), ("♡", ""), ("©", ""), ("\\", ""), ("@", " at "), ("e.g.,", "for example, "),
            ("i.e.,", "that is, "),
        ] { value = value.replacingOccurrences(of: from, with: to) }
        value = replace(value, #"\s+([,.!?;:'])"#, "$1")
        value = replace(value, #""{2,}"#, "\"")
        value = replace(value, #"'{2,}"#, "'")
        value = value.components(separatedBy: "\n").map {
            replace($0, #"[^\S\r\n]+"#, " ").trimmingCharacters(in: .whitespaces)
        }.joined(separator: "\n")
        return replace(value, #"\n{3,}"#, "\n\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func normalize(_ value: String) -> String {
        let text = replace(value, #"\s+"#, " ").trimmingCharacters(in: .whitespacesAndNewlines)
        return text.range(of: #"[.!?;:,'"“”‘’)\]}…。」』】〉》›»]$"#, options: .regularExpression) != nil ? text : text + "."
    }
    private static func replaceDashSeparators(_ line: String) -> String {
        guard let expression = try? NSRegularExpression(pattern: #"[ \t]+[-‑‒][ \t]+|[ \t]*[–—―][ \t]*"#) else {
            return line
        }
        let source = line as NSString
        let matches = expression.matches(in: line, range: NSRange(location: 0, length: source.length))
        guard !matches.isEmpty else { return line }
        func neighbor(from offset: Int, forward: Bool) -> Character? {
            var offset = offset
            while offset >= 0 && offset < source.length {
                let range = source.rangeOfComposedCharacterSequence(at: offset)
                guard let character = source.substring(with: range).first else { return nil }
                if !character.isWhitespace { return character }
                offset = forward ? NSMaxRange(range) : range.location - 1
            }
            return nil
        }
        var result = "", start = 0
        for match in matches {
            let before = neighbor(from: match.range.location - 1, forward: false)
            let after = neighbor(from: NSMaxRange(match.range), forward: true)
            let replacement: String
            if source.substring(with: match.range) == "–", let before, let after,
                before.isLetter || before.isNumber, after.isLetter || after.isNumber
            {
                replacement = " to "
            } else if before == nil || after == nil
                || before.map({ ",.!?;:".contains($0) }) == true
                || after.map({ ",.!?;:".contains($0) }) == true
            {
                replacement = " "
            } else {
                replacement = ", "
            }
            result += source.substring(with: NSRange(location: start, length: match.range.location - start))
            result += replacement; start = NSMaxRange(match.range)
        }
        return result + source.substring(from: start)
    }
    private static func replace(_ text: String, _ pattern: String, _ replacement: String) -> String {
        text.replacingOccurrences(of: pattern, with: replacement, options: .regularExpression)
    }
}
