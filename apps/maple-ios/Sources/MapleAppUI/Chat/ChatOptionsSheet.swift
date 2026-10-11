#if os(iOS)
    import MapleAppCore
    import SwiftUI
    import UIKit

    /// Delegate to the platform sizing from the presenter's width, keeping one
    /// presentation type while project navigation changes the sheet contents.
    struct AdaptiveChatSheetSizing: PresentationSizing {
        let regularWidth: Bool

        func proposedSize(
            for root: PresentationSizingRoot,
            context: PresentationSizingContext
        ) -> ProposedViewSize {
            let page = PagePresentationSizing.page.proposedSize(for: root, context: context)
            if regularWidth {
                let form = FormPresentationSizing.form.proposedSize(for: root, context: context)
                // Retain the native form width and outside space while giving
                // the scrollable model catalog the native page's vertical room.
                return ProposedViewSize(width: form.width, height: page.height)
            }
            return page
        }
    }

    /// One native sheet owns the model, web-search and project choices. Project
    /// navigation uses this stack instead of presenting another picker sheet.
    struct ChatOptionsSheet: View {
        @Bindable var entitlements: AccountEntitlements
        @Bindable var preferences: PreferencesController
        @Bindable var library: LibraryModel
        let regularWidth: Bool
        let selectedProjectID: String?
        let makeInstructionEditor: @MainActor (InstructionDestination) -> InstructionEditor
        let isCurrent: @MainActor () -> Bool
        let onSelectProject: @MainActor (String?) async -> Bool
        let onSelectModel: @MainActor (String) -> Void
        @Environment(\.dismiss) private var dismiss
        @State private var path: [ProjectSheetDestination] = []
        @State private var selectedDetent: PresentationDetent = .fraction(0.6)

        var body: some View {
            NavigationStack(path: $path) {
                SettingsModelPicker(
                    entitlements: entitlements,
                    webSearchPreference: Binding(
                        get: { preferences.value.webSearchPreferred },
                        set: { if isCurrent() { preferences.setWebSearchPreferred($0) } }),
                    webSearchPreferences: preferences,
                    projectRow: AnyView(projectRow), canInteract: isCurrent
                ) { id in
                    guard isCurrent() else { return }
                    onSelectModel(id)
                }
                .ignoresSafeArea(.container, edges: .top)
                .background(
                    OptionsRootNavigationBarPassThrough(active: path.isEmpty && isCurrent(), isCurrent: isCurrent)
                        .frame(width: 0, height: 0)
                )
                .navigationBarTitleDisplayMode(.inline)
                .toolbarVisibility(.visible, for: .navigationBar)
                .toolbarBackground(.hidden, for: .navigationBar)
                .navigationDestination(for: ProjectSheetDestination.self) { destination in
                    ProjectSheetDestinationView(
                        destination: destination, library: library, selectedProjectID: selectedProjectID,
                        makeInstructionEditor: makeInstructionEditor, isCurrent: isCurrent,
                        onSelection: selectProject)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .presentationDetents([.fraction(0.6), .fraction(0.98)], selection: $selectedDetent)
            .presentationDragIndicator(.visible)
            .presentationContentInteraction(.resizes)
            .presentationBackgroundInteraction(.disabled)
            .presentationSizing(AdaptiveChatSheetSizing(regularWidth: regularWidth))
            .presentationBackground(MapleSurface.canvas)
            .background(
                SheetOutsideTapDismissal {
                    // Editors retain their discard and in-flight safeguards.
                    if (path.last?.permitsOutsideDismissal ?? true), !library.isMutating { dismiss() }
                }.frame(width: 0, height: 0)
            )
            .accessibilityIdentifier("chat.options-sheet")
        }

        private var projectRow: some View {
            NavigationLink(value: ProjectSheetDestination.projects) {
                HStack(spacing: 12) {
                    Image(systemName: "folder").foregroundStyle(MapleAccent.foreground)
                    libraryText("Project").foregroundStyle(.primary)
                    Spacer(minLength: 8)
                    if let project = library.projects.first(where: { $0.id == selectedProjectID }) {
                        Text(verbatim: project.name).foregroundStyle(.secondary).lineLimit(1)
                    } else {
                        libraryText("No project").foregroundStyle(.secondary)
                    }
                }
                .font(MapleTypography.button)
                .frame(maxWidth: .infinity, minHeight: 44)
                .contentShape(Rectangle())
            }
            .disabled(!isCurrent() || library.isMutating)
            .accessibilityIdentifier("chat-options.project")
        }

        private func selectProject(_ id: String?) async -> Bool {
            guard isCurrent(), await onSelectProject(id), isCurrent() else { return false }
            path = []; return true
        }
    }

    /// Keep native navigation-bar geometry stable while the options root has
    /// no bar controls. Its content can use that space without losing taps.
    private struct OptionsRootNavigationBarPassThrough: UIViewControllerRepresentable {
        let active: Bool
        let isCurrent: @MainActor () -> Bool

        func makeUIViewController(context: Context) -> Controller {
            Controller(active: active, isCurrent: isCurrent)
        }

        func updateUIViewController(_ controller: Controller, context: Context) {
            controller.active = active
            controller.isCurrent = isCurrent
            controller.updateInteraction()
        }

        static func dismantleUIViewController(_ controller: Controller, coordinator: ()) {
            controller.restoreInteraction()
        }

        final class Controller: UIViewController {
            var active: Bool
            var isCurrent: @MainActor () -> Bool
            private var visible = false
            private weak var ownedNavigation: UINavigationController?
            private weak var ownedBar: UINavigationBar?
            private var previousInteraction: Bool?

            init(active: Bool, isCurrent: @escaping @MainActor () -> Bool) {
                self.active = active; self.isCurrent = isCurrent
                super.init(nibName: nil, bundle: nil)
            }

            @available(*, unavailable)
            required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

            override func loadView() {
                let observer = UIView()
                observer.backgroundColor = .clear
                observer.isUserInteractionEnabled = false
                view = observer
            }

            override func viewWillAppear(_ animated: Bool) {
                super.viewWillAppear(animated)
                visible = true; updateInteraction()
            }

            override func viewDidLayoutSubviews() {
                super.viewDidLayoutSubviews()
                updateInteraction()
            }

            override func viewWillDisappear(_ animated: Bool) {
                super.viewWillDisappear(animated)
                visible = false; restoreInteraction()
            }

            func updateInteraction() {
                guard visible, active, isCurrent(), let navigation = owningNavigationController() else {
                    restoreInteraction(); return
                }
                let bar = navigation.navigationBar
                if ownedNavigation !== navigation || ownedBar !== bar {
                    restoreInteraction()
                    ownedNavigation = navigation; ownedBar = bar
                    previousInteraction = bar.isUserInteractionEnabled
                }
                bar.isUserInteractionEnabled = false
            }

            func restoreInteraction() {
                if let navigation = ownedNavigation, let bar = ownedBar,
                    navigation.navigationBar === bar,
                    let previousInteraction
                {
                    bar.isUserInteractionEnabled = previousInteraction
                }
                ownedNavigation = nil; ownedBar = nil; previousInteraction = nil
            }

            private func owningNavigationController() -> UINavigationController? {
                var candidate: UIViewController? = self
                while let controller = candidate {
                    if let navigation = controller as? UINavigationController { return navigation }
                    if let navigation = controller.navigationController { return navigation }
                    candidate = controller.parent
                }
                return nil
            }
        }
    }
#endif
