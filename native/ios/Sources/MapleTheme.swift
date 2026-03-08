import SwiftUI

// MARK: - Color Scales

extension Color {
    // Maple (Primary) - "Primary brand energy"
    static let maple50  = Color(hex: 0xFFF4F0)
    static let maple100 = Color(hex: 0xFFE8E0)
    static let maple200 = Color(hex: 0xFFD1C1)
    static let maple300 = Color(hex: 0xFFBAA2)
    static let maple400 = Color(hex: 0xFFA88A)
    static let maple500 = Color(hex: 0xFF9771)
    static let maple600 = Color(hex: 0xF67D57)
    static let maple700 = Color(hex: 0xE8633D)
    static let maple800 = Color(hex: 0xD04926)
    static let maple900 = Color(hex: 0xA83515)

    // Pebble (Secondary) - "Ethereal balance"
    static let pebble50  = Color(hex: 0xF7F7F9)
    static let pebble100 = Color(hex: 0xE8E8ED)
    static let pebble200 = Color(hex: 0xD1D2DC)
    static let pebble300 = Color(hex: 0xBABCCB)
    static let pebble400 = Color(hex: 0x9C9DAB)
    static let pebble500 = Color(hex: 0x8A8B9A)
    static let pebble600 = Color(hex: 0x757689)
    static let pebble700 = Color(hex: 0x5E5F6E)
    static let pebble800 = Color(hex: 0x474854)
    static let pebble900 = Color(hex: 0x30313A)

    // Bark (Tertiary) - "Grounded structure"
    static let bark50  = Color(hex: 0xF8F5F4)
    static let bark100 = Color(hex: 0xEADED9)
    static let bark200 = Color(hex: 0xD4BCAF)
    static let bark300 = Color(hex: 0xC29A8D)
    static let bark400 = Color(hex: 0xB0877C)
    static let bark500 = Color(hex: 0x9E7469)
    static let bark600 = Color(hex: 0x8A6055)
    static let bark700 = Color(hex: 0x704D43)
    static let bark800 = Color(hex: 0x583A32)
    static let bark900 = Color(hex: 0x3D2821)

    // Grove (Tertiary) - "Organic calming"
    static let grove50  = Color(hex: 0xF7F6F0)
    static let grove100 = Color(hex: 0xE8E4D4)
    static let grove200 = Color(hex: 0xD3CCB0)
    static let grove300 = Color(hex: 0xBEB48C)
    static let grove400 = Color(hex: 0xAEA375)
    static let grove500 = Color(hex: 0x9E925E)
    static let grove600 = Color(hex: 0x8A7F4C)
    static let grove700 = Color(hex: 0x726B3C)
    static let grove800 = Color(hex: 0x5A542D)
    static let grove900 = Color(hex: 0x3F3B1F)

    // Neutral - "Focus and clarity"
    static let neutral0   = Color(hex: 0xFAFAFA)
    static let neutral50  = Color(hex: 0xF5F5F5)
    static let neutral100 = Color(hex: 0xE5E5E5)
    static let neutral200 = Color(hex: 0xD4D4D4)
    static let neutral300 = Color(hex: 0xA3A3A3)
    static let neutral400 = Color(hex: 0x737373)
    static let neutral500 = Color(hex: 0x525252)
    static let neutral600 = Color(hex: 0x404040)
    static let neutral700 = Color(hex: 0x262626)
    static let neutral800 = Color(hex: 0x171717)
    static let neutral900 = Color(hex: 0x0A0A0A)

    // Semantic States
    static let mapleSuccess = Color(hex: 0x7B8F4A)
    static let mapleWarning = Color(hex: 0xD4A35A)
    static let mapleError   = Color(hex: 0xD05E41)
    static let mapleInfo    = Color(hex: 0x7E8DA1)

    // Semantic aliases
    static let maplePrimary          = Color.maple500
    static let mapleOnPrimary        = Color.white
    static let maplePrimaryContainer = Color.maple100
    static let mapleSecondary          = Color.pebble500
    static let mapleOnSecondary        = Color.white
    static let mapleSecondaryContainer = Color.pebble100
    static let mapleTertiary          = Color.bark500
    static let mapleOnTertiary        = Color.white
    static let mapleTertiaryContainer = Color.bark100

    init(hex: UInt, opacity: Double = 1.0) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
        )
    }
}

// MARK: - Typography

enum MapleFont {
    static func regular(_ size: CGFloat) -> Font { .custom("Manrope-Regular", size: size) }
    static func medium(_ size: CGFloat) -> Font { .custom("Manrope-Medium", size: size) }
    static func semiBold(_ size: CGFloat) -> Font { .custom("Manrope-SemiBold", size: size) }
    static func bold(_ size: CGFloat) -> Font { .custom("Manrope-Bold", size: size) }
    static func display(_ size: CGFloat) -> Font { .custom("Array-Bold", size: size) }

    static let title = semiBold(22)
    static let titleMedium = medium(16)
    static let bodyLarge = regular(16)
    static let body = regular(14)
    static let label = medium(14)
    static let caption = regular(12)
    static let captionSmall = regular(10)
}

// MARK: - Spacing

enum MapleSpacing {
    static let xs:  CGFloat = 8
    static let sm:  CGFloat = 12
    static let md:  CGFloat = 20
    static let lg:  CGFloat = 36
    static let xl:  CGFloat = 56
    static let xxl: CGFloat = 88
}

// MARK: - Border Radius

enum MapleRadius {
    static let full: CGFloat = 999
    static let card: CGFloat = 44
    static let xl:   CGFloat = 24
    static let lg:   CGFloat = 16
    static let md:   CGFloat = 12
    static let sm:   CGFloat = 8
}

// MARK: - Glassy Button Style

struct MapleButtonStyle: ButtonStyle {
    enum Size { case small, medium, large }
    let size: Size

    private var fontSize: CGFloat {
        switch size {
        case .small: return 14
        case .medium: return 16
        case .large: return 18
        }
    }

    private var verticalPadding: CGFloat { 8 }
    private var horizontalPadding: CGFloat {
        switch size {
        case .small, .medium: return 20
        case .large: return 28
        }
    }

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.custom("Manrope-Medium", size: fontSize))
            .foregroundStyle(Color.pebble800)
            .padding(.horizontal, horizontalPadding)
            .padding(.vertical, verticalPadding)
            .glassEffect(.regular.interactive())
            .clipShape(Capsule())
            .scaleEffect(configuration.isPressed ? 0.95 : 1.0)
            .animation(.easeOut(duration: 0.2), value: configuration.isPressed)
    }
}

// MARK: - Primary Button Style

struct MaplePrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.custom("Manrope-SemiBold", size: 16))
            .foregroundStyle(.white)
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
            .background(
                LinearGradient(
                    colors: [Color.maple400, Color.maple600],
                    startPoint: .top,
                    endPoint: .bottom
                )
            )
            .clipShape(Capsule())
            .glassEffect()
            .opacity(isEnabled ? 1.0 : 0.5)
            .scaleEffect(configuration.isPressed ? 0.95 : 1.0)
            .animation(.easeOut(duration: 0.2), value: configuration.isPressed)
    }
}

// MARK: - Secondary Button Style

struct MapleSecondaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.custom("Manrope-Medium", size: 16))
            .foregroundStyle(Color.pebble700)
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
            .glassEffect(.regular.interactive())
            .clipShape(Capsule())
            .opacity(isEnabled ? 1.0 : 0.5)
            .scaleEffect(configuration.isPressed ? 0.95 : 1.0)
            .animation(.easeOut(duration: 0.2), value: configuration.isPressed)
    }
}
