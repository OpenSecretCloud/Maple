#if os(iOS)
    import SwiftUI
    import UIKit

    @MainActor enum MapleAccent {
        // Maple 500 is the fill. Dark text keeps that exact brand color readable.
        static let prominent = Color(red: 1, green: 151 / 255, blue: 113 / 255)
        static let onProminent = Color(red: 23 / 255, green: 23 / 255, blue: 23 / 255)
        static let foreground = adaptive(light: 0xA8_35_15, dark: 0xFF_97_71)

        // System-owned filled controls choose their own white label. Keep their
        // behavior and use the contrast-safe darker member of the same palette.
        static let nativeFilled = Color(red: 168 / 255, green: 53 / 255, blue: 21 / 255)

        // UIKit may resolve this pure color provider during background rendering.
        // Keep its closure free of inherited main-actor isolation.
        nonisolated static func adaptive(light: UInt32, dark: UInt32) -> Color {
            Color(
                uiColor: UIColor { traits in
                    let value = traits.userInterfaceStyle == .dark ? dark : light
                    return UIColor(
                        red: CGFloat((value >> 16) & 255) / 255,
                        green: CGFloat((value >> 8) & 255) / 255,
                        blue: CGFloat(value & 255) / 255, alpha: 1)
                })
        }
    }

    @MainActor enum MapleSurface {
        static let canvas = MapleAccent.adaptive(light: 0xFF_FF_FF, dark: 0x0A_0A_0A)
        static let card = MapleAccent.adaptive(light: 0xFA_FA_FA, dark: 0x26_26_26)
        static let sidebar = MapleAccent.adaptive(light: 0xF7_F7_F9, dark: 0x30_31_3A)
        static let border = MapleAccent.adaptive(light: 0xD1_D2_DC, dark: 0x5E_5F_6E)
        static let displayText = MapleAccent.adaptive(light: 0x47_48_54, dark: 0xBA_BC_CB)
    }

    @MainActor enum MapleTypography {
        static let body = Font.custom("Manrope-Regular", size: 17, relativeTo: .body)
        static let title = Font.custom("Manrope-Regular", size: 22, relativeTo: .title2).weight(.semibold)
        static let button = Font.custom("Manrope-Regular", size: 17, relativeTo: .body).weight(.medium)
        static let inlineCode = Font.custom("GeistMono-Regular", size: 16, relativeTo: .body)
        static let code = Font.custom("GeistMono-Regular", size: 15, relativeTo: .footnote)
        static let metadata = Font.custom("GeistMono-Regular", size: 12, relativeTo: .caption)
        static let display = Font.custom("Array-Regular", size: 34, relativeTo: .largeTitle)

    }

    struct MaplePrimaryButtonStyle: ButtonStyle {
        var compact = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        func makeBody(configuration: Configuration) -> some View {
            configuration.label
                .font(MapleTypography.button)
                .padding(.horizontal, compact ? 0 : 20)
                .padding(.vertical, compact ? 0 : 8)
                .frame(minWidth: 44, minHeight: 44)
                .foregroundStyle(MapleAccent.onProminent)
                .background(MapleAccent.prominent, in: Capsule())
                .scaleEffect(configuration.isPressed && !reduceMotion ? 0.95 : 1)
                // Keep the hit region full-sized even while the pixels compress.
                .frame(minWidth: 44, minHeight: 44)
                .contentShape(Capsule())
                .opacity(isEnabled ? 1 : 0.4)
                .animation(reduceMotion ? nil : .easeOut(duration: 0.2), value: configuration.isPressed)
        }
    }

    struct MapleWordmark: View {
        var body: some View {
            Image("MapleWordmark", bundle: .module)
                .renderingMode(.template).resizable().scaledToFit()
                .foregroundStyle(.primary)
        }
    }
#endif
