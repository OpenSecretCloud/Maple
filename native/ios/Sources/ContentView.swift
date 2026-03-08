import SwiftUI

struct ContentView: View {
    @Bindable var manager: AppManager
    @State private var showSplash = true
    @State private var splashMinTimePassed = false

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
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                splashMinTimePassed = true
                dismissSplashIfReady()
            }
        }
        .onChange(of: manager.state.router.defaultScreen) { _, _ in
            dismissSplashIfReady()
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
    @State private var email = ""
    @State private var password = ""
    @State private var name = ""
    @State private var isSignUp = false
    @FocusState private var focusedField: LoginField?

    private enum LoginField {
        case name, email, password
    }

    private var isLoading: Bool {
        switch manager.state.auth {
        case .loggingIn, .signingUp:
            return true
        default:
            return false
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(spacing: 24) {
                MapleWordmark(color: .pebble800, height: 28)

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

                    SecureField("Password", text: $password)
                        .textFieldStyle(.roundedBorder)
                        .textContentType(isSignUp ? .newPassword : .password)
                        .focused($focusedField, equals: .password)
                        .submitLabel(.go)
                        .onSubmit(submit)
                        .overlay(
                            RoundedRectangle(cornerRadius: MapleRadius.md)
                                .stroke(Color.neutral200, lineWidth: 1)
                        )
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

                Button(isSignUp ? "Already have an account? Sign In" : "Don't have an account? Sign Up") {
                    isSignUp.toggle()
                }
                .font(MapleFont.body)
                .foregroundStyle(Color.pebble500)
            }
            .padding(24)
            .glassEffect(.regular, in: .rect(cornerRadius: MapleRadius.xl))
            .padding(.horizontal, MapleSpacing.md)

            Spacer()

            if let toast = manager.state.toast {
                Text(toast)
                    .font(MapleFont.caption)
                    .foregroundStyle(Color.mapleError)
                    .padding()
                    .onTapGesture {
                        manager.dispatch(.clearToast)
                    }
            }
        }
        .background(
            RadialGradient(
                colors: [
                    Color.maple500.opacity(0.15),
                    Color.bark300.opacity(0.1),
                    Color.pebble400.opacity(0.08),
                    Color.neutral50,
                ],
                center: .bottom,
                startRadius: 0,
                endRadius: 500
            )
            .ignoresSafeArea()
        )
    }

    private func submit() {
        if isSignUp {
            manager.dispatch(.signUpWithEmail(email: email, password: password, name: name))
        } else {
            manager.dispatch(.loginWithEmail(email: email, password: password))
        }
    }

    private func mapleTextField(_ placeholder: String, text: Binding<String>) -> some View {
        TextField(placeholder, text: text)
            .textFieldStyle(.roundedBorder)
            .overlay(
                RoundedRectangle(cornerRadius: MapleRadius.md)
                    .stroke(Color.neutral200, lineWidth: 1)
            )
    }

    private func dividerWithText(_ text: String) -> some View {
        HStack {
            Rectangle().frame(height: 1).foregroundStyle(Color.neutral200)
            Text(text).font(MapleFont.caption).foregroundStyle(Color.pebble400)
            Rectangle().frame(height: 1).foregroundStyle(Color.neutral200)
        }
    }

    private func oauthButton(label: String, icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                Image(systemName: icon)
                Text(label)
            }
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(MapleSecondaryButtonStyle())
    }
}

// MARK: - Agent Chat

struct AgentChatView: View {
    @Bindable var manager: AppManager
    @State private var composeText = ""
    let timestampTimer = Timer.publish(every: 30, on: .main, in: .common).autoconnect()

    var body: some View {
        messageList
            .safeAreaInset(edge: .top) {
                ZStack {
                    GlassEffectContainer {
                        MapleWordmark(color: .primary, height: 20)
                            .padding(.horizontal, MapleSpacing.md)
                            .padding(.vertical, MapleSpacing.sm)
                            .glassEffect(in: .capsule)
                    }

                    HStack {
                        Spacer()
                        GlassEffectContainer {
                            Button {
                                manager.dispatch(.toggleSettings)
                            } label: {
                                Image(systemName: "gearshape")
                                    .font(.system(size: 16, weight: .medium))
                                    .frame(width: 36, height: 36)
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
                MeshGradient(width: 3, height: 3, points: [
                    [0.0, 0.0], [0.5, 0.0], [1.0, 0.0],
                    [0.0, 0.5], [0.5, 0.5], [1.0, 0.5],
                    [0.0, 1.0], [0.5, 1.0], [1.0, 1.0],
                ], colors: [
                    .pebble100, .maple50,   .bark50,
                    .maple50,   .neutral0,  .pebble50,
                    .bark50,    .maple50,   .pebble100,
                ])
                .ignoresSafeArea()
            )
        .overlay(alignment: .bottom) {
            if let toast = manager.state.toast {
                Text(toast)
                    .font(MapleFont.caption)
                    .foregroundStyle(Color.mapleError)
                    .padding(MapleSpacing.xs)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: MapleRadius.sm))
                    .padding(.bottom, 80)
                    .onTapGesture { manager.dispatch(.clearToast) }
            }
        }
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
                LazyVStack(spacing: MapleSpacing.xs) {
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
                        MessageBubble(message: message)
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
                                .foregroundStyle(Color.pebble400)
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

    private var composeBar: some View {
        GlassEffectContainer {
            HStack(spacing: MapleSpacing.xs) {
                TextField("Message Maple...", text: $composeText)
                    .font(MapleFont.body)
                    .padding(.horizontal, MapleSpacing.sm)
                    .padding(.vertical, MapleSpacing.xs)
                    .onSubmit(sendMessage)

                Button(action: sendMessage) {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                        .foregroundStyle(
                            composeText.trimmingCharacters(in: .whitespaces).isEmpty || manager.state.isAgentTyping
                                ? Color.neutral300
                                : Color.maple500
                        )
                }
                .disabled(composeText.trimmingCharacters(in: .whitespaces).isEmpty || manager.state.isAgentTyping)
            }
            .padding(.horizontal, MapleSpacing.sm)
            .padding(.vertical, MapleSpacing.xs)
            .glassEffect(in: .capsule)
        }
        .padding(.horizontal, MapleSpacing.sm)
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

    var body: some View {
        HStack {
            if message.isUser { Spacer(minLength: 60) }

            VStack(alignment: message.isUser ? .trailing : .leading, spacing: 4) {
                if !message.isUser && message.showSender {
                    Text("Maple")
                        .font(MapleFont.caption)
                        .foregroundStyle(Color.pebble400)
                }
                Text(message.content)
                .font(MapleFont.body)
                .padding(.horizontal, MapleSpacing.sm)
                .padding(.vertical, MapleSpacing.xs)
                .background(
                    Group {
                        if message.isUser {
                            LinearGradient(
                                colors: [.maple400, .maple600],
                                startPoint: .top,
                                endPoint: .bottom
                            )
                        } else {
                            LinearGradient(
                                colors: [.pebble50, .pebble50],
                                startPoint: .top,
                                endPoint: .bottom
                            )
                        }
                    }
                )
                .clipShape(
                    UnevenRoundedRectangle(
                        topLeadingRadius: MapleRadius.lg,
                        bottomLeadingRadius: message.isUser ? MapleRadius.lg : 4,
                        bottomTrailingRadius: message.isUser ? 4 : MapleRadius.lg,
                        topTrailingRadius: MapleRadius.lg
                    )
                )
                .foregroundStyle(message.isUser ? Color.white : Color.neutral800)

                if message.showTimestamp {
                    Text(message.timestampDisplay)
                        .font(MapleFont.captionSmall)
                        .foregroundStyle(Color.pebble400)
                }
            }

            if !message.isUser { Spacer(minLength: 60) }
        }
    }
}

// MARK: - Settings Sheet

struct SettingsSheet: View {
    let manager: AppManager

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
                                .foregroundStyle(Color.pebble500)
                            Text("Sign Out")
                                .foregroundStyle(Color.neutral800)
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
