import SwiftUI

@main
struct DMNEAlphaApp: App {
    @StateObject private var desk = DeskViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(desk)
                .preferredColorScheme(.dark)
        }
    }
}
