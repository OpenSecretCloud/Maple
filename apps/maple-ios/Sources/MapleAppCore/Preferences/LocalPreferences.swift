import Foundation

public enum MapleTheme: String, Codable, CaseIterable, Sendable { case system, light, dark }
public enum MapleChatFont: String, Codable, CaseIterable, Sendable { case manrope, system, serif }

/// This is the playback rate. Request synthesis at speed 1 to avoid applying it twice.
public enum MaplePlaybackSpeed: Double, Codable, CaseIterable, Sendable {
    case normal = 1, relaxedFast = 1.25, fast = 1.5, double = 2
}

/// The supported provider identifiers, deliberately independent of translated labels.
public enum MapleVoice: String, Codable, CaseIterable, Sendable {
    case neutralFemale = "neutral_female", neutralMale = "neutral_male"
    case casualFemale = "casual_female", casualMale = "casual_male", cheerfulFemale = "cheerful_female"
    case arabicMale = "ar_male", germanFemale = "de_female", germanMale = "de_male"
    case spanishFemale = "es_female", spanishMale = "es_male", frenchFemale = "fr_female", frenchMale = "fr_male"
    case hindiFemale = "hi_female", hindiMale = "hi_male", italianFemale = "it_female", italianMale = "it_male"
    case dutchFemale = "nl_female", dutchMale = "nl_male", portugueseFemale = "pt_female", portugueseMale = "pt_male"
}

/// Nonsensitive device preferences only. Prompts, projects, drafts, credentials,
/// playback ownership and autoplay must never be added to this record.
public struct LocalPreferences: Codable, Equatable, Sendable {
    public var theme: MapleTheme
    public var chatFont: MapleChatFont
    public var selectedModelID: String
    public var webSearchPreferred: Bool
    public var voice: MapleVoice
    public var playbackSpeed: MaplePlaybackSpeed

    public init(
        theme: MapleTheme = .system, chatFont: MapleChatFont = .manrope,
        selectedModelID: String = "auto:quick", webSearchPreferred: Bool = true,
        voice: MapleVoice = .casualFemale, playbackSpeed: MaplePlaybackSpeed = .normal
    ) {
        self.theme = theme; self.chatFont = chatFont; self.selectedModelID = selectedModelID
        self.webSearchPreferred = webSearchPreferred; self.voice = voice; self.playbackSpeed = playbackSpeed
    }

    public static func isValidModelID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 256
            && value.utf8.allSatisfy {
                (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
                    || [45, 46, 47, 58, 95].contains($0)
            }
    }
}
