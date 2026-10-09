#if os(iOS)
    import MapleAppCore
    import SwiftUI

    /// Settings is pushed by the compact menu and covered in its own native stack
    /// on a wide window. Each account operation owns a new revocable ceremony.
    struct SettingsScreen: View {
        @Bindable var environment: AppEnvironment
        let context: LibraryContext
        @State private var destination: Destination?
        @State private var confirmDeleteHistory = false
        @State private var externalLink: URL?

        private enum Destination: Identifiable {
            case instructions(InstructionEditor), account(AccountFlow)
            var id: String {
                switch self {
                case .instructions: "instructions"
                case .account(let flow): flow.id.uuidString
                }
            }
        }

        var body: some View {
            Group {
                if let preferences = environment.preferences, let entitlements = environment.entitlements,
                    let user = environment.session.account?.user
                {
                    SettingsView(
                        preferences: preferences, entitlements: entitlements,
                        account: SettingsAccount(
                            email: user.email, isVerified: user.emailVerified,
                            isGuest: user.loginMethod == "guest",
                            canChangePassword: ["email", "guest"].contains(user.loginMethod.lowercased())),
                        version: version, onAction: handle)
                } else {
                    MapleSurface.canvas
                }
            }
            .navigationDestination(
                isPresented: Binding(
                    get: { destination != nil }, set: { if !$0 { destination = nil } }
                )
            ) {
                switch destination {
                case .instructions(let editor): InstructionEditorView(editor: editor)
                case .account(let flow): AccountFlowContent(flow: flow, session: environment.session)
                case nil: EmptyView()
                }
            }
            .confirmationDialog(
                MapleStrings.text("Delete all chat history?"), isPresented: $confirmDeleteHistory,
                titleVisibility: .visible
            ) {
                Button(MapleStrings.text("Delete all chats"), role: .destructive) {
                    Task { await context.root.deleteAllConversations() }
                }
            } message: {
                Text(
                    MapleStrings.text(
                        "This permanently deletes your chats and their saved drafts and queued messages. Active requests must stop before deletion can finish."
                    )
                )
            }
            .overlay(alignment: .bottom) {
                if let issue = context.root.issue { LibraryIssueView(issue: issue).padding().background(.bar) }
            }
            .externalLinks($externalLink)
            .safeAreaInset(edge: .bottom) {
                if environment.settingsPresented, let listening = environment.listening {
                    CompactReplyPlayer(session: listening).padding(.horizontal, 8)
                }
            }
        }

        private func handle(_ action: SettingsAction) {
            switch action {
            case .accountInstructions: destination = .instructions(context.makeInstructionEditor(.account))
            case .verifyEmail: open(.verification)
            case .changePassword: open(.changePassword)
            case .resetPassword: open(.reset)
            case .deleteAccount: open(.deletion)
            case .deleteAllHistory: confirmDeleteHistory = true
            case .signOut: Task { await environment.session.signOut() }
            case .externalURL(let url): externalLink = url
            }
        }

        private func open(_ screen: AccountScreen) {
            destination = .account(AccountFlow(screen: screen, ceremonies: environment.makeCeremony(), code: nil))
        }

        private var version: String {
            let bundle = Bundle.main
            return
                "\(bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "") (\(bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? ""))"
        }
    }
#endif
