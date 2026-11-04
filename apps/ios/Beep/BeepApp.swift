import SwiftUI

@main
struct BeepApp: App {
    @UIApplicationDelegateAdaptor private var delegate: AppDelegate
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(delegate.model)
                .task { delegate.model.connect() }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active { delegate.model.ensureConnected() }
                }
                .onOpenURL { url in
                    if url.scheme == "beep", url.host == "new" { delegate.model.destination = .newChat }
                    // The Talk widget and the Live Activity. With live voice running already,
                    // bringing the app up is all it does.
                    if url.scheme == "beep", url.host == "live", !delegate.model.live.active {
                        delegate.model.live.start(from: .control)
                    }
                    if let token = shareToken(url: url.absoluteString) { delegate.model.destination = .shared(token) }
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
        // An App Intent may be waiting to start live voice.
        LiveLauncher.attach(model)
        // A token needs no permission; showing what it brings does, which onboarding asks.
        application.registerForRemoteNotifications()
        return true
    }

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) {
        model.pushToken = token.map { String(format: "%02x", $0) }.joined()
    }

    /// A background push from the server: it has notices it could not send over the connection.
    func application(_ application: UIApplication,
                     didReceiveRemoteNotification userInfo: [AnyHashable: Any]) async -> UIBackgroundFetchResult {
        await model.wake()
        return .newData
    }
}

enum Route: Hashable {
    case groupInfo(String)
    case settings, memories, routines, skills, places, locationAlways, apps, devices, machines
    case skill(String)
    /// Memories, opened on the newest read.
    case latestRead
    /// Routines with the new routine sheet open on arriving at a place.
    case placeRoutine(String)
    case researches
    /// What routines may do without asking: every routine's, or one routine's by its id.
    case grants(String?)
    /// A research chat's workspace, on a tab or the one that fits.
    case workspace(String, WorkspaceTab?)
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
    /// A shared chat being read, shown full screen.
    @State private var reading: String?
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
                .edgeSwipeBack { joining = nil }
            }
            .fullScreenCover(item: Binding(get: { reading.map(JoinToken.init) }, set: { reading = $0?.id })) { shared in
                SharedChatView(token: shared.id) {
                    reading = nil
                } continued: {
                    reading = nil
                    left = nil
                    path = []
                }
                .edgeSwipeBack { reading = nil }
            }
            .task {
                #if DEBUG
                // `-noask 1` keeps the permission prompt off screenshots.
                if UserDefaults.standard.bool(forKey: "noask") { return }
                #endif
                model.notifier.requestPermission()
            }
            .onChange(of: model.destination, initial: true) { follow() }
            .onChange(of: model.live.launches) { uncoverChat() }
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
                ChatView(menu: { drawer = true }, newChat: startNewChat,
                         workspace: { path.append(.workspace(model.chat.id, $0)) })
            }
        }
    }

    @ViewBuilder private func screen(_ route: Route) -> some View {
        switch route {
        case .groupInfo(let id):
            GroupInfoView(groupId: id, back: { pop() }, left: {
                model.openGroup = nil
                path.removeAll()
            }) { name in
                model.pendingMention = name
                pop()
            } invite: {
                sheet = .invite(id: $0.id, name: $0.name)
            }
        case .settings: SettingsView(path: $path)
        case .devices: LinkedDevicesView(back: { pop() })
        case .machines: MachinesView(back: { pop() })
        case .apps: AppsView(back: { pop() })
        case .memories: MemoriesView(path: $path)
        case .latestRead: MemoriesView(path: $path, latestRead: true)
        case .routines: RoutinesView(path: $path)
        case .grants(let routine): GrantsView(routine: routine, back: { pop() })
        case .skills: SkillsView(path: $path)
        case .skill(let id): SkillView(id: id, back: { pop() })
        case .placeRoutine(let id):
            RoutinesView(path: $path, startAt: model.places.first { $0.id == id })
        case .places:
            PlacesView(back: { pop() }, always: { path.append(.locationAlways) }) { place in
                path.append(.placeRoutine(place.id))
            }
        case .researches:
            ResearchList(back: { pop() }, open: { id in
                left = nil
                model.openGroup = nil
                model.openChat(id)
                path = []
            }, start: { topic in
                left = nil
                model.startResearch(topic)
                path = []
            })
        case .workspace(let id, let tab):
            ResearchWorkspace(id: id, start: tab, back: { pop() }) { text in
                model.pendingQuote = text
                path = []
            }
        case .locationAlways:
            LocationAlwaysView(close: { pop() }) { path = [.places] }
        }
    }

    /// Goes back one screen. A second tap on back while the first pop animates finds the
    /// path already shorter, and `removeLast` on an empty path traps.
    private func pop() {
        if !path.isEmpty { path.removeLast() }
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

    /// Live voice talks in the chat, so starting it closes what covers the chat: pushed screens,
    /// the drawer, sheets, full screen covers and the browser. Views further in close their own
    /// with `closesForLive`.
    private func uncoverChat() {
        path = []
        drawer = false
        sheet = nil
        joining = nil
        reading = nil
        model.browserOpen = false
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
        case .shared(let token):
            reading = token
        }
    }

    #if DEBUG
    /// Lets the simulator be driven without taps: `-open chat|settings|memories|routines|skills|places`
    /// opens a screen and `-send "text"` sends a message once connected. `drawer` opens the chat
    /// list. `group`, `groupinfo` and `invite` open the first group, `photos` the photo sheet in a
    /// new chat, `newgroup` and `joingroup` the sheets, `locationalways` the location ask. `-places sample` swaps the
    /// server's places for made up ones, and `placesave` shows a chat with a saved place card.
    /// `widget` takes the widget's path to a new chat with the keyboard up.
    /// `apps` opens the apps, `shared` with `-token t` the shared chat behind a link.
    /// `researches` opens the research list, `research` and `workspace` the newest research, and
    /// `-research "topic"` starts one.
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
        // The newest chat from the saved list, without waiting for the server.
        case "last": openChat(model.sessions.max { $0.updatedAt < $1.updatedAt })
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
        case "cards", "cardstop":
            openChat(nil)
            model.chat.messages = sampleCards(top: defaults.string(forKey: "open") == "cardstop")
        case "settings": path = [.settings]
        case "apps": path = [.apps]
        case "devices": path = [.settings, .devices]
        case "machines": path = [.settings, .machines]
        case "shared":
            if let token = defaults.string(forKey: "token") { reading = token }
        case "memories": path = [.memories]
        case "routines": path = [.routines]
        case "grants": path = [.settings, .grants(nil)]
        case "skills": path = [.skills]
        case "places": path = [.places]
        case "researches": path = [.researches]
        case "research", "workspace":
            // The newest research's chat, or its workspace on `-tab paper|notes|sources|team`.
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            guard let newest = try? await model.listResearches().first else { break }
            model.openChat(newest.sessionId)
            if defaults.string(forKey: "open") == "workspace" {
                path = [.workspace(newest.sessionId, defaults.string(forKey: "tab").flatMap { WorkspaceTab(rawValue: $0.capitalized) })]
            }
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
        if let topic = defaults.string(forKey: "research") {
            while !model.connected { try? await Task.sleep(for: .milliseconds(200)) }
            model.startResearch(topic)
            return
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

#if DEBUG
/// A reply with every tool card for `-open cards`, or only the first half for `-open cardstop`.
private func sampleCards(top: Bool) -> [Message] {
    let calls: [(String, String)] = [
        ("open_maps", #"{"place": {"mode": "search", "query": "Sagrada Familia", "name": "Sagrada Familia"}}"#),
        ("get_location", #"{"latitude": 41.3874, "longitude": 2.1686}"#),
        ("query_contacts", #"{"contacts": [{"name": "Ana Sample", "phones": [{"number": "+34 600 000 000", "type": "mobile"}], "emails": [{"address": "ana@example.com"}]}, {"name": "Ben Sample", "phones": [{"number": "+34 600 000 001"}]}], "count": 2}"#),
        ("open_app", #"{"app": "spotify", "uri": "spotify:", "fallback_url": "https://open.spotify.com"}"#),
        ("open_browser", #"{"url": "https://www.apple.com"}"#),
        ("set_alarm", #"{"hour": 7, "minute": 30, "label": "Gym", "days": ["monday", "wednesday"]}"#),
        ("send_image", #"{"images": [{"url": "https://picsum.photos/id/10/800/600", "caption": "Forest"}, {"url": "https://picsum.photos/id/20/800/600", "caption": "Desk"}]}"#),
        ("show_cards", #"{"cards": [{"kind": "place", "title": "Cafe One", "url": "https://example.com/1", "subtitle": "Coffee", "rating": 4.6, "reviews": 230, "note": "Closest", "place": {"address": "1 Sample St", "latitude": 41.39, "longitude": 2.17, "distance_m": 650, "open_now": true}}, {"kind": "event", "title": "Jazz night", "url": "https://example.com/2", "event": {"start": "2026-11-02T21:00:00+01:00", "venue": "Hall", "tickets_url": "https://example.com/t"}}, {"kind": "media", "title": "A film", "url": "https://example.com/3", "media": {"year": 2024, "creator": "Someone", "watch": "Netflix", "watch_url": "https://netflix.com"}}]}"#),
        ("send_file", #"{"file": {"id": "f1", "name": "report.pdf", "mime": "application/pdf", "size": 48213}}"#),
        ("suggest_replies", #"{"replies": ["Sounds good", "Something else"], "kind": "choice"}"#),
    ]
    var reply = Message(id: "a", role: .assistant, text: "Here is everything. See https://www.rust-lang.org for more.")
    reply.tools = calls.prefix(top ? 6 : calls.count).enumerated().map { ToolCall(id: "c\($0.offset)", name: $0.element.0, done: true, output: $0.element.1) }
    return [Message(id: "u", role: .user, text: "Show me every card"), reply]
}
#endif

private struct JoinToken: Identifiable {
    let id: String
}
