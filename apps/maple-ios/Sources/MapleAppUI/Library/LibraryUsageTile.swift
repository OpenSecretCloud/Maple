#if os(iOS)
    import MapleAppCore
    import SwiftUI

    /// Sidebar billing summary. It deliberately has no purchase or pricing action.
    public struct LibraryUsageTile: View {
        private let account: AccountEntitlements
        @Environment(\.locale) private var locale
        @Environment(\.calendar) private var calendar

        public init(account: AccountEntitlements) { self.account = account }

        public var body: some View {
            let summary = account.billing.value?.usageSummary(calendar: calendar)
            VStack(alignment: .leading, spacing: 6) {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) {
                        plan(summary?.planLabel ?? .loading)
                        if let meter = summary?.meter {
                            separator
                            Text(verbatim: LibraryUsagePresentation.percent(meter.roundedPercentUsed, locale: locale))
                                .fontWeight(.semibold).monospacedDigit()
                            separator
                            reset(meter.reset)
                        }
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        plan(summary?.planLabel ?? .loading)
                        if let meter = summary?.meter {
                            Text(verbatim: LibraryUsagePresentation.percent(meter.roundedPercentUsed, locale: locale))
                                .fontWeight(.semibold).monospacedDigit()
                            reset(meter.reset)
                        }
                    }
                }
                if let meter = summary?.meter {
                    GeometryReader { geometry in
                        Capsule().fill(.quaternary)
                            .overlay(alignment: .leading) {
                                Capsule().fill(
                                    LinearGradient(
                                        colors: [
                                            MapleAccent.prominent, Color(red: 0.9076, green: 0.43804, blue: 0.2524),
                                        ],
                                        startPoint: .leading, endPoint: .trailing)
                                )
                                .frame(width: geometry.size.width * meter.percentUsed / 100)
                            }
                    }
                    .frame(height: 4).accessibilityHidden(true)
                    if let credits = meter.creditBalance {
                        Text(verbatim: LibraryUsagePresentation.credits(credits, locale: locale))
                            .monospacedDigit()
                            .foregroundStyle(MapleAccent.adaptive(light: 0x7B_8F_4A, dark: 0x87_A2_53))
                    }
                }
            }
            .font(.caption2)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(MapleSurface.card, in: RoundedRectangle(cornerRadius: 12))
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("library.usage")
        }

        private func plan(_ label: BillingUsageSummary.PlanLabel) -> some View {
            Text(verbatim: LibraryUsagePresentation.plan(label, locale: locale))
                .fontWeight(.semibold).textCase(.uppercase)
                .padding(.horizontal, 6).padding(.vertical, 3)
                .background(.quaternary, in: Capsule())
                .overlay(Capsule().strokeBorder(MapleSurface.border.opacity(0.5)))
        }

        private var separator: some View {
            Text(verbatim: "·").foregroundStyle(.tertiary).accessibilityHidden(true)
        }

        private func reset(_ value: BillingUsageReset) -> some View {
            Text(verbatim: LibraryUsagePresentation.reset(value, locale: locale, calendar: calendar))
                .foregroundStyle(.secondary)
        }
    }

    public struct LibraryAccountAttentionBadge: View {
        private let account: AccountEntitlements
        public init(account: AccountEntitlements) { self.account = account }

        public var body: some View {
            if let attention = LibraryAccountAttention(team: account.team.value) {
                Image(systemName: "exclamationmark.circle.fill")
                    .foregroundStyle(attention == .usagePaused ? Color.red : Color.orange)
                    .accessibilityLabel(Text(verbatim: attention.accessibilityLabel))
                    .accessibilityIdentifier("library.settings-attention")
            }
        }
    }
#endif
