import Foundation
import MapleSDK

public enum MapleTheme: String, Codable, CaseIterable, Sendable { case system, light, dark }
public enum MapleChatFont: String, Codable, CaseIterable, Sendable { case manrope, system, serif }

/// Stable preference values shared with Research's chatThinkingLevel choice.
public enum MapleThinkingLevel: String, Codable, CaseIterable, Sendable {
    case `default`, none, minimal, low, medium, high, xhigh, max

    public var effort: ReasoningEffort? { ReasoningEffort(rawValue: rawValue) }
    public var label: String {
        switch self {
        case .default: "Default"
        case .none: "Off"
        case .minimal: "Minimal"
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        case .xhigh: "Extra high"
        case .max: "Max"
        }
    }
}

/// This is the playback rate. Request synthesis at speed 1 to avoid applying it twice.
public enum MaplePlaybackSpeed: Double, Codable, CaseIterable, Sendable {
    case normal = 1, slightlyFast = 1.2, relaxedFast = 1.25, fast = 1.5, double = 2
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
    public var thinkingLevel: MapleThinkingLevel
    public var voice: MapleVoice
    public var playbackSpeed: MaplePlaybackSpeed
    public var sidebarIsOpen: Bool

    public init(
        theme: MapleTheme = .system, chatFont: MapleChatFont = .manrope,
        selectedModelID: String = "auto:quick", webSearchPreferred: Bool = true,
        thinkingLevel: MapleThinkingLevel = .default,
        voice: MapleVoice = .casualFemale, playbackSpeed: MaplePlaybackSpeed = .slightlyFast,
        sidebarIsOpen: Bool = true
    ) {
        self.theme = theme; self.chatFont = chatFont; self.selectedModelID = selectedModelID
        self.webSearchPreferred = webSearchPreferred; self.voice = voice; self.playbackSpeed = playbackSpeed
        self.thinkingLevel = thinkingLevel
        self.sidebarIsOpen = sidebarIsOpen
    }

    private enum CodingKeys: String, CodingKey {
        case theme, chatFont, selectedModelID, webSearchPreferred, voice, playbackSpeed, sidebarIsOpen
        case thinkingLevel = "chatThinkingLevel"
    }

    public init(from decoder: any Decoder) throws {
        let record = try decoder.container(keyedBy: CodingKeys.self)
        theme = try record.decode(MapleTheme.self, forKey: .theme)
        chatFont = try record.decode(MapleChatFont.self, forKey: .chatFont)
        selectedModelID = try record.decode(String.self, forKey: .selectedModelID)
        webSearchPreferred = try record.decodeIfPresent(Bool.self, forKey: .webSearchPreferred) ?? true
        // Research treats an absent or obsolete stored thinking choice as Default.
        let storedLevel = try record.decodeIfPresent(String.self, forKey: .thinkingLevel)
        thinkingLevel = storedLevel.flatMap(MapleThinkingLevel.init(rawValue:)) ?? .default
        voice = try record.decode(MapleVoice.self, forKey: .voice)
        playbackSpeed = try record.decode(MaplePlaybackSpeed.self, forKey: .playbackSpeed)
        // Existing records predate the sidebar choice; their selected speech rate remains authoritative.
        sidebarIsOpen = record.contains(.sidebarIsOpen) ? try record.decode(Bool.self, forKey: .sidebarIsOpen) : true
    }

    public static func isValidModelID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 256
            && value.utf8.allSatisfy {
                (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
                    || [45, 46, 47, 58, 95].contains($0)
            }
    }
}
