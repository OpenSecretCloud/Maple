import Foundation
import MapleSDK

enum ChatToolPresentation {
    enum Status: String, Equatable, Sendable {
        case active = "In progress", completed = "Completed", incomplete = "Incomplete", failed = "Failed"
    }
    struct Details: Equatable, Sendable {
        let input: String
        let output: String
        var hasDetails: Bool { !input.isEmpty || !output.isEmpty }
    }

    static func arguments(_ value: JSONValue?) -> JSONValue? {
        guard case .string(let text) = value else { return value }
        guard text.utf8.count <= 1_048_576 else { return value }
        return (try? JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))) ?? value
    }

    static func title(name: String?, arguments value: JSONValue?, webSearch: Bool = false, locale: Locale = .current)
        -> String
    {
        let name = webSearch ? "web_search" : (name ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let value = arguments(value)
        func string(_ key: String) -> String? {
            guard case .object(let object) = value, case .string(let text) = object[key],
                !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            else { return nil }
            return text
        }
        let query = string("query")
        switch name {
        case "web_search":
            guard let query else {
                return String(
                    localized: "chat.tool-search-name", defaultValue: "Web Search",
                    table: "Localizable", bundle: .module, locale: locale)
            }
            return String(
                localized: "chat.tool-search-title", defaultValue: "Web Search: \"\(query)\"",
                table: "Localizable", bundle: .module, locale: locale)
        case "open_url":
            guard let url = string("url") else { return MapleStrings.text("Open URL", locale: locale) }
            return String(
                localized: "chat.tool-url-title", defaultValue: "Open URL: \(url)",
                table: "Localizable", bundle: .module, locale: locale)
        case "open_urls":
            var urls: [String] = []
            if case .object(let object) = value, case .array(let values) = object["urls"] {
                urls = values.compactMap {
                    guard case .string(let url) = $0, !url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    else { return nil }
                    return url
                }
            }
            if urls.count == 1 {
                return String(
                    localized: "chat.tool-url-title", defaultValue: "Open URL: \(urls[0])",
                    table: "Localizable", bundle: .module, locale: locale)
            }
            guard !urls.isEmpty else { return MapleStrings.text("Open URLs", locale: locale) }
            return String(
                localized: "chat.tool-urls-title", defaultValue: "Open URLs: \(urls.count) pages",
                table: "Localizable", bundle: .module, locale: locale)
        case "read_image":
            guard case .object(let object) = value else { return MapleStrings.text("Read image", locale: locale) }
            let number: Double
            switch object["image_number"] {
            case .integer(let integer): number = Double(integer)
            case .number(let decimal) where decimal.isFinite: number = decimal
            default: return MapleStrings.text("Read image", locale: locale)
            }
            let image = number.formatted(.number.grouping(.never).locale(locale))
            return String(
                localized: "chat.tool-image-title", defaultValue: "Read image: Image \(image)",
                table: "Localizable", bundle: .module, locale: locale)
        case "", "function": return MapleStrings.text("Tool call", locale: locale)
        default:
            guard let query else { return name }
            return String(
                localized: "chat.tool-named-query-title", defaultValue: "\(name): \"\(query)\"",
                table: "Localizable", bundle: .module, locale: locale)
        }
    }

    static func status(item: ConversationItem, result: ConversationItem?) -> Status {
        if item.type == "web_search_call" {
            switch item.status {
            case "in_progress", "searching": return .active
            case "completed": return .completed
            case "incomplete": return .incomplete
            default: return .failed
            }
        }
        let statuses = [item.status, result?.status]
        if statuses.contains(where: { $0 == "failed" || $0 == "error" }) { return .failed }
        if statuses.contains("incomplete") { return .incomplete }
        if ChatTranscriptRow.isToolOutput(item) {
            return ["in_progress", "streaming"].contains(item.status ?? "") ? .active : .completed
        }
        if let result {
            return ["in_progress", "streaming"].contains(result.status ?? "") ? .active : .completed
        }
        return .active
    }

    static func details(item: ConversationItem, result: ConversationItem?) -> Details {
        func formatted(_ value: JSONValue?) -> String {
            guard let value else { return "" }
            let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            guard let bytes = try? encoder.encode(value), bytes.count <= 1_048_576 else { return "" }
            return String(decoding: bytes, as: UTF8.self)
        }
        func output(_ item: ConversationItem?) -> String {
            guard let item else { return "" }
            if case .string(let text) = item.output { return text.utf8.count <= 1_048_576 ? text : "" }
            if item.output != nil { return formatted(item.output) }
            let text = (item.content ?? []).compactMap(\.text).joined(separator: "\n\n")
            return text.utf8.count <= 1_048_576 ? text : ""
        }
        let input: String
        if ChatTranscriptRow.isToolOutput(item) || item.type == "web_search_call" {
            input = ""
        } else if case .string(let text) = item.arguments, arguments(item.arguments) == item.arguments {
            input =
                text.utf8.count <= 1_048_576 && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? text : ""
        } else {
            input = formatted(arguments(item.arguments))
        }
        return Details(
            input: input, output: [output(item), output(result)].filter { !$0.isEmpty }.joined(separator: "\n\n"))
    }

    /// Research's 150-character preview retains full destinations when it cuts
    /// a plain URL, and stops before a Markdown link rather than breaking it.
    static func preview(_ text: String, limit: Int = 150) -> String {
        let source = text as NSString
        guard source.length > limit, limit > 0 else { return text }
        let fullRange = NSRange(location: 0, length: source.length)
        let markdown =
            (try? NSRegularExpression(pattern: "\\[([^\\]]*)\\]\\(([^)]+)\\)"))?
            .matches(in: text, range: fullRange).map(\.range) ?? []
        let urls =
            (try? NSRegularExpression(pattern: "https?://[^\\s\\]<>]+"))?
            .matches(in: text, range: fullRange).map(\.range) ?? []
        for range in urls where !markdown.contains(where: { NSLocationInRange(range.location, $0) }) {
            if limit > range.location, limit < NSMaxRange(range) {
                let before = source.substring(to: range.location), url = source.substring(with: range)
                let display = prefix(source.substring(with: range), utf16Length: limit - range.location)
                return before + "[" + display + "...](" + url + ")"
            }
        }
        for range in markdown where limit > range.location && limit < NSMaxRange(range) {
            return source.substring(to: range.location).trimmingCharacters(in: .whitespacesAndNewlines) + "..."
        }
        return prefix(text, utf16Length: limit) + "..."
    }

    private static func prefix(_ text: String, utf16Length: Int) -> String {
        var length = min(utf16Length, text.utf16.count)
        while length > 0 {
            if let range = Range(NSRange(location: 0, length: length), in: text) { return String(text[range]) }
            length -= 1
        }
        return ""
    }
}
