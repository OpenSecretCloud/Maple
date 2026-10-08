#if os(iOS)
    import MapleAppCore
    import SwiftUI

    extension MapleTheme {
        public var preferredColorScheme: ColorScheme? {
            switch self {
            case .system: nil;
            case .light: .light;
            case .dark: .dark
            }
        }
        public var messageRenderTheme: MessageRenderTheme {
            switch self {
            case .system: .automatic;
            case .light: .light;
            case .dark: .dark
            }
        }
        var settingsLabel: String {
            switch self {
            case .system: "System";
            case .light: "Light";
            case .dark: "Dark"
            }
        }
    }

    extension MapleChatFont {
        public var messageRenderFont: MessageRenderFont {
            switch self {
            case .manrope: .manrope;
            case .system: .system;
            case .serif: .serif
            }
        }
        var settingsLabel: String {
            switch self {
            case .manrope: "Manrope";
            case .system: "System";
            case .serif: "Serif"
            }
        }
    }

    extension MapleVoice {
        var settingsLabel: String {
            switch self {
            case .neutralFemale: "Neutral — Female"
            case .neutralMale: "Neutral — Male"
            case .casualFemale: "Casual — Female"
            case .casualMale: "Casual — Male"
            case .cheerfulFemale: "Cheerful — Female"
            case .arabicMale: "Arabic-accented — Male"
            case .germanFemale: "German-accented — Female"
            case .germanMale: "German-accented — Male"
            case .spanishFemale: "Spanish-accented — Female"
            case .spanishMale: "Spanish-accented — Male"
            case .frenchFemale: "French-accented — Female"
            case .frenchMale: "French-accented — Male"
            case .hindiFemale: "Hindi-accented — Female"
            case .hindiMale: "Hindi-accented — Male"
            case .italianFemale: "Italian-accented — Female"
            case .italianMale: "Italian-accented — Male"
            case .dutchFemale: "Dutch-accented — Female"
            case .dutchMale: "Dutch-accented — Male"
            case .portugueseFemale: "Portuguese-accented — Female"
            case .portugueseMale: "Portuguese-accented — Male"
            }
        }
    }
#endif
