import SwiftUI

@main
struct BeepApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .task { model.connect() }
        }
    }
}

enum Route: Hashable {
    case chats, settings, memories, routines
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [Route] = []

    var body: some View {
        if model.onboarded {
            NavigationStack(path: $path) {
                ChatView(path: $path)
                    .navigationDestination(for: Route.self) { route in
                        switch route {
                        case .chats: SessionsView(path: $path)
                        case .settings: SettingsView()
                        case .memories: MemoriesView()
                        case .routines: RoutinesView(path: $path)
                        }
                    }
            }
            .tint(Mint.brand)
            #if DEBUG
            .task { await debugLaunch() }
            #endif
        } else {
            OnboardingView()
        }
    }

    #if DEBUG
    /// Lets the simulator be driven without taps: `-open chats|settings|memories|routines`
    /// opens a screen and `-send "text"` sends a message once connected.
    private func debugLaunch() async {
        let defaults = UserDefaults.standard
        switch defaults.string(forKey: "open") {
        case "chats": path = [.chats]
        case "settings": path = [.settings]
        case "memories": path = [.settings, .memories]
        case "routines": path = [.settings, .routines]
        default: break
        }
        guard let text = defaults.string(forKey: "send") else { return }
        while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
        model.send(text)
    }
    #endif
}
