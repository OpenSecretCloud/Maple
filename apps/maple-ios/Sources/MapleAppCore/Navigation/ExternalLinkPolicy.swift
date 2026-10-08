import Foundation

public enum ExternalLinkPolicy {
    /// A displayed link cannot smuggle file access, script execution or embedded
    /// credentials into either the embedded browser or a system handoff.
    public static func isWeb(_ url: URL) -> Bool {
        guard url.absoluteString.utf8.count <= 8_192,
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let scheme = parts.scheme?.lowercased(), ["https", "http"].contains(scheme),
            let host = parts.host, !host.isEmpty,
            parts.user == nil, parts.password == nil,
            parts.port.map({ (1...65_535).contains($0) }) ?? true,
            !url.absoluteString.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else { return false }
        return true
    }

    public static func isMail(_ url: URL) -> Bool {
        guard url.absoluteString.utf8.count <= 4_096, url.scheme?.lowercased() == "mailto",
            let decoded = url.absoluteString.removingPercentEncoding,
            !decoded.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        else { return false }
        return !url.path.isEmpty
    }
}
