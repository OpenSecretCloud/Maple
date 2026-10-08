#if os(iOS)
    import Foundation
    import SwiftUI
    import Textual
    import UIKit
    import UniformTypeIdentifiers

    public enum MessageRenderTheme: String, Sendable { case automatic, light, dark }
    public enum MessageRenderFont: String, Sendable { case manrope, system, serif }

    private struct RetainedMessageRenderingKey: EnvironmentKey {
        static let defaultValue: @MainActor @Sendable () -> Bool = { false }
    }

    extension EnvironmentValues {
        var retainedMessageRendering: @MainActor @Sendable () -> Bool {
            get { self[RetainedMessageRenderingKey.self] }
            set { self[RetainedMessageRenderingKey.self] = newValue }
        }
    }

    /// Shared message surface for every supported iOS version. Its parser owns no network capability.
    public struct MessageView: View {
        private let text: String
        private let theme: MessageRenderTheme
        private let font: MessageRenderFont
        private let accountGeneration: UUID?
        private let onOpenLink: (URL) -> Void
        #if DEBUG
            private var diagnosticAppearance: ((Bool) -> Void)?
            private var diagnosticPublication: ((Int, Int) -> Void)?
        #endif
        @Environment(\.colorScheme) private var colorScheme
        @Environment(\.scenePhase) private var scenePhase
        @Environment(\.legibilityWeight) private var legibilityWeight
        @Environment(\.retainedMessageRendering) private var retainsRendering
        @State private var windowHost: EquationLifecycleWindowHost
        @State private var model: MessageRenderModel

        public init(
            text: String, theme: MessageRenderTheme = .automatic,
            font: MessageRenderFont = .manrope, accountGeneration: UUID? = nil,
            onOpenLink: @escaping (URL) -> Void
        ) {
            self.text = text
            self.theme = theme
            self.font = font
            self.accountGeneration = accountGeneration
            self.onOpenLink = onOpenLink
            let host = EquationLifecycleWindowHost(frame: .zero)
            _windowHost = State(initialValue: host)
            _model = State(
                initialValue: MessageRenderModel(equations: { formulas in
                    let renderer = EquationRenderer(host: host)
                    defer { renderer.invalidate() }
                    return try await renderer.render(formulas).mapValues { AnyAttachment($0) }
                }))
        }

        #if DEBUG
            // The fixture's observed renderer uses the same host mounted by this
            // production body. Its gate only controls when synthetic work begins.
            init(
                text: String, accountGeneration: UUID, diagnosticModel: MessageRenderModel,
                diagnosticHost: EquationLifecycleWindowHost,
                diagnosticAppearance: @escaping (Bool) -> Void
            ) {
                self.init(text: text, theme: .light, accountGeneration: accountGeneration) { _ in }
                _model = State(initialValue: diagnosticModel)
                _windowHost = State(initialValue: diagnosticHost)
                self.diagnosticAppearance = diagnosticAppearance
            }

            // Observes publication through the unmodified production initializer;
            // no injected parser, renderer, host, clock or equation warm-up.
            init(text: String, accountGeneration: UUID, diagnosticPublication: @escaping (Int, Int) -> Void) {
                self.init(text: text, theme: .light, accountGeneration: accountGeneration) { _ in }
                self.diagnosticPublication = diagnosticPublication
            }
        #endif

        public var body: some View {
            Group {
                if let prepared = model.prepared, model.generation == accountGeneration {
                    StructuredText(prepared.presentationKey, parser: PreparedMarkup(content: prepared.content))
                        .textual.textSelection(.enabled)
                        .textual.inlineStyle(
                            .default.code(.font(MapleTypography.inlineCode))
                                .link(.foregroundColor(MapleAccent.foreground))
                        )
                        .textual.imageAttachmentLoader(NoMessageAttachments())
                        .textual.emojiAttachmentLoader(NoMessageAttachments())
                        .textual.codeBlockStyle(MessageCodeStyle(blocks: prepared.codeBlocks))
                        .textual.tableStyle(MessageTableStyle())
                        .textual.tableCellStyle(MessageTableCellStyle())
                        .scrollIndicators(.visible)
                        .accessibilityElement(children: .contain)
                        .accessibilityLabel(Text("Message", bundle: .module))
                        .accessibilityCustomContent(
                            AccessibilityCustomContentKey(Text("Equations", bundle: .module), id: "maple.equations"),
                            equationAccessibilityText(prepared), importance: .high)
                } else {
                    Text(text).textSelection(.enabled)
                }
            }
            .font(messageFont)
            .fontWeight(legibilityWeight == .bold ? .semibold : .regular)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background {
                EquationLifecycleWindowAnchor(host: windowHost)
                    .frame(width: 0, height: 0)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
            .environment(\.colorScheme, theme == .automatic ? colorScheme : theme == .dark ? .dark : .light)
            .environment(
                \.openURL,
                OpenURLAction { url in
                    guard ["https", "http"].contains(url.scheme?.lowercased() ?? ""),
                        url.host != nil, url.user == nil, url.password == nil
                    else { return .discarded }
                    onOpenLink(url)
                    return .handled
                }
            )
            .onAppear {
                #if DEBUG
                    diagnosticAppearance?(true)
                #endif
                model.submit(text: text, generation: accountGeneration)
                windowHost.resumeMount()
            }
            .onChange(of: Request(text: text, generation: accountGeneration)) {
                model.submit(text: text, generation: accountGeneration)
            }
            .onChange(of: scenePhase) {
                if scenePhase == .active {
                    windowHost.resumeMount()
                    model.submit(text: text, generation: accountGeneration)
                } else {
                    model.suspend()
                    windowHost.renderer?.invalidate()
                }
            }
            #if DEBUG
                .onChange(of: model.prepared?.presentationKey) {
                    if let diagnosticPublication {
                        let attachments = model.prepared?.content.runs.compactMap { $0.textual.attachment } ?? []
                        let ink = attachments.compactMap { $0.pngData() }.filter(
                            EquationDiagnosticView.hasVisibleGlyphs
                        ).count
                        diagnosticPublication(attachments.count, ink)
                    }
                }
            #endif
            .onDisappear {
                #if DEBUG
                    diagnosticAppearance?(false)
                #endif
                if retainsRendering() { model.suspend() } else { model.cancel() }
                windowHost.renderer?.invalidate()
            }
        }

        private struct Request: Equatable { let text: String; let generation: UUID? }
        private func equationAccessibilityText(_ prepared: PreparedMessage) -> Text? {
            // Accessibility must not wait for rasterization or disappear when an
            // unsupported equation stays as its original selectable source.
            let sources = prepared.equationSources
            return sources.isEmpty ? nil : Text(verbatim: sources.joined(separator: "; "))
        }
        private var messageFont: Font {
            switch font {
            case .manrope: MapleTypography.body
            case .system: .body
            case .serif: .system(.body, design: .serif)
            }
        }
    }

    private struct PreparedMarkup: MarkupParser {
        let content: AttributedString
        func attributedString(for input: String) throws -> AttributedString { content }
    }

    /// Measure bounded cells at their ideal width before the horizontal scroller
    /// proposes its viewport width. A max-width-only table permits Grid to squeeze
    /// every column into the viewport, even at accessibility text sizes.
    private struct MessageTableStyle: StructuredText.TableStyle {
        func makeBody(configuration: Configuration) -> some View {
            Overflow {
                configuration.label
                    .fixedSize(horizontal: true, vertical: true)
                    .textual.tableOverlay { layout in
                        Canvas { context, _ in
                            for divider in layout.dividers() {
                                context.fill(Path(divider), with: .color(.secondary.opacity(0.2)))
                            }
                        }
                    }
                    .padding(1)
            }
            .textual.tableCellSpacing(horizontal: 1, vertical: 1)
            .textual.blockSpacing(.fontScaled(top: 1.6, bottom: 1.6))
        }
    }

    private struct MessageTableCellStyle: StructuredText.TableCellStyle {
        @ScaledMetric(relativeTo: .body) private var maximumWidth: CGFloat = 280
        func makeBody(configuration: Configuration) -> some View {
            BoundedTableCellLayout(maximumWidth: maximumWidth) {
                configuration.label
                    .fontWeight(configuration.row == 0 ? .semibold : .regular)
                    .textual.lineSpacing(.fontScaled(0.471))
                    .textual.padding(.fontScaled(0.588))
            }
        }
    }

    /// Report the cell's bounded intrinsic width instead of expanding a frame
    /// to Grid's column proposal. Grid then retains the Markdown column's own
    /// leading, center, or trailing alignment for shorter cell content.
    private struct BoundedTableCellLayout: Layout {
        let maximumWidth: CGFloat

        func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
            guard let cell = subviews.first else { return .zero }
            let width = min(maximumWidth, cell.sizeThatFits(.unspecified).width)
            return cell.sizeThatFits(ProposedViewSize(width: width, height: nil))
        }

        func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
            guard let cell = subviews.first else { return }
            cell.place(
                at: bounds.origin, anchor: .topLeading,
                proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
        }
    }

    private struct MessageCodeStyle: StructuredText.CodeBlockStyle {
        let blocks: [String: PreparedCode]
        func makeBody(configuration: Configuration) -> some View {
            if let key = configuration.languageHint, let code = blocks[key] {
                VStack(alignment: .leading, spacing: 8) {
                    // Header height follows actual Dynamic Type content. Keeping
                    // it outside Overflow avoids covering the first code line.
                    HStack {
                        Text(code.block.language).font(MapleTypography.metadata).foregroundStyle(.secondary)
                        Spacer()
                        Button {
                            UIPasteboard.general.setItems(
                                [[UTType.utf8PlainText.identifier: code.block.source]],
                                options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(120)])
                        } label: {
                            Image(systemName: "document.on.document")
                                .frame(minWidth: 44, minHeight: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .textual.textSelectionInteractionExcluded()
                        .accessibilityLabel(Text("Copy code", bundle: .module))
                    }
                    Overflow {
                        InlineText(code.block.source, parser: PreparedMarkup(content: highlighted(code.tokens)))
                            .textual.textSelection(.enabled)
                            .font(MapleTypography.code)
                            .fixedSize(horizontal: true, vertical: false)
                    }
                    .scrollIndicators(.visible)
                    .accessibilityIdentifier("message.code.scroll")
                }
                .padding(12)
                .background(MapleSurface.card, in: RoundedRectangle(cornerRadius: 12))
            } else {
                configuration.label
            }
        }

        private func highlighted(_ tokens: [MessageCodeToken]) -> AttributedString {
            var result = AttributedString()
            for token in tokens {
                var part = AttributedString(token.text)
                switch token.kind {
                case .plain: break
                case .keyword: part.foregroundColor = codeColor(light: (0.45, 0.18, 0.65), dark: (0.80, 0.63, 1.0))
                case .string: part.foregroundColor = codeColor(light: (0.03, 0.39, 0.14), dark: (0.48, 0.87, 0.56))
                case .comment: part.foregroundColor = .secondary
                case .number: part.foregroundColor = codeColor(light: (0.53, 0.25, 0.01), dark: (1.0, 0.72, 0.42))
                }
                result.append(part)
            }
            return result
        }

        private func codeColor(
            light: (CGFloat, CGFloat, CGFloat), dark: (CGFloat, CGFloat, CGFloat)
        ) -> Color {
            Color(
                uiColor: UIColor { traits in
                    let color = traits.userInterfaceStyle == .dark ? dark : light
                    return UIColor(red: color.0, green: color.1, blue: color.2, alpha: 1)
                })
        }
    }

    private struct NoMessageAttachments: AttachmentLoader {
        struct Rejected: Error {}
        func attachment(for url: URL, text: String, environment: ColorEnvironmentValues) async throws -> NeverAttachment
        {
            throw Rejected()
        }
    }

    private struct NeverAttachment: Attachment {
        var description: String { "" }
        var body: some View { EmptyView() }
        func sizeThatFits(_ proposal: ProposedViewSize, in environment: TextEnvironmentValues) -> CGSize { .zero }
    }
#endif
