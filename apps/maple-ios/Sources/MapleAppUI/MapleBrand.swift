#if os(iOS)
    import SwiftUI
    import UIKit

    @MainActor enum MapleAccent {
        // https://www.trymaple.ai/brand: Maple 500 is the core accent. Use
        // supplied logo colors for contrast instead of a darker primary shade.
        static let prominent = Color(red: 1, green: 151 / 255, blue: 113 / 255)
        static let onProminent = Color(red: 23 / 255, green: 23 / 255, blue: 23 / 255)
        static let foreground = prominent
        static let action = actionColor()

        // System-owned filled controls can choose a white label. Use the
        // documented dark ground there; custom primary buttons use peach with
        // dark text. Both treatments preserve readable labels.
        static let nativeFilled = onProminent

        nonisolated private static func actionColor() -> Color {
            Color(
                uiColor: UIColor { traits in
                    if traits.accessibilityContrast == .high {
                        return traits.userInterfaceStyle == .dark
                            ? UIColor(red: 247 / 255, green: 247 / 255, blue: 249 / 255, alpha: 1)
                            : UIColor(red: 23 / 255, green: 23 / 255, blue: 23 / 255, alpha: 1)
                    }
                    return UIColor(red: 1, green: 151 / 255, blue: 113 / 255, alpha: 1)
                })
        }

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
        static let canvas = MapleAccent.adaptive(light: 0xFF_FF_FF, dark: 0x00_00_00)
        static let card = MapleAccent.adaptive(light: 0xFA_FA_FA, dark: 0x26_26_26)
        static let sidebar = canvas
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
                .foregroundStyle(MapleAccent.adaptive(light: 0x17_17_17, dark: 0xF7_F7_F9))
        }
    }
#endif
