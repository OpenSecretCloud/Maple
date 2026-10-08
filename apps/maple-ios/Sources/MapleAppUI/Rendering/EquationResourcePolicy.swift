import Foundation

/// The equation process can read only these packaged assets. It cannot resolve a
/// renderer-controlled path, query, network origin, or credential-bearing URL.
enum EquationResourcePolicy {
    static let scheme = "maple-equation"
    static let origin = "maple-equation://bundle"
    static let bootstrap = URL(string: "about:blank")!
    static let fonts: Set<String> = [
        "AMS-Regular", "Caligraphic-Bold", "Caligraphic-Regular", "Fraktur-Bold",
        "Fraktur-Regular", "Main-Bold", "Main-BoldItalic", "Main-Italic", "Main-Regular",
        "Math-BoldItalic", "Math-Italic", "SansSerif-Bold", "SansSerif-Italic", "SansSerif-Regular",
        "Script-Regular", "Size1-Regular", "Size2-Regular", "Size3-Regular", "Size4-Regular",
        "Typewriter-Regular",
    ]
    static let paths: Set<String> = Set(["index.html", "renderer.js", "renderer.css", "katex.min.js", "katex.min.css"])
        .union(fonts.map { "fonts/KaTeX_\($0).woff2" })

    static func path(for url: URL) -> String? {
        guard url.scheme == scheme, url.host == "bundle", url.port == nil,
            url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
            !url.absoluteString.contains("%"), !url.absoluteString.contains("\\")
        else { return nil }
        let path = String(url.path.dropFirst())
        guard url.path == "/" + path, paths.contains(path) else { return nil }
        return path
    }

    static func resource(_ path: String) -> URL? {
        guard paths.contains(path), let root = Bundle.module.url(forResource: "EquationRenderer", withExtension: nil)
        else { return nil }
        return root.appendingPathComponent(path)
    }

    /// The bootstrap is packaged application code, never message-derived HTML.
    static func bootstrapHTML() -> String? {
        guard let url = resource("index.html"), let data = try? Data(contentsOf: url),
            !data.isEmpty, data.count <= 1_048_576
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static let contentRules =
        #"[{"trigger":{"url-filter":".*"},"action":{"type":"block"}},{"trigger":{"url-filter":"^maple-equation://bundle/"},"action":{"type":"ignore-previous-rules"}}]"#
}

/// Only the internally initiated document load may navigate this web view.
struct EquationBootstrapPolicy {
    private(set) var started = false

    mutating func admit(_ url: URL?, mainFrame: Bool, other: Bool) -> Bool {
        guard !started, mainFrame, other,
            url?.absoluteString == EquationResourcePolicy.bootstrap.absoluteString
        else { return false }
        started = true
        return true
    }

    func allowsResponse(_ url: URL?, mainFrame: Bool) -> Bool {
        started && mainFrame && url?.absoluteString == EquationResourcePolicy.bootstrap.absoluteString
    }
}
