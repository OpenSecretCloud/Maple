#if os(iOS) && DEBUG
    import Foundation
    import MapleAppCore
    import MapleSDK
    import Observation
    import SwiftUI
    import UIKit

    /// Exercises ChatDetail itself. Only service, persistence and account inputs
    /// are synthetic; pagination, turn grouping, rendering, actions, progress,
    /// accepted-turn anchoring and the composer inset are shipping implementations.
    struct ShippingViewportFixtureView: View {
        @State private var model: ShippingViewportFixtureModel?
        @State private var failed = false
        @State private var showingChat = true
        @State private var compactLease = CompactMenuLease()
        @AccessibilityFocusState private var paginationMenuFocused: Bool
        @AccessibilityFocusState private var paginationCloseFocused: Bool
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @Environment(\.layoutDirection) private var layoutDirection

        private var resizeProof: Bool { ProcessInfo.processInfo.arguments.contains("--maple-shipping-window-proof") }

        var body: some View {
            Group {
                if let model,
                    ProcessInfo.processInfo.arguments.contains("--maple-shipping-pagination-menu-proof")
                {
                    paginationMenuViewport(model)
                } else if let model, model.compactProjectionProof {
                    compactViewport(model)
                } else {
                    standardViewport
                }
            }
            .font(.custom("Manrope-Regular", size: 17, relativeTo: .body))
            .tint(MapleAccent.foreground)
            .task {
                guard model == nil, !failed else { return }
                do {
                    let created = try ShippingViewportFixtureModel()
                    model = created
                    await created.prepare()
                    if resizeProof { created.prepareWindowOwnership() }
                } catch { failed = true }
            }
            .onDisappear {
                if let model { compactLease.retireAccount(scope: model.account.scope); model.retire() }
            }
        }

        private var standardViewport: some View {
            NavigationStack {
                Group {
                    if let model {
                        Group {
                            if model.draftLoadProof && !showingChat {
                                Text("Retained draft remains in its runtime")
                                    .accessibilityIdentifier("shipping-draft.away")
                            } else {
                                ChatDetail(
                                    environment: model.environment, runtime: model.runtime,
                                    account: model.account, preferences: model.preferences,
                                    fixtureProjectionIsCurrent: { [weak model] scope, owner in
                                        model?.projectionIsCurrent(scope: scope, storageOwner: owner) ?? false
                                    }, fixtureOptions: model.optionsContext,
                                    fixtureScrollMetricsChanged: { [weak model] metrics in
                                        guard
                                            ProcessInfo.processInfo.arguments.contains(
                                                "--maple-shipping-follow-scroll-proof")
                                        else { return }
                                        model?.scrollMetrics = metrics
                                    }
                                )
                                .environment(\.messageRenderDisappearanceDiagnostic) {
                                    [weak model] prepared, retained in
                                    model?.recordRenderingDisappearance(prepared: prepared, retained: retained)
                                }
                            }
                        }
                        .safeAreaInset(edge: .top, spacing: 0) {
                            controls(model).padding(8).background(.bar)
                        }
                    } else {
                        Text(failed ? "Viewport fixture failed" : "Opening viewport fixture")
                    }
                }
                .navigationTitle("Shipping transcript fixture")
                .navigationBarTitleDisplayMode(.inline)
            }
        }

        /// The same history runtime stays behind the actual retained navigation
        /// while its existing synthetic pagination service is released. Match
        /// ChatScreen's native navigation shells without an outer fixture inset.
        private func paginationMenuViewport(_ model: ShippingViewportFixtureModel) -> some View {
            let scope = model.runtime.scope, owner = model.runtime.snapshot.storageOwner
            return CompactMenuNavigation(
                scope: scope, storageOwner: owner,
                requestedMenu: model.projectionMenuPresented, reduceMotion: reduceMotion, lease: compactLease,
                library: NavigationStack {
                    List {
                        Text("Synthetic retained Library")
                        Button("Release older") { Task { await model.releaseOlder() } }
                            .accessibilityIdentifier("shipping-pagination.release-older")
                            .disabled(!model.runtime.isLoadingOlder || !compactLease.menuVisible)
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                    .background(MapleSurface.sidebar)
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .principal) { paginationNavigationDiagnostics(model) }
                        ToolbarItem(placement: .topBarTrailing) {
                            Button {
                                guard model.projectionIsCurrent(scope: scope, storageOwner: owner) else { return }
                                model.projectionMenuPresented = false
                            } label: {
                                Label("Close menu", systemImage: "xmark")
                                    .labelStyle(.iconOnly)
                                    .frame(minWidth: 44, minHeight: 44)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("shipping-pagination.close")
                            .accessibilityFocused($paginationCloseFocused)
                        }
                    }
                }
                .environment(\.layoutDirection, layoutDirection)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(MapleSurface.sidebar)
                .background(ShippingPaginationViewportProbe())
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("shipping-pagination.library"),
                chat: NavigationStack {
                    ChatDetail(
                        environment: model.environment, runtime: model.runtime,
                        account: model.account, preferences: model.preferences,
                        fixtureProjectionIsCurrent: { [weak model] scope, owner in
                            model?.projectionIsCurrent(scope: scope, storageOwner: owner) ?? false
                        }, fixtureOptions: model.optionsContext
                    )
                    .id(owner)
                    .background(MapleSurface.canvas)
                    .navigationTitle("Synthetic retained chat")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar(removing: .sidebarToggle)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            Button {
                                guard model.projectionIsCurrent(scope: scope, storageOwner: owner) else { return }
                                model.projectionMenuPresented = true
                            } label: {
                                Label("Menu", systemImage: "line.3.horizontal")
                                    .labelStyle(.iconOnly)
                                    .frame(minWidth: 44, minHeight: 44)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("shipping-pagination.open")
                            .accessibilityFocused($paginationMenuFocused)
                            .disabled(!model.ready || model.projectionMenuPresented)
                        }
                        ToolbarItem(placement: .principal) { paginationNavigationDiagnostics(model) }
                    }
                }
                .environment(\.layoutDirection, layoutDirection)
                .environment(\.compactMenuChatState) { [weak compactLease] scope, owner in
                    compactLease?.state(scope: scope, storageOwner: owner) ?? CompactMenuChatState()
                }
                .background(MapleSurface.canvas)
                .modifier(
                    CompactMenuChatAccessibility(
                        lease: compactLease, scope: scope, storageOwner: owner,
                        isCurrent: { [weak model] in
                            model?.projectionIsCurrent(scope: scope, storageOwner: owner) ?? false
                        })),
                // Admit same-account updates while initial loading replaces the
                // owner; the lease and child still fence the exact projection.
                isCurrent: { [weak model] in
                    model?.projectionIsCurrent(scope: scope) ?? false
                },
                presentationBlocked: { false },
                requestMenu: { [weak model] in model?.projectionMenuPresented = $0 },
                blockPresentation: { _, _ in },
                chatShown: { reason in
                    guard model.projectionIsCurrent(scope: scope, storageOwner: owner),
                        !model.projectionMenuPresented
                    else { return }
                    if reason == .navigation { paginationMenuFocused = true }
                }
            )
            .id(scope)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .ignoresSafeArea(.container)
            .onChange(of: model.projectionMenuPresented) { _, presented in
                if presented { paginationMenuFocused = false }
            }
            .onChange(of: compactLease.menuVisible) { _, available in
                paginationCloseFocused = available
            }
        }

        private func paginationNavigationDiagnostics(_ model: ShippingViewportFixtureModel) -> some View {
            VStack(spacing: 0) {
                Text(model.summary).font(.caption2).lineLimit(1).minimumScaleFactor(0.6)
                    .accessibilityIdentifier("shipping-viewport.summary")
                Text("Synthetic retained Chat owners").font(.caption2)
                    .accessibilityIdentifier("shipping-window.ownership")
                    .accessibilityValue(model.windowOwnership)
            }
            .accessibilityElement(children: .contain)
        }

        private func compactViewport(_ model: ShippingViewportFixtureModel) -> some View {
            let selected = model.selectedProjection
            let scope = model.account.scope
            return CompactMenuNavigation(
                scope: scope, storageOwner: selected.snapshot.storageOwner,
                requestedMenu: model.projectionMenuPresented, reduceMotion: reduceMotion, lease: compactLease,
                library: Text("Synthetic retained Library").frame(maxWidth: .infinity, maxHeight: .infinity),
                chat: NavigationStack {
                    ChatDetail(
                        environment: model.environment, runtime: selected,
                        account: model.account, preferences: model.preferences,
                        fixtureProjectionIsCurrent: { [weak model] scope, owner in
                            model?.projectionIsCurrent(scope: scope, storageOwner: owner) ?? false
                        }, fixtureOptions: model.optionsContext
                    )
                    .id(selected.snapshot.storageOwner)
                }
                .environment(\.compactMenuChatState) { [weak compactLease] scope, owner in
                    compactLease?.state(scope: scope, storageOwner: owner) ?? CompactMenuChatState()
                }
                .modifier(
                    CompactMenuChatAccessibility(
                        lease: compactLease, scope: scope, storageOwner: selected.snapshot.storageOwner,
                        isCurrent: { [weak model] in model?.projectionIsCurrent(scope: scope) ?? false })),
                isCurrent: { [weak model] in model?.projectionIsCurrent(scope: scope) ?? false },
                presentationBlocked: { false },
                requestMenu: { [weak model] in model?.projectionMenuPresented = $0 },
                blockPresentation: { _, _ in }, chatShown: { _ in }
            )
            .id(scope)
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(spacing: 4) {
                    HStack {
                        Button("Select cached A") { model.selectProjection(newChat: true) }
                            .accessibilityIdentifier("shipping-projection.select-a")
                            .disabled(!model.ready || model.projectionIsNewChat)
                        Button("Select cached B") { model.selectProjection(newChat: false) }
                            .accessibilityIdentifier("shipping-projection.select-b")
                            .disabled(!model.ready || !model.projectionIsNewChat)
                    }.font(.caption)
                    Text(model.summary).font(.caption2)
                        .accessibilityIdentifier("shipping-viewport.summary")
                    Text("Synthetic cached owners").font(.caption2)
                        .accessibilityIdentifier("shipping-projection.ownership")
                        .accessibilityValue(model.projectionOwnership)
                }
                .frame(maxWidth: .infinity).frame(height: 80).background(.bar)
            }
        }

        @ViewBuilder private func controls(_ model: ShippingViewportFixtureModel) -> some View {
            if model.historyRenderingProof {
                VStack(spacing: 4) {
                    Text(model.summary).font(.caption2)
                        .accessibilityIdentifier("shipping-viewport.summary")
                    Text("Synthetic completed render lifecycles").font(.caption2)
                        .accessibilityIdentifier("shipping-history.render-lifecycles")
                        .accessibilityValue(model.renderLifecyclesJSON)
                }
                .frame(maxWidth: .infinity).frame(height: 60)
            } else if model.draftLoadProof {
                VStack(spacing: 4) {
                    HStack {
                        Button("Release stored draft") { Task { await model.releaseDraftLoad() } }
                            .accessibilityIdentifier("shipping-draft.release")
                            .disabled(!model.draftLoadHeld)
                        Button(showingChat ? "Visit another screen" : "Return to chat") { showingChat.toggle() }
                            .accessibilityIdentifier("shipping-draft.navigate")
                            .disabled(!model.ready)
                    }.font(.caption)
                    Text(model.draftLoadHeld ? "Load held" : "Load resumed")
                        .accessibilityIdentifier("shipping-draft.state")
                    Text(model.summary).font(.caption2)
                        .accessibilityIdentifier("shipping-viewport.summary")
                    Text(model.runtime.draftText).font(.caption2).lineLimit(1)
                        .accessibilityIdentifier("shipping-draft.model")
                }
                .frame(maxWidth: .infinity).frame(height: 96)
            } else {
                regularControls(model)
            }
        }

        private func regularControls(_ model: ShippingViewportFixtureModel) -> some View {
            VStack(spacing: 4) {
                HStack {
                    Button("Release older") { Task { await model.releaseOlder() } }
                        .accessibilityIdentifier("shipping-viewport.release-older")
                        .disabled(!model.runtime.isLoadingOlder)
                    Button("Start run") { model.startRun() }
                        .accessibilityIdentifier("shipping-viewport.start")
                        .disabled(!model.ready || model.started)
                    Button("Emit chunks") { model.emitChunks() }
                        .accessibilityIdentifier("shipping-viewport.emit")
                        .disabled(model.runtime.phase != .streaming || model.emitting || model.chunks > 0)
                    Button("Finish") { Task { await model.finish() } }
                        .accessibilityIdentifier("shipping-viewport.finish")
                        .disabled(model.chunks != 40 || model.emitting || !model.runtime.hasUnresolvedRun)
                    if ProcessInfo.processInfo.arguments.contains("--maple-shipping-follow-scroll-proof") {
                        Button("Append item") { Task { await model.appendReplyItem() } }
                            .accessibilityIdentifier("shipping-viewport.append-item")
                            .disabled(model.chunks != 40 || model.emitting || !model.runtime.hasUnresolvedRun)
                    }
                }.font(.caption)
                Text(model.summary).font(.caption2).lineLimit(1).minimumScaleFactor(0.6)
                    .accessibilityIdentifier("shipping-viewport.summary")
                if ProcessInfo.processInfo.arguments.contains("--maple-shipping-follow-scroll-proof") {
                    Text("Synthetic native viewport geometry").font(.caption2)
                        .accessibilityIdentifier("shipping-viewport.scroll-metrics")
                        .accessibilityValue(model.scrollMetricsJSON)
                }
                if resizeProof {
                    Text("Window ownership").font(.caption2)
                        .accessibilityIdentifier("shipping-window.ownership")
                        .accessibilityValue(model.windowOwnership)
                }
            }
            .frame(
                height: resizeProof
                    || ProcessInfo.processInfo.arguments.contains("--maple-shipping-follow-scroll-proof") ? 80 : 60)
        }
    }

    /// Exposes UIKit's actual laid-out bounds without assigning an accessibility frame.
    private struct ShippingPaginationViewportProbe: UIViewRepresentable {
        func makeUIView(context: Context) -> UIView {
            let view = UIView()
            view.backgroundColor = .clear
            view.isUserInteractionEnabled = false
            view.isAccessibilityElement = true
            view.accessibilityLabel = "Synthetic retained Library viewport"
            view.accessibilityIdentifier = "shipping-pagination.viewport"
            return view
        }

        func updateUIView(_ uiView: UIView, context: Context) {}
    }

    @MainActor @Observable private final class ShippingViewportFixtureModel {
        let environment: AppEnvironment
        let runtime: ChatRuntime
        let account: AccountEntitlements
        let preferences: PreferencesController
        let optionsLibrary: LibraryModel
        private let optionsService: LibraryControlsFixtureService
        private var optionsEditors: [InstructionEditor] = []
        private var selectedOptionsProjects: [UUID: String] = [:]
        private let service = ShippingViewportService(
            complexMarkup: ProcessInfo.processInfo.arguments.contains("--maple-shipping-complex-markdown-proof"),
            transcriptRows: ProcessInfo.processInfo.arguments.contains("--maple-shipping-transcript-rows-proof"),
            historyRendering: ProcessInfo.processInfo.arguments.contains("--maple-shipping-history-rendering-proof"))
        private let store: ShippingViewportStore
        let draftLoadProof: Bool
        let compactProjectionProof: Bool
        let historyRenderingProof = ProcessInfo.processInfo.arguments.contains(
            "--maple-shipping-history-rendering-proof")
        private let cachedProjection: ChatRuntime?
        private let cachedProjectionInstance = UUID()
        private(set) var projectionIsNewChat = true
        var projectionMenuPresented = false
        private let instanceID = UUID()
        private(set) var draftLoadHeld = false
        @ObservationIgnored private var draftLoadTask: Task<Void, Never>?
        @ObservationIgnored private var retired = false
        var ready = false
        var started = false
        var emitting = false
        var chunks = 0
        var appendedItems = 0
        var scrollMetrics: TranscriptViewportMetrics?
        private var preparedDisappeared = 0
        private var preparedRetained = 0
        private var preparedCleared = 0
        var failed = false
        @ObservationIgnored private var streamTask: Task<Void, Never>?

        init() throws {
            let draftLoadProof =
                ProcessInfo.processInfo.arguments.contains("--maple-shipping-viewport-fixture")
                && ProcessInfo.processInfo.arguments.contains("--maple-shipping-draft-load-proof")
            self.draftLoadProof = draftLoadProof
            let compactProjectionProof =
                ProcessInfo.processInfo.arguments.contains("--maple-shipping-viewport-fixture")
                && ProcessInfo.processInfo.arguments.contains("--maple-shipping-compact-projection-proof")
            self.compactProjectionProof = compactProjectionProof
            let store = ShippingViewportStore(holdInitialLoad: draftLoadProof)
            self.store = store
            // This environment is never restored or activated. Inert loopback
            // origins isolate its SDK namespace from the local signed-in app.
            let configuration = try AppConfiguration(
                profile: .local, apiURL: "http://127.0.0.1:9", billingURL: "http://127.0.0.1:9",
                flagsURL: "http://127.0.0.1:9", authURL: "http://127.0.0.1:9",
                clientID: "00000000-0000-4000-8000-000000000009", callbackScheme: "cloud.opensecret.maple.native.dev")
            environment = try AppEnvironment(configuration: configuration)
            let scope = AccountScope(apiOrigin: configuration.apiURL.absoluteString, principalID: "synthetic-viewport")
            runtime = ChatRuntime(
                scope: scope, service: service, persistence: store,
                conversationID: ShippingViewportService.chatID)
            cachedProjection =
                compactProjectionProof
                ? ChatRuntime(scope: scope, service: service, persistence: store) : nil
            let entitlements = ShippingViewportEntitlements()
            account = AccountEntitlements(scope: scope, isGuest: false, billing: entitlements, catalog: entitlements)
            preferences = PreferencesController(scope: scope, store: ShippingViewportPreferences())
            let optionsService = LibraryControlsFixtureService(
                failPage: false, failDelete: false, dragProof: false, optionsProof: true)
            self.optionsService = optionsService
            optionsLibrary = LibraryModel(
                scope: scope, service: optionsService,
                deletions: ChatDeletionCoordinator(
                    scope: scope, retained: [], service: optionsService, persistence: store))
            runtime.isVisible = true
        }

        var optionsContext: ChatOptionsFixtureContext {
            let scope = account.scope, service = optionsService, owner = selectedProjection.snapshot.storageOwner
            return ChatOptionsFixtureContext(
                library: optionsLibrary,
                projectID: { [weak self] in self?.selectedOptionsProjects[owner] },
                makeInstructionEditor: { [weak self] destination in
                    let editor = InstructionEditor(scope: scope, destination: destination, service: service)
                    guard let self, projectionIsCurrent(scope: scope, storageOwner: owner) else {
                        editor.invalidate(); return editor
                    }
                    optionsEditors.append(editor); return editor
                },
                selectProject: { [weak self] id in
                    guard let self, projectionIsCurrent(scope: scope, storageOwner: owner) else { return false }
                    selectedOptionsProjects[owner] = id; return true
                })
        }

        var summary: String {
            "Ready \(ready ? 1 : 0); older \(runtime.isLoadingOlder ? 1 : 0); messages \(runtime.messages.count); chunks \(chunks); running \(runtime.hasUnresolvedRun ? 1 : 0); failed \(failed ? 1 : 0)"
        }
        var scrollMetricsJSON: String {
            guard let scrollMetrics, let data = try? JSONEncoder().encode(scrollMetrics) else { return "{}" }
            return String(data: data, encoding: .utf8) ?? "{}"
        }
        var renderLifecyclesJSON: String {
            let values = [
                "preparedDisappeared": preparedDisappeared,
                "preparedRetained": preparedRetained,
                "preparedCleared": preparedCleared,
            ]
            guard let data = try? JSONEncoder().encode(values) else { return "{}" }
            return String(data: data, encoding: .utf8) ?? "{}"
        }
        func recordRenderingDisappearance(prepared: Bool, retained: Bool) {
            guard historyRenderingProof, !retired, prepared else { return }
            preparedDisappeared += 1
            if retained { preparedRetained += 1 } else { preparedCleared += 1 }
        }
        func prepare() async {
            guard !retired else { return }
            await preferences.load()
            guard !retired, !Task.isCancelled else { return }
            await account.refresh()
            await optionsLibrary.loadProjects()
            guard !retired, !Task.isCancelled else { return }
            if draftLoadProof {
                await store.seed(
                    ChatSnapshot(
                        storageKey: ShippingViewportService.chatID, conversationID: ShippingViewportService.chatID,
                        draft: ChatDraft(text: "Stored synthetic draft arrives before typing")))
                guard !retired, !Task.isCancelled else { return }
                let runtime = runtime
                let loading = Task { await runtime.load(chatID: ShippingViewportService.chatID) }
                draftLoadTask = loading
                await store.waitForInitialLoad()
                guard !retired, !Task.isCancelled else {
                    loading.cancel(); await store.cancelInitialLoad(); return
                }
                draftLoadHeld = true
                await loading.value
                draftLoadTask = nil; draftLoadHeld = false
            } else {
                await runtime.load(chatID: ShippingViewportService.chatID)
            }
            guard !retired, !Task.isCancelled else { return }
            if compactProjectionProof, let cachedProjection {
                await cachedProjection.load(chatID: nil)
                guard !retired, !Task.isCancelled else { return }
                runtime.draftText = "Cached B draft stays separate"
            }
            ready =
                preferences.state == .ready && account.chatAccess == .allowed
                && runtime.messages.count == (historyRenderingProof ? 12 : 8)
            failed = !ready
        }
        var selectedProjection: ChatRuntime {
            projectionIsNewChat ? cachedProjection ?? runtime : runtime
        }
        func projectionIsCurrent(scope: AccountScope) -> Bool {
            !retired && account.scope == scope && selectedProjection.scope == scope
        }
        func projectionIsCurrent(scope: AccountScope, storageOwner: UUID) -> Bool {
            projectionIsCurrent(scope: scope) && selectedProjection.snapshot.storageOwner == storageOwner
        }
        func selectProjection(newChat: Bool) {
            guard compactProjectionProof, ready, !retired, cachedProjection != nil else { return }
            // Select an already-loaded runtime only. No load, draft mutation or
            // deferred model publication may repair the returned projection.
            projectionIsNewChat = newChat
        }
        var projectionOwnership: String {
            guard let cachedProjection else { return "{}" }
            let value: [String: String] = [
                "selected": projectionIsNewChat ? "A" : "B",
                "aInstance": cachedProjectionInstance.uuidString, "bInstance": instanceID.uuidString,
                "aStorageOwner": cachedProjection.snapshot.storageOwner.uuidString,
                "aDraftOwner": cachedProjection.snapshot.draft.id.uuidString,
                "bStorageOwner": runtime.snapshot.storageOwner.uuidString,
                "bDraftOwner": runtime.snapshot.draft.id.uuidString,
                "aText": cachedProjection.draftText, "bText": runtime.draftText,
                "aLoading": cachedProjection.isLoading ? "true" : "false",
                "bLoading": runtime.isLoading ? "true" : "false",
                "running": cachedProjection.hasUnresolvedRun || runtime.hasUnresolvedRun ? "true" : "false",
            ]
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            return (try? encoder.encode(value)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        }
        func releaseDraftLoad() async {
            guard draftLoadProof, draftLoadHeld, !retired else { return }
            await store.releaseInitialLoad()
        }
        /// Resize tests retain actual runtime draft/queue owners. The initial
        /// synthetic data is admitted by the runtime; no Responses call is made.
        func prepareWindowOwnership() {
            guard ready, !runtime.hasUnresolvedRun, runtime.queuedTurns.isEmpty else { failed = true; return }
            runtime.draftText = "Window queued A remains paused"
            guard runtime.enqueueDraft() else { failed = true; return }
            runtime.draftText = "Window held draft B"
        }
        var windowOwnership: String {
            let value: [String: String] = [
                "instance": instanceID.uuidString,
                "storageOwner": runtime.snapshot.storageOwner.uuidString,
                "draftOwner": runtime.snapshot.draft.id.uuidString,
                "queueOwner": runtime.queuedTurns.first?.id.uuidString ?? "none",
                "queueDraftOwner": runtime.queuedTurns.first?.draft.id.uuidString ?? "none",
                "queueMessageID": runtime.queuedTurns.first?.messageID ?? "none",
                "queueCount": String(runtime.queuedTurns.count),
                "queuePaused": runtime.isQueuePaused ? "true" : "false",
                "queuedText": runtime.queuedTurns.first?.draft.text ?? "",
                "running": runtime.hasUnresolvedRun ? "true" : "false",
            ]
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            return (try? encoder.encode(value)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        }
        func releaseOlder() async { await service.releaseOlder() }
        func startRun() {
            guard ready, !started else { return }
            runtime.draftText = "Synthetic live viewport turn"
            started = runtime.send()
            failed = !started
        }
        func emitChunks() {
            guard runtime.phase == .streaming, !emitting, chunks == 0 else { return }
            emitting = true
            streamTask = Task { [weak self] in
                guard let self else { return }
                defer { emitting = false }
                do {
                    for index in 0..<40 {
                        try Task.checkCancellation()
                        try await service.append(index: index)
                        chunks = index + 1
                        try await Task.sleep(for: .milliseconds(40))
                    }
                } catch { if !Task.isCancelled { failed = true } }
            }
        }
        func finish() async {
            do { try await service.finish() } catch { failed = true }
        }
        func appendReplyItem() async {
            guard ProcessInfo.processInfo.arguments.contains("--maple-shipping-follow-scroll-proof"),
                !retired, !emitting, chunks == 40, runtime.phase == .streaming
            else { return }
            do {
                try await service.appendReplyItem(index: appendedItems)
                appendedItems += 1
            } catch { failed = true }
        }
        func retire() {
            guard !retired else { return }
            retired = true; draftLoadTask?.cancel(); streamTask?.cancel(); runtime.invalidate()
            cachedProjection?.invalidate()
            optionsLibrary.invalidate()
            for editor in optionsEditors { editor.invalidate() }
            optionsEditors.removeAll()
            Task {
                await store.cancelInitialLoad(); await service.abort()
            }
        }
    }

    private actor ShippingViewportService: ChatServing {
        static let chatID = "synthetic-viewport-chat"
        private let complexMarkup: Bool
        private let transcriptRows: Bool
        private let historyRendering: Bool
        private var older: CheckedContinuation<APIPage<ConversationItem>, Error>?
        private var stream: AsyncThrowingStream<ResponseStreamEvent, Error>.Continuation?
        private var text = "Synthetic live reply."
        private var retired = false

        init(complexMarkup: Bool = false, transcriptRows: Bool = false, historyRendering: Bool = false) {
            self.complexMarkup = complexMarkup; self.transcriptRows = transcriptRows
            self.historyRendering = historyRendering
        }

        func createConversation(metadata: [String: JSONValue]?, projectId: String?, pinned: Bool?) throws
            -> Conversation
        {
            throw ChatIssue.uncertainResponse
        }
        func listConversations(_ parameters: ConversationsListParameters) -> APIPage<Conversation> { APIPage(data: []) }
        func listConversationItems(conversationId: String, parameters: ListParameters) async throws -> APIPage<
            ConversationItem
        > {
            guard !retired, conversationId == Self.chatID, parameters.order == .desc else {
                throw ChatIssue.historyUnavailable
            }
            if parameters.after == nil {
                if historyRendering { return Self.historyPage() }
                return transcriptRows
                    ? try Self.transcriptPage() : Self.page(20..<24, hasMore: true, complexMarkup: complexMarkup)
            }
            guard parameters.after == "viewport-user-20", older == nil else { throw ChatIssue.historyUnavailable }
            return try await withCheckedThrowingContinuation { older = $0 }
        }
        func releaseOlder() {
            let pending = older; older = nil
            pending?.resume(returning: Self.page(10..<20, hasMore: false, complexMarkup: complexMarkup))
        }
        /// Decode both API representations through the SDK item model. A
        /// conversation page uses function_call; retrieved Responses output
        /// uses tool_call. Both preserve string arguments and call_id pairing.
        private static func transcriptPage() throws -> APIPage<ConversationItem> {
            let json = #"""
                [
                  {"id":"viewport-user-20","type":"message","status":"completed","role":"user",
                   "content":[{"type":"input_text","text":"A synthetic message on the right"}]},
                  {"id":"viewport-reasoning-20","type":"reasoning","status":"completed",
                   "content":[{"type":"reasoning_text","text":"Synthetic tool research reasoning"}]},
                  {"id":"viewport-search-20","response_id":"synthetic-response","type":"function_call",
                   "status":"completed","role":"assistant","call_id":"synthetic-search","name":"web_search",
                   "arguments":"{\"query\":\"synthetic garden\"}"},
                  {"id":"viewport-search-result-20","response_id":"synthetic-response","type":"function_call_output",
                   "status":"completed","call_id":"synthetic-search","output":"Synthetic searchable garden result"},
                  {"id":"viewport-native-search-20","type":"web_search_call","status":"completed","role":"assistant"},
                  {"id":"viewport-open-20","response_id":"synthetic-response","type":"tool_call",
                   "status":"completed","role":"assistant","call_id":"synthetic-open","name":"open_urls",
                   "arguments":"{\"urls\":[\"https://example.com/garden\"]}"},
                  {"id":"viewport-open-result-20","response_id":"synthetic-response","type":"tool_output",
                   "status":"completed","call_id":"synthetic-open","output":"Synthetic opened page contents"},
                  {"id":"viewport-assistant-20","type":"message","status":"completed","role":"assistant",
                   "content":[{"type":"output_text","text":"The synthetic research reply stays on the left."}]}
                ]
                """#
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            let ascending = try decoder.decode([ConversationItem].self, from: Data(json.utf8))
            return APIPage(
                data: Array(ascending.reversed()), firstId: ascending.last?.id,
                lastId: ascending.first?.id, hasMore: false)
        }
        private static func page(_ indices: Range<Int>, hasMore: Bool, complexMarkup: Bool) -> APIPage<ConversationItem>
        {
            let ascending = indices.flatMap { index in
                [
                    ConversationItem(
                        id: "viewport-user-\(index)", type: "message", status: "completed", role: "user",
                        content: [MessageContent(type: "input_text", text: "Viewport reading anchor \(index)")]),
                    ConversationItem(
                        id: "viewport-assistant-\(index)", type: "message", status: "completed", role: "assistant",
                        content: [
                            MessageContent(
                                type: "output_text",
                                text:
                                    complexMarkup
                                    ? complexReply(index)
                                    : "Synthetic reply \(index). **Bold**, *emphasis*, and `inline code` use the shipping renderer.\n\nA second paragraph makes this turn taller than a plain list row."
                            )
                        ]),
                ]
            }
            return APIPage(
                data: Array(ascending.reversed()), firstId: ascending.last?.id,
                lastId: ascending.first?.id, hasMore: hasMore)
        }
        private static func complexReply(_ index: Int) -> String {
            """
            # Synthetic Garden Notes \(index)

            This **synthetic** response exercises *retained* Markdown, tables, and selectable code.

            > A retained chat keeps its reading position while a native menu moves across the screen.

            - [ ] Record the assistant's position
            - [x] Keep the same draft and runtime owners

            | Language | Description | Example |
            |:---|:---:|---:|
            | Swift | A deliberately wide synthetic column exercises horizontal overflow | 26 |
            | Arabic | مرحبا | 42 |

            ```swift
            let retainedDraft = "Synthetic native navigation"
            let position = CGPoint(x: 20, y: 100)
            ```

            ## A second section

            The menu previews underneath this screen. Releasing a short gesture returns the same content.

            This paragraph deliberately gives the accessibility container content beyond the visible viewport.

            Multilingual text remains selectable: مرحبا, こんにちは, שלום, 🌿.
            """
        }
        /// A completed multi-turn topic keeps rich replies taller than the
        /// viewport so ordinary history scrolling crosses lazy-row lifetimes.
        /// No account, parser, publication deadline or renderer is substituted.
        private static func historyPage() -> APIPage<ConversationItem> {
            let ascending = (20..<26).flatMap { index in
                [
                    ConversationItem(
                        id: "viewport-user-\(index)", type: "message", status: "completed", role: "user",
                        content: [
                            MessageContent(
                                type: "input_text", text: "Continue the synthetic garden discussion, turn \(index).")
                        ]),
                    ConversationItem(
                        id: "viewport-assistant-\(index)", type: "message", status: "completed", role: "assistant",
                        content: [MessageContent(type: "output_text", text: historyReply(index))]),
                ]
            }
            return APIPage(
                data: Array(ascending.reversed()), firstId: ascending.last?.id,
                lastId: ascending.first?.id, hasMore: false)
        }
        private static func historyReply(_ index: Int) -> String {
            """
            # Synthetic Garden Notes \(index)

            This **synthetic garden** reply continues the same topic with *formatted* notes and `native code`.

            [Garden reference \(index)](https://example.com/garden/\(index))

            ```swift
            let gardenTurn = \(index)
            let readingEarlierNotes = true
            ```

            ## Soil and water

            The earlier notes describe a small synthetic garden with several planting beds. Reading this discussion means moving between completed replies, comparing the soil notes, and returning to later details without losing the structure of either message.

            - Compare the **earlier plan** with this turn's watering notes.
            - Keep the *same topic* in view while checking another completed reply.
            - Read `gardenTurn` as selectable code rather than its Markdown fence.

            > Each follow-up adds more detail to the same synthetic discussion. A reader can pause anywhere in the transcript before continuing to an earlier or later turn.

            ## Seasonal observations

            A second long paragraph makes the reading target extend beyond the first screen on a phone. The synthetic beds have different shade and drainage, so the discussion records the same observations in several turns. These paragraphs exercise native text selection, wrapping, and ordinary vertical history scrolling.

            | Bed | Observation |
            |:---|:---|
            | North | Morning shade and steady moisture |
            | South | Afternoon sun and slower watering |

            The final note for turn \(index) remains part of this completed reply when the reader revisits the conversation.
            """
        }
        func createResponse(_ request: ResponsesCreateRequest, maximumInferenceSends: Int) throws
            -> AsyncThrowingStream<ResponseStreamEvent, Error>
        {
            guard !retired, stream == nil, request.conversation == Self.chatID else {
                throw ChatIssue.uncertainResponse
            }
            let pair = AsyncThrowingStream<ResponseStreamEvent, Error>.makeStream()
            stream = pair.continuation
            stream?.yield(.created(try lifecycle(status: .inProgress, output: [])))
            struct Item: Encodable { let outputIndex = 0; let item: ConversationItem }
            stream?.yield(.outputItemAdded(try event(Item(item: assistant(status: "in_progress")))))
            return pair.stream
        }
        func append(index: Int) throws {
            guard !retired, let stream else { throw CancellationError() }
            let delta = " Synthetic chunk \(index) grows the last reply while earlier text remains the reading target."
            text += delta
            struct Delta: Encodable {
                let itemId = "viewport-live-assistant"
                let outputIndex = 0
                let contentIndex = 0
                let delta: String
            }
            stream.yield(.outputTextDelta(try event(Delta(delta: delta))))
        }
        func finish() throws {
            guard !retired, let current = stream else { throw CancellationError() }
            current.yield(.completed(try lifecycle(status: .completed, output: [assistant(status: "completed")])))
            current.finish(); stream = nil
        }
        func appendReplyItem(index: Int) throws {
            guard !retired, let stream else { throw CancellationError() }
            let item = ConversationItem(
                id: "viewport-follow-item-\(index)", responseId: "viewport-response", type: "message",
                status: "in_progress", role: "assistant",
                content: [
                    MessageContent(
                        type: "output_text",
                        text: String(
                            repeating: "A synthetic appended reply block verifies following at the bottom. ", count: 12)
                    )
                ])
            struct Item: Encodable { let outputIndex: Int; let item: ConversationItem }
            stream.yield(.outputItemAdded(try event(Item(outputIndex: index + 1, item: item))))
        }
        private func assistant(status: String) -> ConversationItem {
            ConversationItem(
                id: "viewport-live-assistant", responseId: "viewport-response", type: "message", status: status,
                role: "assistant", content: [MessageContent(type: "output_text", text: text)])
        }
        private func lifecycle(status: ResponseStatus, output: [ConversationItem]) throws -> ResponseLifecycleEvent {
            struct Wrapper: Encodable { let response: ResearchResponse }
            return try event(
                Wrapper(
                    response: ResearchResponse(
                        id: "viewport-response", object: "response", createdAt: 0,
                        status: status, model: "auto:quick", output: output)))
        }
        private func event<T: Decodable, Value: Encodable>(_ value: Value) throws -> T {
            let encoder = JSONEncoder(); encoder.keyEncodingStrategy = .convertToSnakeCase
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(T.self, from: encoder.encode(value))
        }
        func abort() {
            retired = true
            older?.resume(throwing: CancellationError()); older = nil
            stream?.finish(throwing: CancellationError()); stream = nil
        }
        func retrieveResponse(id: String) throws -> ResearchResponse { throw ChatIssue.uncertainResponse }
        func cancelResponse(id: String) throws -> ResearchResponse { throw ChatIssue.cancellationUnavailable }
        func getConversationItem(conversationId: String, itemId: String) throws -> ConversationItem {
            throw ResearchAPIError.http(status: 404, reason: .notFound)
        }
    }

    private actor ShippingViewportStore: ChatPersistence {
        private var values: [String: ChatSnapshot] = [:]
        private var holdInitialLoad: Bool
        private var initialLoadEntered = false
        private var retired = false
        private var arrival: CheckedContinuation<Void, Never>?
        private var pendingLoad: (String, CheckedContinuation<ChatSnapshot?, Error>)?

        init(holdInitialLoad: Bool = false) { self.holdInitialLoad = holdInitialLoad }
        func seed(_ snapshot: ChatSnapshot) { values[snapshot.storageKey] = snapshot }
        func waitForInitialLoad() async {
            guard !initialLoadEntered, !retired else { return }
            await withCheckedContinuation { arrival = $0 }
        }
        func load(chatID: String, scope: AccountScope) async throws -> ChatSnapshot? {
            guard !retired else { throw CancellationError() }
            guard holdInitialLoad else { return values[chatID] }
            holdInitialLoad = false; initialLoadEntered = true
            return try await withCheckedThrowingContinuation { continuation in
                pendingLoad = (chatID, continuation)
                let waiter = arrival; arrival = nil; waiter?.resume()
            }
        }
        func releaseInitialLoad() {
            guard !retired, let pending = pendingLoad else { return }
            pendingLoad = nil
            pending.1.resume(returning: values[pending.0])
        }
        func cancelInitialLoad() {
            retired = true; holdInitialLoad = false
            let waiter = arrival; arrival = nil; waiter?.resume()
            let pending = pendingLoad; pendingLoad = nil
            pending?.1.resume(throwing: CancellationError())
        }
        func loadAll(scope: AccountScope) -> [ChatSnapshot] { Array(values.values) }
        func save(_ snapshot: ChatSnapshot, scope: AccountScope) { values[snapshot.storageKey] = snapshot }
        func remove(chatID: String, scope: AccountScope) { values.removeValue(forKey: chatID) }
        func rekey(from: String, to snapshot: ChatSnapshot, scope: AccountScope) {
            values.removeValue(forKey: from); values[snapshot.storageKey] = snapshot
        }
    }
    private actor ShippingViewportPreferences: PreferencesStoring {
        private var value = LocalPreferences()
        func activate(_ scope: AccountScope) -> LocalPreferences { value }
        func save(_ value: LocalPreferences, scope: AccountScope, revision: UInt64) { self.value = value }
        func retire(_ scope: AccountScope, removePersisted: Bool) {}
    }
    private actor ShippingViewportEntitlements: BillingServing, ModelCatalogServing {
        func subscriptionStatus() throws -> BillingStatus {
            try decode([
                "is_subscribed": true, "product_id": "synthetic", "product_name": "Pro",
                "subscription_status": "active", "can_chat": true, "chats_remaining": 50,
            ])
        }
        func teamStatus() throws -> TeamStatus {
            try decode(["has_team_subscription": false, "team_created": false])
        }
        func fetchCatalog() -> ModelCatalog {
            let pickerProof =
                ProcessInfo.processInfo.arguments.contains("--maple-shipping-viewport-fixture")
                && ProcessInfo.processInfo.arguments.contains("--maple-shipping-picker-proof")
            let capabilities = ModelCapabilities(chat: true, vision: false, reasoning: true, toolUse: pickerProof)
            let choices = [
                ("synthetic-quick", "Quick target", "Quick"),
                ("synthetic-powerful", "Powerful target", "Powerful"),
                ("synthetic-advanced-1", "Synthetic advanced 1", "Advanced 1"),
                ("synthetic-advanced-2", "Synthetic advanced 2", "Advanced 2"),
                ("synthetic-advanced-3", "Synthetic advanced 3", "Advanced 3"),
                ("synthetic-advanced-4", "Synthetic advanced 4", "Advanced 4"),
            ]
            return ModelCatalog(
                object: "list",
                data: choices.enumerated().filter { pickerProof || $0.offset == 0 }.map { index, choice in
                    ModelCatalogItem(
                        id: choice.0, object: "model", created: 0,
                        ownedBy: "synthetic", displayName: choice.1, shortName: choice.2,
                        description: pickerProof ? "Synthetic model choice for picker interaction" : nil,
                        contextWindow: 32768,
                        maxContextTokens: 32768,
                        access: "free", capabilities: capabilities, enabled: true, deprecated: false,
                        sortOrder: pickerProof ? index : nil)
                },
                aliases: [
                    ModelAlias(
                        id: .quick, label: "Quick", shortName: "Quick", description: "Synthetic quick model",
                        targetModel: "synthetic-quick", access: "free", capabilities: capabilities)
                ]
                    + (pickerProof
                        ? [
                            ModelAlias(
                                id: .powerful, label: "Powerful", shortName: "Powerful",
                                description: "Synthetic powerful model",
                                targetModel: "synthetic-powerful", access: "free", capabilities: capabilities)
                        ] : []),
                defaults: ModelDefaults(quick: .quick, powerful: .powerful),
                audio: AudioModelCatalog(
                    transcription: AudioModelAvailability(available: true, model: "synthetic-stt"),
                    speech: AudioModelAvailability(available: true, model: "synthetic-tts")))
        }
        func retire() {}
        private func decode<T: Decodable>(_ value: [String: Any]) throws -> T {
            let decoder = JSONDecoder(); decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try decoder.decode(T.self, from: JSONSerialization.data(withJSONObject: value))
        }
    }
#endif
