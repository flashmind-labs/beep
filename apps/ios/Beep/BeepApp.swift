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
    case settings, memories, routines
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [Route] = []
    @State private var drawer = false

    var body: some View {
        if model.onboarded {
            ZStack(alignment: .leading) {
                NavigationStack(path: $path) {
                    ChatView(drawer: $drawer)
                        .navigationDestination(for: Route.self) { route in
                            Group {
                                switch route {
                                case .settings: SettingsView(path: $path)
                                case .memories: MemoriesView(path: $path)
                                case .routines: RoutinesView(path: $path)
                                }
                            }
                            .toolbar(.hidden, for: .navigationBar)
                        }
                }
                if drawer {
                    Mint.ink.opacity(0.32).ignoresSafeArea()
                        .onTapGesture { withAnimation(.snappy) { drawer = false } }
                        .transition(.opacity)
                    ChatsDrawer(open: $drawer, path: $path)
                        .padding(.trailing, 40)
                        .transition(.move(edge: .leading))
                        .gesture(DragGesture().onEnded { drag in
                            if drag.translation.width < -60 { withAnimation(.snappy) { drawer = false } }
                        })
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
        case "chats": drawer = true
        case "settings": path = [.settings]
        case "memories": path = [.memories]
        case "routines": path = [.routines]
        default: break
        }
        guard let text = defaults.string(forKey: "send") else { return }
        while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
        model.send(text)
    }
    #endif
}
