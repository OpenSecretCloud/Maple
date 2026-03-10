import SwiftUI

struct ContentView: View {
    @Bindable var manager: AppManager
    @State private var showSplash = true
    @State private var splashMinTimePassed = false
    @State private var toastDismissTask: Task<Void, Never>?

    var body: some View {
        ZStack {
            switch manager.state.router.defaultScreen {
            case .loading:
                SplashView()
            case .login:
                LoginView(manager: manager)
            case .chat:
                AgentChatView(manager: manager)
            }

            if showSplash {
                SplashView()
                    .transition(.opacity)
                    .zIndex(1)
            }
        }
        .overlay(alignment: .bottom) {
            MapleToastOverlay(
                message: manager.state.toast,
                screen: manager.state.router.defaultScreen,
                onDismiss: { manager.dispatch(.clearToast) }
            )
        }
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                splashMinTimePassed = true
                dismissSplashIfReady()
            }
        }
        .onChange(of: manager.state.router.defaultScreen) { _, _ in
            dismissSplashIfReady()
        }
        .onChange(of: manager.state.toast) { _, newToast in
            toastDismissTask?.cancel()
            guard let newToast else { return }

            toastDismissTask = Task {
                try? await Task.sleep(for: .seconds(4))
                guard !Task.isCancelled, manager.state.toast == newToast else { return }
                manager.dispatch(.clearToast)
            }
        }
        .onDisappear {
            toastDismissTask?.cancel()
        }
    }

    private func dismissSplashIfReady() {
        guard splashMinTimePassed else { return }
        guard manager.state.router.defaultScreen != .loading else { return }
        withAnimation(.easeOut(duration: 0.4)) {
            showSplash = false
        }
    }
}

private struct MapleToastOverlay: View {
    @Environment(\.colorScheme) private var colorScheme

    let message: String?
    let screen: Screen
    let onDismiss: () -> Void

    private var bottomPadding: CGFloat {
        screen == .chat ? 96 : 28
    }

    private var textColor: Color {
        colorScheme == .dark ? .pebble50 : .neutral800
    }

    private var borderColor: Color {
        Color.mapleError.opacity(colorScheme == .dark ? 0.35 : 0.18)
    }

    var body: some View {
        if let message {
            Text(message)
                .font(MapleFont.caption)
                .foregroundStyle(textColor)
                .multilineTextAlignment(.center)
                .padding(.horizontal, MapleSpacing.md)
                .padding(.vertical, MapleSpacing.sm)
                .background(.ultraThinMaterial, in: Capsule())
                .overlay {
                    Capsule().stroke(borderColor, lineWidth: 1)
                }
                .padding(.horizontal, MapleSpacing.md)
                .padding(.bottom, bottomPadding)
                .onTapGesture(perform: onDismiss)
                .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }
}

// MARK: - Splash

struct SplashView: View {
    var body: some View {
        ZStack {
            RadialGradient(
                colors: [
                    Color(hex: 0xFF9771),
                    Color(hex: 0xCE9A8E),
                    Color(hex: 0x9C9DAB),
                ],
                center: .bottom,
                startRadius: 0,
                endRadius: 600
            )
            .ignoresSafeArea()

            VStack(spacing: MapleSpacing.lg) {
                MapleWordmark(color: .pebble50, height: 40)

                Text("Privacy-first intelligence")
                    .font(MapleFont.display(16))
                    .foregroundStyle(Color.pebble100.opacity(0.8))
            }
        }
    }
}

// MARK: - Login

struct LoginView: View {
    @Bindable var manager: AppManager
    @Environment(\.colorScheme) private var colorScheme
    @State private var email = ""
    @State private var password = ""
    @State private var name = ""
    @State private var isSignUp = false
    @FocusState private var focusedField: LoginField?

    private enum LoginField {
        case name, email, password
    }

    private struct LoginPalette {
        let backgroundBase: Color
        let backgroundGlow: [Color]
        let cardBackground: Color
        let cardHighlight: Color
        let cardBorder: Color
        let cardShadow: Color
        let wordmark: Color
        let supportingText: Color
        let tertiaryText: Color
        let divider: Color
        let fieldBackground: Color
        let fieldBorder: Color
        let fieldText: Color
        let fieldPlaceholder: Color
        let secondaryButtonBackground: Color
        let secondaryButtonBorder: Color
        let secondaryButtonForeground: Color
        let secondaryButtonShadow: Color
    }

    private var isLoading: Bool {
        switch manager.state.auth {
        case .loggingIn, .signingUp:
            return true
        default:
            return false
        }
    }

    private var loginPalette: LoginPalette {
        if colorScheme == .dark {
            LoginPalette(
                backgroundBase: Color(hex: 0x1A110E),
                backgroundGlow: [
                    Color.maple500.opacity(0.07),
                    Color(hex: 0x5D4036, opacity: 0.11),
                    Color.pebble800.opacity(0.12),
                    .clear,
                ],
                cardBackground: Color(hex: 0x271D1A, opacity: 0.9),
                cardHighlight: Color.white.opacity(0.04),
                cardBorder: Color(hex: 0x53433E),
                cardShadow: Color.black.opacity(0.4),
                wordmark: .pebble50,
                supportingText: Color(hex: 0xD8C2BB),
                tertiaryText: Color(hex: 0xD8C2BB, opacity: 0.8),
                divider: Color(hex: 0x53433E),
                fieldBackground: Color(hex: 0x231A16, opacity: 0.96),
                fieldBorder: Color(hex: 0xA08D86, opacity: 0.45),
                fieldText: Color(hex: 0xF1DFD9),
                fieldPlaceholder: Color(hex: 0xD8C2BB, opacity: 0.75),
                secondaryButtonBackground: Color(hex: 0x322824, opacity: 0.96),
                secondaryButtonBorder: Color(hex: 0x53433E),
                secondaryButtonForeground: Color(hex: 0xF1DFD9),
                secondaryButtonShadow: Color.black.opacity(0.14)
            )
        } else {
            LoginPalette(
                backgroundBase: Color(hex: 0xFBF8F6),
                backgroundGlow: [
                    Color.maple500.opacity(0.18),
                    Color.bark300.opacity(0.1),
                    Color.pebble300.opacity(0.1),
                    .clear,
                ],
                cardBackground: Color.white.opacity(0.74),
                cardHighlight: Color.white.opacity(0.42),
                cardBorder: Color.white.opacity(0.72),
                cardShadow: Color.pebble900.opacity(0.08),
                wordmark: .pebble800,
                supportingText: .pebble600,
                tertiaryText: .pebble400,
                divider: .neutral200,
                fieldBackground: Color.white.opacity(0.84),
                fieldBorder: Color.neutral200.opacity(0.95),
                fieldText: .pebble800,
                fieldPlaceholder: .pebble400,
                secondaryButtonBackground: Color.white.opacity(0.56),
                secondaryButtonBorder: Color.white.opacity(0.68),
                secondaryButtonForeground: .pebble700,
                secondaryButtonShadow: Color.pebble900.opacity(0.05)
            )
        }
    }

    private var loginCardShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: MapleRadius.xl, style: .continuous)
    }

    private var loginFieldShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: MapleRadius.md, style: .continuous)
    }

    var body: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(spacing: 24) {
                MapleWordmark(color: loginPalette.wordmark, height: 28)

                VStack(spacing: MapleSpacing.sm) {
                    if isSignUp {
                        mapleTextField("Name", text: $name)
                            .textContentType(.name)
                            .autocorrectionDisabled()
                            .focused($focusedField, equals: .name)
                            .submitLabel(.next)
                            .onSubmit { focusedField = .email }
                    }

                    mapleTextField("Email", text: $email)
                        .textContentType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.emailAddress)
                        .focused($focusedField, equals: .email)
                        .submitLabel(.next)
                        .onSubmit { focusedField = .password }

                    mapleSecureField("Password", text: $password)
                        .textContentType(isSignUp ? .newPassword : .password)
                        .focused($focusedField, equals: .password)
                        .submitLabel(.go)
                        .onSubmit(submit)
                }

                Button(action: submit) {
                    if isLoading {
                        ProgressView()
                            .tint(.white)
                            .frame(maxWidth: .infinity)
                    } else {
                        Text(isSignUp ? "Sign Up" : "Sign In")
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(MaplePrimaryButtonStyle())
                .disabled(email.isEmpty || password.isEmpty || isLoading)

                dividerWithText("or")

                VStack(spacing: MapleSpacing.xs) {
                    oauthButton(label: "Continue with GitHub", icon: "globe") {
                        manager.dispatch(.initiateOAuth(provider: .github, inviteCode: nil))
                    }
                    oauthButton(label: "Continue with Google", icon: "globe") {
                        manager.dispatch(.initiateOAuth(provider: .google, inviteCode: nil))
                    }
                    oauthButton(label: "Continue with Apple", icon: "applelogo") {
                        manager.dispatch(.initiateOAuth(provider: .apple, inviteCode: nil))
                    }
                }
                .disabled(isLoading)
                .opacity(isLoading ? 0.6 : 1.0)

                Button(isSignUp ? "Already have an account? Sign In" : "Don't have an account? Sign Up") {
                    isSignUp.toggle()
                }
                .font(MapleFont.body)
                .foregroundStyle(loginPalette.supportingText)
            }
            .padding(24)
            .background {
                ZStack {
                    loginCardShape.fill(loginPalette.cardBackground)

                    loginCardShape.fill(
                        LinearGradient(
                            colors: [loginPalette.cardHighlight, Color.white.opacity(0.02)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                }
            }
            .overlay {
                loginCardShape.stroke(loginPalette.cardBorder, lineWidth: 1)
            }
            .shadow(color: loginPalette.cardShadow, radius: 28, y: 20)
            .padding(.horizontal, MapleSpacing.md)

            Spacer()
        }
        .background(
            ZStack {
                loginPalette.backgroundBase

                RadialGradient(
                    colors: loginPalette.backgroundGlow,
                    center: .bottom,
                    startRadius: 0,
                    endRadius: 500
                )
            }
            .ignoresSafeArea()
        )
        .tint(Color.maple500)
    }

    private func submit() {
        if isSignUp {
            manager.dispatch(.signUpWithEmail(email: email, password: password, name: name))
        } else {
            manager.dispatch(.loginWithEmail(email: email, password: password))
        }
    }

    private func mapleTextField(_ placeholder: String, text: Binding<String>) -> some View {
        TextField(
            "",
            text: text,
            prompt: Text(placeholder).foregroundStyle(loginPalette.fieldPlaceholder)
        )
        .font(MapleFont.bodyLarge)
        .foregroundStyle(loginPalette.fieldText)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(loginPalette.fieldBackground, in: loginFieldShape)
        .overlay {
            loginFieldShape.stroke(loginPalette.fieldBorder, lineWidth: 1)
        }
    }

    private func mapleSecureField(_ placeholder: String, text: Binding<String>) -> some View {
        SecureField(
            "",
            text: text,
            prompt: Text(placeholder).foregroundStyle(loginPalette.fieldPlaceholder)
        )
        .font(MapleFont.bodyLarge)
        .foregroundStyle(loginPalette.fieldText)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(loginPalette.fieldBackground, in: loginFieldShape)
        .overlay {
            loginFieldShape.stroke(loginPalette.fieldBorder, lineWidth: 1)
        }
    }

    private func dividerWithText(_ text: String) -> some View {
        HStack {
            Rectangle().frame(height: 1).foregroundStyle(loginPalette.divider)
            Text(text).font(MapleFont.caption).foregroundStyle(loginPalette.tertiaryText)
            Rectangle().frame(height: 1).foregroundStyle(loginPalette.divider)
        }
    }

    private func oauthButton(label: String, icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: MapleSpacing.sm) {
                Image(systemName: icon)
                    .font(.system(size: 16, weight: .medium))
                Text(label)
            }
            .font(MapleFont.medium(16))
            .foregroundStyle(loginPalette.secondaryButtonForeground)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .background(loginPalette.secondaryButtonBackground, in: Capsule())
            .overlay {
                Capsule().stroke(loginPalette.secondaryButtonBorder, lineWidth: 1)
            }
            .shadow(color: loginPalette.secondaryButtonShadow, radius: 10, y: 6)
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Agent Chat

struct ChatPalette {
    let backgroundBase: Color
    let backgroundGlow: [Color]
    let headerWordmark: Color
    let secondaryIcon: Color
    let composeText: Color
    let composePlaceholder: Color
    let metadataText: Color
    let assistantText: Color
    let userBubbleColor: Color
    let userText: Color
    let settingsSecondaryText: Color

    static func forColorScheme(_ colorScheme: ColorScheme) -> ChatPalette {
        if colorScheme == .dark {
            return ChatPalette(
                backgroundBase: Color(hex: 0x1A110E),
                backgroundGlow: [
                    Color.maple500.opacity(0.07),
                    Color(hex: 0x5D4036, opacity: 0.1),
                    Color.pebble800.opacity(0.12),
                    .clear,
                ],
                headerWordmark: .pebble50,
                secondaryIcon: Color(hex: 0xD8C2BB),
                composeText: Color(hex: 0xF1DFD9),
                composePlaceholder: Color(hex: 0xD8C2BB, opacity: 0.78),
                metadataText: Color(hex: 0xD8C2BB, opacity: 0.82),
                assistantText: Color(hex: 0xF1DFD9),
                userBubbleColor: Color(hex: 0x322824, opacity: 0.96),
                userText: Color(hex: 0xF1DFD9),
                settingsSecondaryText: Color(hex: 0xD8C2BB)
            )
        }

        return ChatPalette(
            backgroundBase: .white,
            backgroundGlow: [
                Color(hex: 0xFF9771, opacity: 0.35),
                Color(hex: 0xECB8A5, opacity: 0.2),
                Color(hex: 0xDADADA, opacity: 0.1),
                Color.white.opacity(0),
            ],
            headerWordmark: .pebble800,
            secondaryIcon: .pebble800,
            composeText: .neutral800,
            composePlaceholder: Color(hex: 0x878787),
            metadataText: .pebble400,
            assistantText: .neutral800,
            userBubbleColor: .pebble100,
            userText: .neutral800,
            settingsSecondaryText: .pebble500
        )
    }
}

struct AgentChatView: View {
    @Bindable var manager: AppManager
    @Environment(\.colorScheme) private var colorScheme
    @State private var composeText = ""
    let timestampTimer = Timer.publish(every: 30, on: .main, in: .common).autoconnect()

    private var palette: ChatPalette {
        ChatPalette.forColorScheme(colorScheme)
    }

    var body: some View {
        messageList
            .safeAreaInset(edge: .top) {
                ZStack {
                    GlassEffectContainer {
                        HStack(spacing: 8) {
                            MapleWordmarkAbbr(color: palette.headerWordmark, height: 16)
                            Text("\u{2304}")
                                .font(.system(size: 12, weight: .heavy))
                                .foregroundStyle(Color.pebble400)
                        }
                        .padding(.leading, 16)
                        .padding(.trailing, 12)
                        .padding(.vertical, 12)
                        .glassEffect(in: .capsule)
                    }

                    HStack {
                        GlassEffectContainer {
                            Button {
                                manager.dispatch(.toggleSettings)
                            } label: {
                                Image(systemName: "line.3.horizontal")
                                    .font(.system(size: 18, weight: .bold))
                                    .foregroundStyle(palette.secondaryIcon)
                                    .frame(width: 43, height: 43)
                            }
                            .buttonStyle(.glass)
                        }
                        Spacer()
                        GlassEffectContainer {
                            Button {
                            } label: {
                                Image(systemName: "magnifyingglass")
                                    .font(.system(size: 18, weight: .bold))
                                    .foregroundStyle(palette.secondaryIcon)
                                    .frame(width: 43, height: 43)
                            }
                            .buttonStyle(.glass)
                        }
                    }
                    .padding(.horizontal, MapleSpacing.sm)
                }
            }
            .safeAreaInset(edge: .bottom) {
                composeBar
            }
            .background(
                ZStack {
                    palette.backgroundBase
                    RadialGradient(
                        colors: palette.backgroundGlow,
                        center: .top,
                        startRadius: 0,
                        endRadius: 500
                    )
                }
                .ignoresSafeArea()
            )
        .tint(Color.maple500)
        .onReceive(timestampTimer) { _ in
            manager.dispatch(.refreshTimestamps)
        }
        .sheet(isPresented: Binding(
            get: { manager.state.showSettings },
            set: { if !$0 { manager.dispatch(.toggleSettings) } }
        )) {
            SettingsSheet(manager: manager)
                .presentationDetents([.medium])
        }
        .alert("Delete Agent?", isPresented: Binding(
            get: { manager.state.confirmDeleteAgent },
            set: { if !$0 { manager.dispatch(.cancelDeleteAgent) } }
        )) {
            Button("Cancel", role: .cancel) {
                manager.dispatch(.cancelDeleteAgent)
            }
            Button("Delete", role: .destructive) {
                manager.dispatch(.confirmDeleteAgent)
            }
        } message: {
            Text("This will permanently delete your agent conversation history. This cannot be undone.")
        }
    }

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: MapleSpacing.md) {
                    // Load-more indicator at top
                    if manager.state.isLoadingHistory {
                        ProgressView()
                            .padding(.vertical, 8)
                            .id("loading-history")
                    } else if manager.state.hasOlderMessages {
                        Button("Load older messages") {
                            manager.dispatch(.loadOlderMessages)
                        }
                        .font(MapleFont.caption)
                        .foregroundStyle(Color.maple500)
                        .padding(.vertical, 8)
                        .id("load-more")
                    }

                    ForEach(Array(manager.state.messages.enumerated()), id: \.element.id) { index, message in
                        MessageBubble(message: message, palette: palette)
                            .id(message.id)
                            .onAppear {
                                // Prefetch when within 3 messages of top
                                if index < 3 && manager.state.hasOlderMessages && !manager.state.isLoadingHistory {
                                    manager.dispatch(.loadOlderMessages)
                                }
                            }
                    }

                    if manager.state.isAgentTyping {
                        HStack {
                            Text("Maple is typing...")
                                .font(MapleFont.caption)
                                .foregroundStyle(palette.metadataText)
                            Spacer()
                        }
                        .padding(.horizontal)
                        .id("typing-indicator")
                    }
                }
                .padding()
            }
            .onChange(of: manager.state.messages.count) { oldCount, newCount in
                if newCount > oldCount {
                    if manager.state.isAgentTyping {
                        withAnimation { proxy.scrollTo("typing-indicator", anchor: .bottom) }
                    } else if let last = manager.state.messages.last {
                        withAnimation { proxy.scrollTo(last.id, anchor: .bottom) }
                    }
                }
            }
            .onChange(of: manager.state.messages.last?.content) { _, _ in
                if manager.state.isAgentTyping {
                    withAnimation { proxy.scrollTo("typing-indicator", anchor: .bottom) }
                } else if let last = manager.state.messages.last {
                    proxy.scrollTo(last.id, anchor: .bottom)
                }
            }
            .onChange(of: manager.state.isAgentTyping) { _, isTyping in
                if isTyping {
                    withAnimation { proxy.scrollTo("typing-indicator", anchor: .bottom) }
                }
            }
        }
    }

    private var canSend: Bool {
        !composeText.trimmingCharacters(in: .whitespaces).isEmpty && !manager.state.isAgentTyping
    }

    private var composeBar: some View {
        GlassEffectContainer {
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    TextField(
                        "",
                        text: $composeText,
                        prompt: Text("Write...").foregroundStyle(palette.composePlaceholder)
                    )
                        .font(MapleFont.medium(15))
                        .foregroundStyle(palette.composeText)
                        .onSubmit(sendMessage)

                    Button(action: {}) {
                        Image(systemName: "plus")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(Color.maple500)
                            .frame(width: 24, height: 24)
                            .background(Color.maple500.opacity(0.15), in: Circle())
                    }
                }

                Button(action: sendMessage) {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 20, weight: .heavy))
                        .foregroundStyle(.white.opacity(canSend ? 1.0 : 0.1))
                        .frame(width: 71)
                        .padding(.vertical, 8)
                        .background(
                            LinearGradient(
                                colors: [
                                    Color.maple500.opacity(canSend ? 1.0 : 0.5),
                                    Color.maple700.opacity(canSend ? 1.0 : 0.5),
                                ],
                                startPoint: .top,
                                endPoint: .bottom
                            ),
                            in: Capsule()
                        )
                }
                .disabled(!canSend)
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 14)
            .glassEffect(in: RoundedRectangle(cornerRadius: MapleRadius.xl, style: .continuous))
        }
        .padding(.horizontal, 16)
        .padding(.bottom, MapleSpacing.xs)
    }

    private func sendMessage() {
        let text = composeText.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty, !manager.state.isAgentTyping else { return }
        manager.dispatch(.sendMessage(content: text))
        composeText = ""
    }
}

// MARK: - Message Bubble

struct MessageBubble: View {
    let message: ChatMessage
    let palette: ChatPalette

    var body: some View {
        HStack {
            if message.isUser { Spacer(minLength: 60) }

            VStack(alignment: message.isUser ? .trailing : .leading, spacing: 4) {
                if message.isUser {
                    Text(message.content)
                        .font(MapleFont.medium(16))
                        .lineSpacing(10)
                        .foregroundStyle(palette.userText)
                        .padding(.horizontal, MapleSpacing.sm)
                        .padding(.vertical, MapleSpacing.xs)
                        .background(palette.userBubbleColor, in: RoundedRectangle(cornerRadius: MapleRadius.lg, style: .continuous))
                } else {
                    Text(message.content)
                        .font(MapleFont.medium(16))
                        .lineSpacing(10)
                        .foregroundStyle(palette.assistantText)
                }

                if message.showTimestamp {
                    Text(message.timestampDisplay)
                        .font(MapleFont.captionSmall)
                        .foregroundStyle(palette.metadataText)
                }
            }

            if !message.isUser { Spacer(minLength: 60) }
        }
    }
}

// MARK: - Settings Sheet

struct SettingsSheet: View {
    @Environment(\.colorScheme) private var colorScheme
    let manager: AppManager

    private var palette: ChatPalette {
        ChatPalette.forColorScheme(colorScheme)
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Button {
                        manager.dispatch(.requestDeleteAgent)
                    } label: {
                        HStack {
                            Image(systemName: "trash")
                                .foregroundStyle(Color.mapleError)
                            Text("Delete Agent")
                                .foregroundStyle(Color.mapleError)
                        }
                    }
                    .disabled(manager.state.isDeletingAgent)
                } footer: {
                    Text("Permanently deletes your agent and conversation history.")
                        .font(MapleFont.captionSmall)
                }

                Section {
                    Button {
                        manager.dispatch(.toggleSettings)
                        manager.dispatch(.logout)
                    } label: {
                        HStack {
                            Image(systemName: "rectangle.portrait.and.arrow.right")
                                .foregroundStyle(palette.settingsSecondaryText)
                            Text("Sign Out")
                                .foregroundStyle(.primary)
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        manager.dispatch(.toggleSettings)
                    }
                    .font(MapleFont.medium(16))
                }
            }
        }
    }
}
