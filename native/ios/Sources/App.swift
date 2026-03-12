import SwiftUI

@main
struct MapleApp: App {
    @State private var manager = AppManager()

    var body: some Scene {
        WindowGroup {
            ContentView(manager: manager)
#if os(macOS)
                .frame(minWidth: 430, minHeight: 760)
#endif
        }
#if os(macOS)
        .defaultSize(width: 430, height: 900)
        .windowStyle(.titleBar)
        .windowToolbarStyle(.unifiedCompact(showsTitle: false))
        .windowBackgroundDragBehavior(.enabled)
        .windowResizability(.contentMinSize)
#endif
    }
}
