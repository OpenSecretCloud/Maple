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

private let typingIndicatorID = "typing-indicator"

private enum HistoryLoadScrollStrategy: Equatable {
    case preserveBottom
    case preserveAnchor(String)
}

private struct MessageFramePreferenceKey: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]

    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, new in new })
    }
}

private struct ContentHeightPreferenceKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

private func didPrependMessages(oldIDs: [String], newIDs: [String]) -> Bool {
    oldIDs.count > 0 &&
        newIDs.count > oldIDs.count &&
        Array(newIDs.suffix(oldIDs.count)) == oldIDs
}

private func didAppendMessages(oldIDs: [String], newIDs: [String]) -> Bool {
    oldIDs.count > 0 &&
        newIDs.count > oldIDs.count &&
        Array(newIDs.prefix(oldIDs.count)) == oldIDs
}

struct AgentChatView: View {
    @Bindable var manager: AppManager
    @Environment(\.colorScheme) private var colorScheme
    @State private var composeText = ""
    @State private var hasSettledInitialScroll = false
    @State private var messageFrames: [String: CGRect] = [:]
    @State private var scrollViewHeight: CGFloat = 0
    @State private var contentHeight: CGFloat = 0
    @State private var topVisibleMessageID: String?
    @State private var isNearBottom = true
    @State private var pendingHistoryScrollStrategy: HistoryLoadScrollStrategy?
    @State private var lastMessageIDs: [String] = []
    let timestampTimer = Timer.publish(every: 30, on: .main, in: .common).autoconnect()

    private var palette: ChatPalette {
        ChatPalette.forColorScheme(colorScheme)
    }

    private var messageIDs: [String] {
        manager.state.messages.map(\.id)
    }

    private var bottomTargetID: String? {
        manager.state.isAgentTyping ? typingIndicatorID : manager.state.messages.last?.id
    }

    private func scrollToBottom(with proxy: ScrollViewProxy, animated: Bool) {
        guard let bottomTargetID else { return }

        if animated {
            withAnimation {
                proxy.scrollTo(bottomTargetID, anchor: .bottom)
            }
            return
        }

        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            proxy.scrollTo(bottomTargetID, anchor: .bottom)
        }
    }

    private func restoreAfterHistoryLoad(with proxy: ScrollViewProxy) {
        guard let pendingHistoryScrollStrategy else { return }

        switch pendingHistoryScrollStrategy {
        case .preserveBottom:
            scrollToBottom(with: proxy, animated: false)
        case .preserveAnchor(let anchorID):
            var transaction = Transaction()
            transaction.disablesAnimations = true
            withTransaction(transaction) {
                proxy.scrollTo(anchorID, anchor: .top)
            }
        }

        self.pendingHistoryScrollStrategy = nil
    }

    private func requestOlderMessages(triggerMessageID: String? = nil) {
        guard hasSettledInitialScroll,
              manager.state.hasOlderMessages,
              !manager.state.isLoadingHistory,
              pendingHistoryScrollStrategy == nil else { return }

        if contentHeight <= scrollViewHeight + 1, isNearBottom {
            pendingHistoryScrollStrategy = .preserveBottom
        } else if let anchorID = topVisibleMessageID ?? triggerMessageID {
            pendingHistoryScrollStrategy = .preserveAnchor(anchorID)
        } else {
            pendingHistoryScrollStrategy = .preserveBottom
        }

        manager.dispatch(.loadOlderMessages)
    }

    private func maybeRequestOlderMessagesForUnderfilledViewport() {
        guard hasSettledInitialScroll,
              manager.state.hasOlderMessages,
              !manager.state.isLoadingHistory,
              pendingHistoryScrollStrategy == nil,
              scrollViewHeight > 0,
              contentHeight > 0,
              contentHeight <= scrollViewHeight + 1,
              isNearBottom else { return }

        requestOlderMessages()
    }

    private func settleInitialScrollIfNeeded(with proxy: ScrollViewProxy) {
        guard !hasSettledInitialScroll,
              !messageIDs.isEmpty,
              scrollViewHeight > 0,
              let bottomTargetID,
              messageFrames[bottomTargetID] != nil else { return }

        scrollToBottom(with: proxy, animated: false)
        hasSettledInitialScroll = true
    }

    private func updateScrollTracking(with frames: [String: CGRect]) {
        let visibleMessageFrames = frames
            .filter { key, frame in
                key != typingIndicatorID &&
                    frame.maxY > 0 &&
                    frame.minY < scrollViewHeight
            }

        topVisibleMessageID = visibleMessageFrames.min { lhs, rhs in
            lhs.value.minY < rhs.value.minY
        }?.key

        guard let bottomTargetID,
              let bottomFrame = frames[bottomTargetID] else {
            isNearBottom = true
            return
        }

        isNearBottom = bottomFrame.maxY <= scrollViewHeight + 80
    }

    @ViewBuilder
    private func messageRow(index: Int, message: ChatMessage) -> some View {
        MessageBubble(message: message, palette: palette)
            .id(message.id)
            .background {
                GeometryReader { bubbleGeometry in
                    Color.clear.preference(
                        key: MessageFramePreferenceKey.self,
                        value: [message.id: bubbleGeometry.frame(in: .named("chat-scroll"))]
                    )
                }
            }
            .onAppear {
                if index < 3 {
                    requestOlderMessages(triggerMessageID: message.id)
                }
            }
    }

    @ViewBuilder
    private var typingIndicatorView: some View {
        HStack {
            Text("Maple is typing...")
                .font(MapleFont.caption)
                .foregroundStyle(palette.metadataText)
            Spacer()
        }
        .padding(.horizontal)
        .id(typingIndicatorID)
        .background {
            GeometryReader { bubbleGeometry in
                Color.clear.preference(
                    key: MessageFramePreferenceKey.self,
                    value: [typingIndicatorID: bubbleGeometry.frame(in: .named("chat-scroll"))]
                )
            }
        }
    }

    private func makeMessageList(
        proxy: ScrollViewProxy,
        geometry: GeometryProxy
    ) -> some View {
        let showHistorySpinner = manager.state.isLoadingHistory && !manager.state.messages.isEmpty
        let showInitialSpinner =
            (manager.state.isLoadingHistory && manager.state.messages.isEmpty) ||
            (!hasSettledInitialScroll && !manager.state.messages.isEmpty)

        return ScrollView {
            LazyVStack(spacing: MapleSpacing.md) {
                ForEach(Array(manager.state.messages.enumerated()), id: \.element.id) { index, message in
                    messageRow(index: index, message: message)
                }

                if manager.state.isAgentTyping {
                    typingIndicatorView
                }
            }
            .padding()
            .background {
                GeometryReader { contentGeometry in
                    Color.clear.preference(
                        key: ContentHeightPreferenceKey.self,
                        value: contentGeometry.size.height
                    )
                }
            }
        }
        .defaultScrollAnchor(.bottom)
        .coordinateSpace(name: "chat-scroll")
        .opacity(hasSettledInitialScroll || manager.state.messages.isEmpty ? 1 : 0)
        .overlay(alignment: .top) {
            if showHistorySpinner {
                ProgressView()
                    .padding(.top, MapleSpacing.xs)
            }
        }
        .overlay {
            if showInitialSpinner {
                ProgressView()
            }
        }
        .onAppear {
            scrollViewHeight = geometry.size.height
            updateScrollTracking(with: messageFrames)
        }
        .onChange(of: geometry.size.height) { _, newHeight in
            scrollViewHeight = newHeight
            updateScrollTracking(with: messageFrames)
            settleInitialScrollIfNeeded(with: proxy)
        }
        .onPreferenceChange(MessageFramePreferenceKey.self) { frames in
            messageFrames = frames
            updateScrollTracking(with: frames)
            settleInitialScrollIfNeeded(with: proxy)
        }
        .onPreferenceChange(ContentHeightPreferenceKey.self) { newHeight in
            contentHeight = newHeight
            settleInitialScrollIfNeeded(with: proxy)
            maybeRequestOlderMessagesForUnderfilledViewport()
        }
        .onChange(of: hasSettledInitialScroll) { _, _ in
            maybeRequestOlderMessagesForUnderfilledViewport()
        }
        .onChange(of: messageIDs) { oldIDs, newIDs in
            if newIDs.isEmpty {
                hasSettledInitialScroll = false
                pendingHistoryScrollStrategy = nil
                messageFrames = [:]
                topVisibleMessageID = nil
                lastMessageIDs = []
            } else if !hasSettledInitialScroll {
                settleInitialScrollIfNeeded(with: proxy)
            } else if didPrependMessages(oldIDs: oldIDs, newIDs: newIDs) {
                restoreAfterHistoryLoad(with: proxy)
            } else if didAppendMessages(oldIDs: oldIDs, newIDs: newIDs) && isNearBottom {
                scrollToBottom(with: proxy, animated: false)
            } else {
                pendingHistoryScrollStrategy = nil
            }

            lastMessageIDs = newIDs
        }
        .onChange(of: manager.state.messages.last?.content) { _, _ in
            guard hasSettledInitialScroll,
                  pendingHistoryScrollStrategy == nil,
                  isNearBottom else { return }

            scrollToBottom(with: proxy, animated: false)
        }
        .onChange(of: manager.state.isAgentTyping) { _, _ in
            guard hasSettledInitialScroll,
                  pendingHistoryScrollStrategy == nil,
                  isNearBottom else { return }

            scrollToBottom(with: proxy, animated: false)
        }
        .onChange(of: manager.state.isLoadingHistory) { _, isLoading in
            if !isLoading,
               pendingHistoryScrollStrategy != nil,
               messageIDs == lastMessageIDs {
                pendingHistoryScrollStrategy = nil
            }
        }
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
                                    .font(.system(size: 13, weight: .bold))
                                    .foregroundStyle(palette.secondaryIcon)
                                    .frame(width: 32, height: 32)
                            }
                            .controlSize(.small)
                            .buttonStyle(.glass)
                        }
                        Spacer()
                        GlassEffectContainer {
                            Button {
                            } label: {
                                Image(systemName: "magnifyingglass")
                                    .font(.system(size: 13, weight: .bold))
                                    .foregroundStyle(palette.secondaryIcon)
                                    .frame(width: 32, height: 32)
                            }
                            .controlSize(.small)
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
        GeometryReader { geometry in
            ScrollViewReader { proxy in
                makeMessageList(proxy: proxy, geometry: geometry)
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
