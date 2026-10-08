import Foundation

/// Package-owned interface text must resolve in the UI resource bundle. Model,
/// account, and user content must bypass this helper and remain verbatim.
enum MapleStrings {
    static func text(_ key: String.LocalizationValue, locale: Locale = .current) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }

    static func queuedMessageCount(_ count: Int, locale: Locale = .current) -> String {
        String(
            localized: "chat.queued-message-count", defaultValue: "\(count) queued messages",
            bundle: .module, locale: locale)
    }

    static func imageCount(_ count: Int, locale: Locale = .current) -> String {
        String(
            localized: "chat.image-count", defaultValue: "\(count) images",
            bundle: .module, locale: locale)
    }

    static func externalLinkMessage(_ destination: String, locale: Locale = .current) -> String {
        String(
            localized: "browser.external-link-message",
            defaultValue: "You’re leaving Maple content.\n\(destination)", bundle: .module, locale: locale)
    }
}
