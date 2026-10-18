import SwiftUI

@main
struct BeepApp: App {
    @UIApplicationDelegateAdaptor private var delegate: AppDelegate

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(delegate.model)
                .task { delegate.model.connect() }
                .onOpenURL { url in
                    if url.scheme == "beep", url.host == "new" { delegate.model.destination = .newChat }
                }
        }
    }
}

/// Owns the model so its location manager exists as soon as iOS starts the app,
/// also in the background for a saved place's region event, when no window shows.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate {
    let model = AppModel()

    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        if options?[.location] != nil { model.connect() }
        return true
    }
}

enum Route: Hashable {
    case groupInfo(String)
    case settings, memories, routines, skills, places, locationAlways
    case skill(String)
    /// Memories, opened on the newest read.
    case latestRead
    /// Routines with the new routine sheet open on arriving at a place.
    case placeRoutine(String)
}

/// The app opens on a chat with Beep, as on Android. The chats and groups are in the drawer
/// behind the menu button, and the other screens push over the chat.
struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [Route] = []
    @State private var sheet: GroupSheet?
    @State private var drawer = false
    /// An invite being looked at, shown full screen.
    @State private var joining: String?
    /// The chat left by starting a new one, so a swipe right can go back to it once. It only
    /// holds while the user stays on the new chat; opening another one forgets it.
    @State private var left: String?
    @State private var trailEnd: String?

    var body: some View {
        if model.onboarded {
            NavigationStack(path: $path) {
                ChatsDrawer(open: $drawer, openChat: { openChat($0) }, openGroup: openGroup,
                            newGroup: { sheet = .new }, invite: { sheet = .invite(id: $0.id, name: $0.name) },
                            go: { path.append($0) }) {
                    chatPane
                }
                .toolbar(.hidden, for: .navigationBar)
                .navigationDestination(for: Route.self) { route in
                    screen(route).toolbar(.hidden, for: .navigationBar)
                }
            }
            .tint(Mint.brand)
            .sheet(item: $sheet) { shown in
                switch shown {
                case .new:
                    NewGroupSheet { group in
                        drawer = false
                        openGroup(group.id)
                        sheet = .invite(id: group.id, name: group.name)
                    } joinCode: {
                        sheet = .code
                    }
                case .code:
                    JoinCodeSheet { token in
                        sheet = nil
                        drawer = false
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
                    openGroup(id)
                } back: {
                    joining = nil
                }
            }
            .task {
                #if DEBUG
                // `-noask 1` keeps the permission prompt off screenshots.
                if UserDefaults.standard.bool(forKey: "noask") { return }
                #endif
                model.notifier.requestPermission()
            }
            .onChange(of: model.destination, initial: true) { follow() }
            #if DEBUG
            .task { await debugLaunch() }
            #endif
        } else {
            OnboardingView()
        }
    }

    /// The group on screen, or the chat with Beep. Swipes work the same in both: left starts
    /// a new chat, right goes back to the chat before or opens the drawer.
    @ViewBuilder private var chatPane: some View {
        if let id = model.openGroup {
            SwipeChats(canStartNew: true, hasPrevious: true, onNew: startNewChat, onBack: { model.openGroup = nil }) {
                GroupChatView(groupId: id, menu: { drawer = true }, back: { model.openGroup = nil },
                              info: { path.append(.groupInfo(id)) }) {
                    sheet = .invite(id: $0.id, name: $0.name)
                }
                .id(id)
            }
        } else {
            SwipeChats(canStartNew: !model.chat.messages.isEmpty, hasPrevious: previousChat != nil,
                       onNew: startNewChat, onBack: goBack) {
                ChatView(menu: { drawer = true }, newChat: startNewChat)
            }
        }
    }

    @ViewBuilder private func screen(_ route: Route) -> some View {
        switch route {
        case .groupInfo(let id):
            GroupInfoView(groupId: id, back: { path.removeLast() }, left: {
                model.openGroup = nil
                path.removeAll()
            }) { name in
                model.pendingMention = name
                path.removeLast()
            } invite: {
                sheet = .invite(id: $0.id, name: $0.name)
            }
        case .settings: SettingsView(path: $path)
        case .memories: MemoriesView(path: $path)
        case .latestRead: MemoriesView(path: $path, latestRead: true)
        case .routines: RoutinesView(path: $path)
        case .skills: SkillsView(path: $path)
        case .skill(let id): SkillView(id: id, back: { path.removeLast() })
        case .placeRoutine(let id):
            RoutinesView(path: $path, startAt: model.places.first { $0.id == id })
        case .places:
            PlacesView(back: { path.removeLast() }, always: { path.append(.locationAlways) }) { place in
                path.append(.placeRoutine(place.id))
            }
        case .locationAlways:
            LocationAlwaysView(close: { path.removeLast() }) { path = [.places] }
        }
    }

    private func openChat(_ session: Session?) {
        left = nil
        model.openGroup = nil
        if let session { model.open(session) } else { model.newChat() }
        path = []
    }

    private func openGroup(_ id: String) {
        model.openGroup = id
        path = []
    }

    private func startNewChat() {
        if model.openGroup != nil {
            model.openGroup = nil
            if !model.chat.messages.isEmpty { model.newChat() }
            return
        }
        guard !model.chat.messages.isEmpty else { return }
        left = model.chat.id
        model.newChat()
        trailEnd = model.chat.id
    }

    private var previousChat: Session? {
        guard trailEnd == model.chat.id, let left, left != model.chat.id else { return nil }
        return model.sessions.first { $0.id == left }
    }

    private func goBack() {
        guard let previous = previousChat else {
            drawer = true
            return
        }
        left = nil
        model.open(previous)
        trailEnd = previous.id
    }

    /// Opens the chat or group a tapped notification belongs to.
    private func follow() {
        guard let destination = model.destination else { return }
        model.destination = nil
        drawer = false
        switch destination {
        case .chat(let id):
            left = nil
            model.openGroup = nil
            model.openChat(id)
            path = []
        case .group(let id):
            openGroup(id)
        case .placeRoutine(let id):
            path.append(.placeRoutine(id))
        case .latestRead:
            path = [.latestRead]
        case .newChat:
            // An empty chat already open is the new chat.
            if !(path.isEmpty && model.openGroup == nil && model.chat.messages.isEmpty) { openChat(nil) }
            model.composeNext = true
        }
    }

    #if DEBUG
    /// Lets the simulator be driven without taps: `-open chat|settings|memories|routines|skills|places`
    /// opens a screen and `-send "text"` sends a message once connected. `drawer` opens the chat
    /// list. `group`, `groupinfo` and `invite` open the first group, `photos` the photo sheet in a
    /// new chat, `newgroup` and `joingroup` the sheets, `locationalways` the location ask. `-places sample` swaps the
    /// server's places for made up ones, and `placesave` shows a chat with a saved place card.
    /// `widget` takes the widget's path to a new chat with the keyboard up.
    /// `-creategroup "name"` makes a group and `-leavegroup 1` leaves the first one.
    private func debugLaunch() async {
        let defaults = UserDefaults.standard
        if let name = defaults.string(forKey: "creategroup") {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            if let group = try? await model.createGroup(name: name, memberName: "Simulator") { openGroup(group.id) }
        }
        if defaults.bool(forKey: "leavegroup") {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            await model.refreshGroups()
            if let group = model.groups.first { try? await model.leaveGroup(group.id) }
        }
        if defaults.string(forKey: "places") == "sample" {
            model.placesStore = SamplePlaces()
            await model.refreshPlaces()
        }
        switch defaults.string(forKey: "open") {
        case "chat", "newchat", "photos": openChat(nil)
        case "drawer":
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            drawer = true
        case "widget": model.destination = .newChat
        case "products":
            openChat(nil)
            model.chat.messages = sampleProducts
        case "placesave":
            openChat(nil)
            model.chat.messages = samplePlaceSave
        case "settings": path = [.settings]
        case "memories": path = [.memories]
        case "routines": path = [.routines]
        case "skills": path = [.skills]
        case "places": path = [.places]
        case "placeroutine":
            while model.places.isEmpty { try? await Task.sleep(for: .milliseconds(200)) }
            path = [.places, .placeRoutine(model.places[0].id)]
        case "locationalways": path = [.locationAlways]
        case "newgroup": sheet = .new
        case "joingroup": sheet = .code
        case "group", "groupinfo", "invite":
            while model.groups.isEmpty { try? await Task.sleep(for: .milliseconds(200)) }
            let group = model.groups[0]
            openGroup(group.id)
            if defaults.string(forKey: "open") == "groupinfo" { path.append(.groupInfo(group.id)) }
            if defaults.string(forKey: "open") == "invite" { sheet = .invite(id: group.id, name: group.name) }
        default: break
        }
        guard let text = defaults.string(forKey: "send") else { return }
        while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
        if let id = model.openGroup {
            try? await model.sendToGroup(id, requestId: UUID().uuidString.lowercased(), text: text)
        } else {
            model.send(text)
        }
    }
    #endif
}

#if DEBUG
/// A made up shopping reply for `-open products`, so the cards can be checked without a server.
private let sampleProducts: [Message] = {
    let block = { (title: String, price: String, was: String?, store: String, rating: Double) in
        "```product\n{\"title\": \"\(title)\", \"url\": \"https://example.com/\(title.count)\", \"price\": \"\(price)\""
            + (was.map { ", \"was\": \"\($0)\"" } ?? "")
            + ", \"store\": \"\(store)\", \"rating\": \(rating), \"reviews\": 1204, \"note\": \"Light\"}\n```"
    }
    let reply = "Three under 200 euros:\n\n"
        + [block("Stick vacuum one", "149 €", "189 €", "Shop A", 4.6),
           block("Stick vacuum two with a long name", "179 €", nil, "Shop B", 4.3),
           block("Stick vacuum three", "199 €", nil, "Shop C", 4.1)].joined(separator: "\n")
        + "\n\nThe first is the lightest.\n\n```product\nnot json\n```"
    return [
        Message(id: "u", role: .user, text: "Find me a stick vacuum under 200 euros"),
        Message(id: "a", role: .assistant, text: reply),
    ]
}()
#endif

#if DEBUG
/// A made up "I'm at the gym" reply for `-open placesave`, to check the place card.
private let samplePlaceSave: [Message] = {
    let output = #"{"saved": true, "moved": false, "place": {"id": "gym", "name": "Gym", "address": "40 Sample Road", "radius_m": 100, "latitude": 41.399, "longitude": 2.197, "here_now": true, "last_here_at": null, "routines": 0}}"#
    var reply = Message(id: "a", role: .assistant,
                        text: "Saved this spot as **Gym**. I'll notice when you get here or leave.")
    reply.tools = [ToolCall(id: "l", name: "get_location", done: true),
                   ToolCall(id: "p", name: "place_save", done: true, output: output)]
    return [Message(id: "u", role: .user, text: "I'm at the gym"), reply]
}()
#endif

private struct JoinToken: Identifiable {
    let id: String
}
