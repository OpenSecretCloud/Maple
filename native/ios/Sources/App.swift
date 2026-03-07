import SwiftUI

@main
struct MapleApp: App {
    @State private var manager = AppManager()

    var body: some Scene {
        WindowGroup {
            ContentView(manager: manager)
        }
    }
}
