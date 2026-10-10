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
    case groupInfo(String)
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [Route] = []
    @State private var drawer = false
    @State private var sheet: GroupSheet?
    /// An invite being looked at, shown full screen.
    @State private var joining: String?

    var body: some View {
        if model.onboarded {
            ZStack(alignment: .leading) {
                NavigationStack(path: $path) {
                    main
                        .navigationDestination(for: Route.self) { route in
                            SwiftUI.Group {
                                switch route {
                                case .settings: SettingsView(path: $path)
                                case .memories: MemoriesView(path: $path)
                                case .routines: RoutinesView(path: $path)
                                case .groupInfo(let id):
                                    GroupInfoView(groupId: id, back: { path.removeAll() }) { sheet = .invite(id: $0.id, name: $0.name) }
                                }
                            }
                            .toolbar(.hidden, for: .navigationBar)
                        }
                }
                if drawer {
                    Mint.ink.opacity(0.32).ignoresSafeArea()
                        .onTapGesture { withAnimation(.snappy) { drawer = false } }
                        .transition(.opacity)
                    ChatsDrawer(open: $drawer, path: $path, sheet: $sheet)
                        .padding(.trailing, 40)
                        .transition(.move(edge: .leading))
                        .gesture(DragGesture().onEnded { drag in
                            if drag.translation.width < -60 { withAnimation(.snappy) { drawer = false } }
                        })
                }
            }
            .tint(Mint.brand)
            .sheet(item: $sheet) { shown in
                switch shown {
                case .new:
                    NewGroupSheet { group in
                        model.openGroup(group.id)
                        path.removeAll()
                        sheet = .invite(id: group.id, name: group.name)
                    } joinCode: {
                        sheet = .code
                    }
                case .code:
                    JoinCodeSheet { token in
                        sheet = nil
                        Task {
                            try? await Task.sleep(for: .milliseconds(400))
                            joining = token
                        }
                    }
                case .invite(let id, let name):
                    GroupInviteSheet(groupId: id, groupName: name)
                }
            }
            .fullScreenCover(item: Binding(get: { joining.map(JoinToken.init) }, set: { joining = $0?.id })) { invite in
                GroupJoinView(token: invite.id) { id in
                    joining = nil
                    path.removeAll()
                    model.openGroup(id)
                } back: {
                    joining = nil
                }
            }
            #if DEBUG
            .task { await debugLaunch() }
            #endif
        } else {
            OnboardingView()
        }
    }

    @ViewBuilder private var main: some View {
        if let id = model.openGroupId {
            GroupChatView(groupId: id, drawer: $drawer, info: { path = [.groupInfo(id)] }) {
                sheet = .invite(id: $0.id, name: $0.name)
            }
            .id(id)
        } else {
            ChatView(drawer: $drawer)
        }
    }

    #if DEBUG
    /// Lets the simulator be driven without taps: `-open chats|settings|memories|routines`
    /// opens a screen and `-send "text"` sends a message once connected. `group`, `groupinfo`
    /// and `invite` open the first group, `newgroup` and `joingroup` the group sheets.
    /// `-creategroup "name"` makes a group and `-leavegroup 1` leaves the first one.
    private func debugLaunch() async {
        let defaults = UserDefaults.standard
        if let name = defaults.string(forKey: "creategroup") {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            if let group = try? await model.createGroup(name: name, memberName: "Simulator") { model.openGroup(group.id) }
        }
        if defaults.bool(forKey: "leavegroup") {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            await model.refreshGroups()
            if let group = model.groups.first { try? await model.leaveGroup(group.id) }
        }
        switch defaults.string(forKey: "open") {
        case "chats": drawer = true
        case "settings": path = [.settings]
        case "memories": path = [.memories]
        case "routines": path = [.routines]
        case "newgroup": sheet = .new
        case "joingroup": sheet = .code
        case "group", "groupinfo", "invite":
            while model.groups.isEmpty { try? await Task.sleep(for: .milliseconds(200)) }
            let group = model.groups[0]
            model.openGroup(group.id)
            if defaults.string(forKey: "open") == "groupinfo" { path = [.groupInfo(group.id)] }
            if defaults.string(forKey: "open") == "invite" { sheet = .invite(id: group.id, name: group.name) }
        default: break
        }
        guard let text = defaults.string(forKey: "send") else { return }
        while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
        if let id = model.openGroupId {
            try? await model.sendToGroup(id, requestId: UUID().uuidString.lowercased(), text: text)
        } else {
            model.send(text)
        }
    }
    #endif
}

private struct JoinToken: Identifiable {
    let id: String
}
