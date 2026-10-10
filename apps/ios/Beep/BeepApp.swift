import SwiftUI

@main
struct BeepApp: App {
    @UIApplicationDelegateAdaptor private var delegate: AppDelegate

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(delegate.model)
                .task { delegate.model.connect() }
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
    case chat
    case group(String)
    case groupInfo(String)
    case settings, memories, routines, places, locationAlways
    /// Memories, opened on the newest read.
    case latestRead
    /// Routines with the new routine sheet open on arriving at a place.
    case placeRoutine(String)
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var path: [Route] = []
    @State private var sheet: GroupSheet?
    /// An invite being looked at, shown full screen.
    @State private var joining: String?

    var body: some View {
        if model.onboarded {
            NavigationStack(path: $path) {
                HomeView(openChat: openChat, openGroup: openGroup, go: { path.append($0) }, sheet: { sheet = $0 })
                    .navigationDestination(for: Route.self) { route in
                        screen(route).toolbar(.hidden, for: .navigationBar)
                    }
            }
            .tint(Mint.brand)
            .sheet(item: $sheet) { shown in
                switch shown {
                case .start:
                    StartSheet {
                        sheet = nil
                        model.newChat()
                        path = [.chat]
                    } newGroup: {
                        sheet = .new
                    } joinCode: {
                        sheet = .code
                    }
                case .new:
                    NewGroupSheet { group in
                        openGroup(group.id)
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

    @ViewBuilder private func screen(_ route: Route) -> some View {
        switch route {
        case .chat:
            ChatView(back: home)
        case .group(let id):
            GroupChatView(groupId: id, back: home, info: { path.append(.groupInfo(id)) }) {
                sheet = .invite(id: $0.id, name: $0.name)
            }
            .id(id)
        case .groupInfo(let id):
            GroupInfoView(groupId: id, back: { path.removeLast() }, left: home) { name in
                model.pendingMention = name
                path.removeLast()
            } invite: {
                sheet = .invite(id: $0.id, name: $0.name)
            }
        case .settings: SettingsView(path: $path)
        case .memories: MemoriesView(path: $path)
        case .latestRead: MemoriesView(path: $path, latestRead: true)
        case .routines: RoutinesView(path: $path)
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

    private func home() { path.removeAll() }

    private func openChat(_ session: Session?) {
        if let session { model.open(session) } else { model.newChat() }
        path = [.chat]
    }

    private func openGroup(_ id: String) {
        model.openGroup(id)
        path = [.group(id)]
    }

    /// Opens the chat or group a tapped notification belongs to.
    private func follow() {
        guard let destination = model.destination else { return }
        model.destination = nil
        switch destination {
        case .chat(let id):
            model.openChat(id)
            path = [.chat]
        case .group(let id):
            openGroup(id)
        case .placeRoutine(let id):
            path.append(.placeRoutine(id))
        case .latestRead:
            path = [.latestRead]
        }
    }

    #if DEBUG
    /// Lets the simulator be driven without taps: `-open chat|settings|memories|routines|places`
    /// opens a screen and `-send "text"` sends a message once connected. The list shows without
    /// `-open`. `group`, `groupinfo` and `invite` open the first group, `photos` the photo sheet in a
    /// new chat, `start`, `newgroup` and `joingroup` the sheets, `locationalways` the location ask. `-places sample` swaps the
    /// server's places for made up ones, and `placesave` shows a chat with a saved place card.
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
        case "products":
            openChat(nil)
            model.chat.messages = sampleProducts
        case "placesave":
            openChat(nil)
            model.chat.messages = samplePlaceSave
        case "settings": path = [.settings]
        case "memories": path = [.memories]
        case "routines": path = [.routines]
        case "places": path = [.places]
        case "placeroutine":
            while model.places.isEmpty { try? await Task.sleep(for: .milliseconds(200)) }
            path = [.places, .placeRoutine(model.places[0].id)]
        case "locationalways": path = [.locationAlways]
        case "start": sheet = .start
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
        if case .group(let id) = path.last {
            try? await model.sendToGroup(id, requestId: UUID().uuidString.lowercased(), text: text)
        } else {
            if path.last != .chat { openChat(nil) }
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
