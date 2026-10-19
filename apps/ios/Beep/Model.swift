import Foundation
import Network
import Observation
import UIKit

/// One tool call shown as a chip, or as a card once it finished with something to show.
struct ToolCall: Identifiable, Equatable {
    var id: String
    var name: String
    private(set) var done = false
    private(set) var success = true
    /// The result, cut to 16 KiB by the server; the card is read from it.
    private(set) var output = ""
    private(set) var card: ToolCard?

    var label: String { toolLabel(name) }

    init(id: String, name: String, done: Bool = false, success: Bool = true, output: String = "") {
        self.id = id
        self.name = name
        if done { finish(success: success, output: output) }
    }

    mutating func finish(success: Bool, output: String) {
        done = true
        self.success = success
        self.output = output
        card = ToolCard(name: name, callId: id, success: success, output: output)
    }
}

/// An approval card: a call that waits for the user.
struct ApprovalPrompt: Equatable {
    var queryId: String
    var approval: Approval
    var answer: ApprovalDecision?
}

struct Message: Identifiable, Equatable {
    enum Role { case user, assistant, scheduled }

    var id: String
    var role: Role
    var text: String
    var tools: [ToolCall] = []
    var reasoning = ""
    var approvals: [ApprovalPrompt] = []
    /// Pictures a user message carries.
    var pictures: [Picture] = []
}

/// A message typed while a reply runs, sent as its own turn once the reply finishes.
struct QueuedMessage: Equatable {
    var text: String
    var images: [Data]
}

/// The open chat and the run streaming into it.
struct Chat: Equatable {
    var id = UUID().uuidString.lowercased()
    var title = ""
    var messages: [Message] = []
    var runId = ""
    var cursor: Int64 = 0
    var requestId = ""
    var pendingText = ""
    /// Data URIs of the pictures going out with `pendingText`.
    var pendingImages: [String] = []
    var thinking = ""
    var error = ""
    /// Messages waiting for the reply to finish, sent one by one.
    var queued: [QueuedMessage] = []

    /// A message waits to go out or a run is streaming.
    var busy: Bool { !requestId.isEmpty || !runId.isEmpty }
}

@MainActor
@Observable
final class AppModel {
    private(set) var identity: Data
    private(set) var accountName: String
    var onboarded: Bool {
        didSet { UserDefaults.standard.set(onboarded, forKey: "onboarded") }
    }
    private(set) var connected = false
    /// A connection attempt is under way.
    private(set) var connecting = false
    private(set) var connectionError = ""
    var chat = Chat()
    private(set) var sessions: [Session] = []
    private(set) var groups: [Group] = []
    /// The chat or group id on screen, so its notices are not shown.
    var onScreen: String? {
        didSet { if let onScreen { notifier.clear(onScreen) } }
    }
    /// Where a tapped notification wants to go; the root view follows it and clears it.
    var destination: Destination?
    /// The group chat on screen instead of the one to one chat, if any.
    var openGroup: String?
    /// Set when a new chat should open with the keyboard up; the chat clears it.
    var composeNext = false
    /// A member to @mention when the group chat shows again, from the member sheet.
    var pendingMention: String?
    /// The last line of each chat for the list, cached by the chat's update time.
    private(set) var previews: [String: ChatPreview] = ChatPreview.load()
    /// Saved places, from the server.
    private(set) var places: [SavedPlace] = []
    /// The user's skills and their groups', kept so the list and a skill's page show the same one.
    private(set) var skills: [Skill] = []
    /// The server's places; `-places sample` swaps in made up ones.
    @ObservationIgnored lazy var placesStore: any PlacesStore = ServerPlaces { [unowned self] in try self.api() }
    private(set) var starters: [PromptSuggestion] = []
    private(set) var models: Models?

    private var client: BeepClient?
    /// get_route cards by call id. A plan does not change, so each is fetched once.
    @ObservationIgnored private var routes: [String: RouteCard] = [:]
    private var runTask: Task<Void, Never>?
    /// Which launch `runTask` belongs to, so an older run ending does not clear a newer one.
    private var runToken = UUID()
    /// Telling the server to stop the run the chat let go of.
    private var stopTask: Task<Void, Never>?
    private var connectTask: Task<Void, Never>?
    private let pathMonitor = NWPathMonitor()
    let location = LocationReporter()
    let notifier = ChatNotifier()
    private var previewLoads: Set<String> = []
    private var reportedTimezone: String?
    private let pictureCache = PictureCache()
    let replyImages = ReplyImageCache()
    /// A page open_browser asked for, shown in Safari over the chat.
    var browsing: URL?

    init() {
        let key = Identity.loadOrCreate()
        identity = key
        accountName = Identity.name(key)
        onboarded = UserDefaults.standard.bool(forKey: "onboarded")
        location.onUpdate = { [weak self] fix in self?.sendLocation(fix) }
        location.onCross = { [weak self] id, crossing in self?.crossPlace(id, crossing) }
        notifier.model = self
        pathMonitor.pathUpdateHandler = { [weak self] path in
            guard path.status == .satisfied else { return }
            Task { @MainActor in
                await self?.client?.networkChanged()
                self?.ensureConnected()
            }
        }
        pathMonitor.start(queue: DispatchQueue(label: "beep.path"))
    }

    // MARK: Connection

    /// The app's version as the server logs it, such as "0.1.0 (1)".
    static let appVersion: String = {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(short) (\(build))"
    }()

    /// Connects to the server, retrying with backoff until it answers. It keeps trying for as
    /// long as the app runs; iOS pauses the retries while the app is in the background.
    func connect() {
        connectTask?.cancel()
        connecting = true
        connectTask = Task {
            var attempt = 0
            while !Task.isCancelled {
                attempt += 1
                do {
                    let fresh = try client ?? BeepClient(nodeId: BeepConfig.nodeId, identity: identity, appVersion: Self.appVersion)
                    if client == nil { fresh.setListener(listener: notifier) }
                    client = fresh
                    try await fresh.health()
                    connected = true
                    connecting = false
                    connectionError = ""
                    connectTask = nil
                    await refreshAfterConnect()
                    return
                } catch {
                    guard !Task.isCancelled else { return }
                    connected = false
                    connectionError = describe(error)
                    try? await Task.sleep(for: .milliseconds(min(1000 * attempt, 15000)))
                }
            }
        }
    }

    /// Reconnects when the app comes back or the network returns: at once when it is known to
    /// be down, and after a failed check when it only looks up, since a connection can die
    /// while the app is suspended.
    func ensureConnected() {
        guard connectTask == nil else { return }
        guard connected, let client else { return connect() }
        Task {
            do {
                try await client.health()
            } catch {
                guard connectTask == nil else { return }
                connected = false
                connect()
            }
        }
    }

    private func refreshAfterConnect() async {
        await refreshSessions()
        await refreshGroups()
        await loadStarters(refresh: false)
        models = try? await client?.getModels()
        await reportTimezone()
        location.refresh()
        await refreshPlaces()
        if chat.busy { launchRun() }
    }

    private func api() throws -> BeepClient {
        guard let client else { throw BeepError.Connection(reason: "Not connected yet") }
        return client
    }

    func route(callId: String) async -> RouteCard? {
        if let card = routes[callId] { return card }
        guard let card = try? await api().getRoute(callId: callId) else { return nil }
        routes[callId] = card
        return card
    }

    // MARK: Sessions

    func refreshSessions() async {
        guard let list = try? await api().listSessions() else { return }
        sessions = list
    }

    func newChat() {
        stopStreaming()
        chat = Chat()
    }

    func open(_ session: Session) {
        guard session.id != chat.id else { return }
        stopStreaming()
        chat = Chat(id: session.id, title: session.title)
        Task { await loadHistory() }
    }

    func search(_ query: String) async -> [SessionMatch] {
        (try? await api().searchSessions(query: query)) ?? []
    }

    /// Opens a chat by id, as from a notification.
    func openChat(_ id: String) {
        if let session = sessions.first(where: { $0.id == id }) {
            open(session)
        } else if id != chat.id {
                stopStreaming()
            chat = Chat(id: id)
            Task { await loadHistory() }
        }
    }

    /// The chat's title as the list shows it.
    func title(of id: String) -> String {
        let title = sessions.first { $0.id == id }?.title ?? (id == chat.id ? chat.title : "")
        return title.isEmpty ? "Beep" : title
    }

    func rename(_ session: Session, to title: String) async {
        try? await api().renameSession(id: session.id, title: title)
        await refreshSessions()
    }

    func togglePin(_ session: Session) async {
        try? await api().pinSession(id: session.id, pinned: !session.pinned)
        await refreshSessions()
    }

    func delete(_ session: Session) async {
        try? await api().deleteSession(id: session.id)
        if session.id == chat.id { newChat() }
        await refreshSessions()
    }

    /// Deletes the chats and leaves the groups picked in the drawer.
    func deleteAll(chats: Set<String>, groups ids: Set<String>) async {
        if !chats.isEmpty {
            _ = try? await api().deleteSessions(ids: Array(chats))
            if chats.contains(chat.id) { newChat() }
        }
        if let open = openGroup, ids.contains(open) { openGroup = nil }
        for id in ids { try? await leaveGroup(id) }
        await refreshSessions()
    }

    // MARK: Sharing

    func share(_ id: String) async throws -> SharedLink { try await api().shareSession(id: id) }
    func unshare(_ id: String) async throws { try await api().unshareSession(id: id) }
    func sharedChat(_ token: String) async throws -> SharedSession { try await api().getShare(token: token) }

    /// Copies a shared chat into the user's own chats and opens the copy.
    func continueShared(_ token: String) async throws {
        let session = try await api().copyShare(token: token, sessionId: UUID().uuidString.lowercased())
        await refreshSessions()
        openGroup = nil
        open(session)
    }

    // MARK: Apps

    func apps() async throws -> [AppIntegration] { try await api().listApps() }
    /// Nil on servers from before MCP servers could be added.
    func mcpServers() async -> [McpServer]? { try? await api().listMcpServers() }
    func connectApp(_ slug: String) async throws -> String { try await api().connectApp(slug: slug).url }
    func disconnectApp(_ slug: String) async throws { try await api().disconnectApp(slug: slug) }
    func startAppAuth(_ slug: String, redirect: String) async throws -> String {
        try await api().startAppAuth(slug: slug, redirectUri: redirect).url
    }
    func finishAppAuth(_ slug: String, code: String, state: String, issuer: String?) async throws {
        try await api().finishAppAuth(slug: slug, code: code, state: state, issuer: issuer)
    }
    /// The accounts of an app that connects several, such as Fastmail, the agent's default first.
    func appAccounts(_ slug: String) async throws -> [AppAccount] { try await api().listAppAccounts(slug: slug) }
    /// Like `startAppAuth`, but signs in another account.
    func addAppAccount(_ slug: String, redirect: String) async throws -> String {
        try await api().addAppAccount(slug: slug, redirectUri: redirect).url
    }
    func removeAppAccount(_ slug: String, id: String) async throws { try await api().removeAppAccount(slug: slug, id: id) }
    func appPermissions(_ slug: String) async throws -> AppPermissions { try await api().getAppPermissions(slug: slug) }
    func setAppPermissions(_ slug: String, _ permissions: AppPermissions) async throws {
        try await api().setAppPermissions(slug: slug, level: permissions.level,
                                          allowed: permissions.actions.filter(\.allowed).map(\.tool))
    }
    func addMcpServer(name: String, url: String) async throws -> McpConnect {
        try await api().addMcpServer(name: name, url: url)
    }
    func connectMcpServer(_ id: String) async throws -> McpConnect { try await api().connectMcpServer(id: id) }
    func finishMcpAuth(_ callback: McpCallback) async throws -> McpServer { try await api().finishMcpAuth(callback: callback) }
    func removeMcpServer(_ id: String) async throws { try await api().removeMcpServer(id: id) }

    /// Loads the open chat from the server. Turns where the model only called tools
    /// fold into the next reply with text, as on Android.
    func loadHistory() async {
        let id = chat.id
        guard let entries = try? await api().getSession(id: id), chat.id == id, !chat.busy else { return }
        // An older server has no pictures to give, which just means none show.
        let pictures = picturesByMessage((try? await api().getSessionImages(id: id)) ?? [])
        guard chat.id == id, !chat.busy else { return }
        var pendingTools: [ToolCall] = []
        var pendingReasoning = ""
        var messages: [Message] = []
        for (index, entry) in entries.enumerated() {
            let mid = "history-\(id)-\(index)"
            switch entry {
            case .user(let content, let timestamp, _):
                pendingTools = []
                pendingReasoning = ""
                messages.append(Message(id: mid, role: .user, text: content, pictures: pictures[timestamp] ?? []))
            case .scheduled(let label, _):
                pendingTools = []
                pendingReasoning = ""
                messages.append(Message(id: mid, role: .scheduled, text: label))
            case .assistant(let content, _, let tools, _, let reasoning, _):
                let calls = pendingTools + tools.map { ToolCall(id: $0.callId, name: $0.name, done: true, success: $0.success, output: $0.output) }
                let thought = [pendingReasoning, reasoning].filter { !$0.isEmpty }.joined(separator: "\n\n")
                if content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    pendingTools = calls
                    pendingReasoning = thought
                } else {
                    pendingTools = []
                    pendingReasoning = ""
                    messages.append(Message(id: mid, role: .assistant, text: content, tools: calls, reasoning: thought))
                }
            case .approval(let queryId, let approval):
                messages.append(Message(
                    id: mid, role: .assistant, text: "", tools: pendingTools, reasoning: pendingReasoning,
                    approvals: [ApprovalPrompt(queryId: queryId, approval: approval)]))
                pendingTools = []
                pendingReasoning = ""
            }
        }
        chat.messages = keepingLocalPictures(messages)
        if let session = sessions.first(where: { $0.id == id }) { remember(preview(entries), for: session) }
    }

    /// Carries pictures sent from this phone over to the reloaded history, matching user
    /// messages by text in order, where the server gave none for that message.
    private func keepingLocalPictures(_ fresh: [Message]) -> [Message] {
        var local = chat.messages.filter { $0.role == .user && $0.pictures.contains { $0.dataUri != nil } }
        guard !local.isEmpty else { return fresh }
        var out = fresh
        for index in out.indices where out[index].role == .user {
            guard let match = local.firstIndex(where: { $0.text == out[index].text }) else { continue }
            if out[index].pictures.isEmpty { out[index].pictures = local[match].pictures }
            local.remove(at: match)
        }
        return out
    }

    /// The cached file for a picture kept on the server, fetched when missing.
    func pictureFile(_ id: String) async -> URL? {
        await pictureCache.file(id, client: client)
    }

    // MARK: Previews

    /// Fetches a chat's last line for the list when the cached one is older than the chat.
    func loadPreview(_ session: Session) async {
        if let cached = previews[session.id], cached.updatedAt >= session.updatedAt { return }
        guard !previewLoads.contains(session.id) else { return }
        previewLoads.insert(session.id)
        defer { previewLoads.remove(session.id) }
        guard let entries = try? await api().getSession(id: session.id) else { return }
        remember(preview(entries), for: session)
    }

    private func remember(_ text: String, for session: Session) {
        previews[session.id] = ChatPreview(text: text, updatedAt: session.updatedAt)
        ChatPreview.save(previews)
    }

    private func preview(_ entries: [HistoryEntry]) -> String {
        for entry in entries.reversed() {
            switch entry {
            case .user(let content, _, _): return "You: " + plainLine(content)
            case .assistant(let content, _, _, _, _, _):
                let line = plainLine(content)
                if !line.isEmpty { return line }
            case .scheduled, .approval: continue
            }
        }
        return ""
    }

    // MARK: Groups

    func refreshGroups() async {
        guard let list = try? await api().listGroups() else { return }
        groups = list
    }

    func group(_ id: String) async throws -> GroupChat { try await api().getGroup(id: id) }

    /// A group's pictures by message seq; empty when the server has none or is too old.
    func groupPictures(_ id: String) async -> [Int64: [Picture]] {
        picturesByMessage((try? await api().getGroupImages(id: id)) ?? [])
    }

    func createGroup(name: String, memberName: String) async throws -> Group {
        let group = try await api().createGroup(id: UUID().uuidString.lowercased(), name: name, memberName: memberName)
        await refreshGroups()
        return group
    }

    /// Posts to a group; the same `requestId` sent twice posts once.
    func sendToGroup(_ id: String, requestId: String, text: String) async throws {
        _ = try await api().sendToGroup(groupId: id, requestId: requestId, message: text, images: [])
        await refreshGroups()
    }

    func groupInvite(_ id: String, reset: Bool = false) async throws -> GroupLink {
        try await api().groupInvite(id: id, reset: reset)
    }

    func peekGroup(_ token: String) async throws -> GroupPreview { try await api().peekGroup(token: token) }

    func joinGroup(_ token: String, memberName: String) async throws -> Group {
        let group = try await api().joinGroup(token: token, memberName: memberName)
        await refreshGroups()
        return group
    }

    func leaveGroup(_ id: String) async throws {
        try await api().leaveGroup(id: id)
        groups.removeAll { $0.id == id }
    }

    /// Pins at once; the list is fetched again if the server refuses.
    func pinGroup(_ id: String, pinned: Bool) async {
        if let index = groups.firstIndex(where: { $0.id == id }) { groups[index].pinned = pinned }
        if (try? await api().pinGroup(id: id, pinned: pinned)) == nil { await refreshGroups() }
    }

    func shareGroupApps(_ id: String, shared: Bool) async {
        if let index = groups.firstIndex(where: { $0.id == id }) { groups[index].appsShared = shared }
        if (try? await api().shareGroupApps(id: id, shared: shared)) == nil { await refreshGroups() }
    }

    func groupReminders(_ id: String) async throws -> [GroupReminder] {
        try await api().listGroupReminders(groupId: id)
    }

    func deleteGroupReminder(_ groupId: String, _ id: String) async throws {
        try await api().deleteGroupReminder(groupId: groupId, id: id)
    }

    // MARK: Starters

    func loadStarters(refresh: Bool) async {
        guard let fresh = try? await api().suggestPrompts(refresh: refresh) else { return }
        starters = fresh.suggestions
    }

    // MARK: Runs

    /// Sends a message, with up to eight pictures as JPEGs.
    func send(_ text: String, images: [Data] = []) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !images.isEmpty, !chat.busy else { return }
        if chat.messages.isEmpty { chat.title = String((trimmed.isEmpty ? "Photo" : trimmed).prefix(44)) }
        chat.messages.append(Message(id: UUID().uuidString, role: .user, text: trimmed,
                                     pictures: images.map { Picture(dataUri: dataURI($0)) }))
        chat.requestId = UUID().uuidString.lowercased()
        chat.pendingText = trimmed
        chat.pendingImages = images.map(dataURI)
        chat.error = ""
        launchRun()
    }

    /// Holds a message until the running reply finishes.
    func queue(_ text: String, images: [Data] = []) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !images.isEmpty else { return }
        chat.queued.append(QueuedMessage(text: trimmed, images: Array(images.prefix(ChatPhotos.limit))))
    }

    /// Drops the queued message at `index` and returns it, so it can go back into the composer.
    func unqueue(_ index: Int) -> QueuedMessage? {
        guard chat.queued.indices.contains(index) else { return nil }
        return chat.queued.remove(at: index)
    }

    /// Sends the first queued message now; the rest follow it.
    func sendQueued() {
        guard !chat.busy, let next = unqueue(0) else { return }
        send(next.text, images: next.images)
    }

    /// Sends the last prompt again after a run that did not finish.
    func retry() {
        guard !chat.busy, let last = chat.messages.last(where: { $0.role == .user }) else { return }
        chat.requestId = UUID().uuidString.lowercased()
        chat.pendingText = last.text
        chat.pendingImages = last.pictures.compactMap(\.dataUri)
        chat.error = ""
        launchRun()
    }

    /// Stops the run at once: the chat lets go of it and is free for the next
    /// message while the server is told to stop it.
    func stop() {
        guard chat.busy else { return }
        stopStreaming()
        let stopped = chat
        stopTask = Task { try? await stopRun(stopped) }
        chat.runId = ""
        chat.requestId = ""
        chat.pendingText = ""
        chat.pendingImages = []
        chat.thinking = ""
        chat.error = runEndError("cancelled", current: chat.error)
        finished(chat)
    }

    /// Tells the server to stop `chat`'s run, learning its id first if the message only just went out.
    private func stopRun(_ chat: Chat) async throws {
        let api = try api()
        var runId = chat.runId
        if runId.isEmpty {
            guard !chat.requestId.isEmpty else { return }
            // The same request finds the run it started, or starts it to be stopped.
            let stream = try await api.chat(
                sessionId: chat.id, requestId: chat.requestId, message: chat.pendingText,
                images: chat.pendingImages, rerun: nil)
            while runId.isEmpty, let received = try await stream.next() {
                if case .runStarted(let started, _) = received.event { runId = started }
            }
            guard !runId.isEmpty else { return }
        }
        try await api.cancelRun(id: runId)
    }

    func answer(_ prompt: ApprovalPrompt, _ decision: ApprovalDecision) {
        guard let index = chat.messages.firstIndex(where: { $0.approvals.contains { $0.queryId == prompt.queryId } }),
            let card = chat.messages[index].approvals.firstIndex(where: { $0.queryId == prompt.queryId }),
            chat.messages[index].approvals[card].answer == nil
        else { return }
        chat.messages[index].approvals[card].answer = decision
        let sessionId = chat.id
        Task {
            do {
                let run = try await api().answerApproval(sessionId: sessionId, queryId: prompt.queryId, decision: decision)
                guard chat.id == sessionId else { return }
                // The card's reply carries on in the new run.
                if let holder = chat.messages.lastIndex(where: { $0.approvals.contains { $0.queryId == prompt.queryId } }) {
                    chat.messages[holder].id = "assistant-\(run.id)"
                }
                chat.runId = run.id
                chat.cursor = 0
                chat.error = ""
                launchRun()
            } catch {
                chat.error = describe(error)
            }
        }
    }

    private func stopStreaming() {
        runTask?.cancel()
        runTask = nil
    }

    /// Streams the open chat's run, resuming from the cursor after a dropped stream.
    private func launchRun() {
        guard runTask == nil else { return }
        let chatId = chat.id
        let token = UUID()
        runToken = token
        let stopping = stopTask
        stopTask = nil
        runTask = Task {
            defer { if runToken == token { runTask = nil } }
            // The server takes the chat's next message once it has the stop.
            await stopping?.value
            var failures = 0
            while !Task.isCancelled, chat.id == chatId, chat.busy {
                do {
                    let api = try api()
                    let stream: EventStream
                    if chat.runId.isEmpty {
                        await reportTimezone()
                        stream = try await api.chat(
                            sessionId: chatId, requestId: chat.requestId, message: chat.pendingText,
                            images: chat.pendingImages, rerun: nil)
                    } else {
                        stream = try await api.events(runId: chat.runId, after: chat.cursor)
                    }
                    var first = true
                    while let received = try await stream.next() {
                        guard !Task.isCancelled, chat.id == chatId else { return }
                        if case .runFinished(let status, _) = received.event, first, chat.runId.isEmpty {
                            // A resent message whose run already finished: its reply is in the history.
                            chat.requestId = ""
                            chat.pendingText = ""
                            chat.pendingImages = []
                            chat.error = runEndError(status)
                            await loadHistory()
                            break
                        }
                        first = false
                        try handle(received)
                    }
                    failures = 0
                    connected = true
                    try await Task.sleep(for: .milliseconds(150))
                } catch is CancellationError {
                    return
                } catch {
                    failures += 1
                    connected = false
                    chat.error = failures < 4 ? "Reconnecting…" : describe(error)
                    if failures >= 4 {
                        connect()
                        return
                    }
                    try? await Task.sleep(for: .milliseconds(1000 << (failures - 1)))
                }
            }
        }
    }

    private struct MissedEvent: Error {}

    private func handle(_ received: Event) throws {
        if case .runStarted(let runId, _) = received.event {
            chat.runId = runId
            chat.cursor = 0
            chat.requestId = ""
            chat.pendingText = ""
            chat.pendingImages = []
        }
        guard received.seq > chat.cursor else { return }
        guard received.seq == chat.cursor + 1 else { throw MissedEvent() }
        chat.cursor = received.seq
        let mid = "assistant-\(chat.runId)"

        switch received.event {
        case .textDelta(let delta):
            updateReply(mid) { $0.text += delta }
        case .reasoningDelta(let delta):
            chat.thinking += delta
        case .toolStarted(let name, let callId, _):
            updateReply(mid) { reply in
                if callId.isEmpty || !reply.tools.contains(where: { $0.id == callId }) {
                    reply.tools.append(ToolCall(id: callId.isEmpty ? UUID().uuidString : callId, name: name))
                }
            }
        case .toolResult(_, let callId, let output, let success):
            updateReply(mid) { reply in
                guard let index = reply.tools.firstIndex(where: { callId.isEmpty ? !$0.done : $0.id == callId }) else { return }
                reply.tools[index].finish(success: success, output: output)
                if success { act(reply.tools[index]) }
            }
        case .deviceQuery(let queryId, let query):
            if case .approve(let approval) = query {
                updateReply(mid) { $0.approvals.append(ApprovalPrompt(queryId: queryId, approval: approval)) }
            } else {
                answerDeviceQuery(runId: chat.runId, queryId: queryId, query: query)
            }
        case .error(let message):
            chat.error = message.isEmpty ? "The assistant couldn't finish." : message
        case .runFinished(let status, _):
            chat.runId = ""
            chat.thinking = ""
            chat.error = runEndError(status, current: chat.error)
            finished(chat)
            // Queued messages wait out a run that stopped or failed, so the user can decide.
            if chat.error.isEmpty { sendQueued() }
            Task { await refreshSessions() }
        case .runStarted, .usage, .memoryScore, .autoPicked:
            break
        }
    }

    /// Changes the run's reply, adding it first if this is its first event.
    private func updateReply(_ id: String, _ change: (inout Message) -> Void) {
        if let index = chat.messages.firstIndex(where: { $0.id == id }) {
            change(&chat.messages[index])
            absorbThinking(index)
        } else {
            var reply = Message(id: id, role: .assistant, text: "")
            change(&reply)
            chat.messages.append(reply)
            absorbThinking(chat.messages.count - 1)
        }
    }

    private func absorbThinking(_ index: Int) {
        let thought = chat.thinking.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !thought.isEmpty else { return }
        let before = chat.messages[index].reasoning
        chat.messages[index].reasoning = before.isEmpty ? thought : before + "\n\n" + thought
        chat.thinking = ""
    }

    /// Questions about the phone. The browser is not wired up on iPhone yet.
    private func answerDeviceQuery(runId: String, queryId: String, query: DeviceQuery) {
        Task {
            let answer: DeviceAnswer
            switch query {
            case .contacts(let name): answer = .ok(output: await PhoneContacts.search(name))
            case .browser, .approve: answer = .err(message: "The iPhone app cannot do that yet.")
            }
            _ = try? await api().deviceResult(runId: runId, queryId: queryId, answer: answer)
        }
    }

    /// Does what a phone tool asked as its result streams in, as Android does: opens the app or
    /// page, or sets the alarm. Places only show as a card; Maps opens when the user taps it.
    private func act(_ call: ToolCall) {
        guard UIApplication.shared.applicationState == .active else { return }
        switch call.card {
        case .openedApp(let app): PhoneApps.open(app: app.app, uri: app.uri, fallback: app.fallback)
        case .link(let url): browsing = url
        case .alarm(let alarm): Task { await PhoneAlarms.set(alarm) }
        default: break
        }
    }

    /// A file Beep sent, downloaded once into the cache under its own name.
    func sentFile(id: String, name: String) async throws -> URL {
        let url = SentFiles.local(id: id, name: name)
        if FileManager.default.fileExists(atPath: url.path) { return url }
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        _ = try await api().saveFile(id: id, path: url.path)
        return url
    }

    /// Keeps the list's preview current, and when the app is in the background
    /// shows the reply as a notification, since only routines get notices.
    private func finished(_ chat: Chat) {
        guard let reply = chat.messages.last(where: { $0.role == .assistant && !$0.text.isEmpty }) else { return }
        if let session = sessions.first(where: { $0.id == chat.id }) {
            previews[chat.id] = ChatPreview(text: plainLine(reply.text), updatedAt: max(session.updatedAt, nowMillis()))
            ChatPreview.save(previews)
        }
        if UIApplication.shared.applicationState != .active {
            notifier.show(chatId: chat.id, title: title(of: chat.id), lines: [.init(sender: nil, text: plainLine(reply.text))])
        }
    }

    /// Answers a chat from its notification. The app may be in the background, so
    /// this runs the turn itself and shows the reply as the next notification.
    /// For the open chat it waits out a reply still streaming, sends, and returns once
    /// the new run ends, so the caller's background time covers the whole turn.
    func reply(toChat id: String, text: String) async {
        if id == chat.id {
            await waitForRun(in: id)
            guard chat.id == id else { return await reply(toChat: id, text: text) }
            guard !chat.busy else { return }
            send(text)
            await waitForRun(in: id)
            return
        }
        guard let api = try? api(),
            let stream = try? await api.chat(
                sessionId: id, requestId: UUID().uuidString.lowercased(), message: text, images: [], rerun: nil)
        else { return }
        var reply = ""
        while let received = try? await stream.next() {
            if case .textDelta(let delta) = received.event { reply += delta }
            if case .runFinished = received.event { break }
        }
        await refreshSessions()
        let line = plainLine(reply)
        guard !line.isEmpty else { return }
        if let session = sessions.first(where: { $0.id == id }) { remember(line, for: session) }
        notifier.show(chatId: id, title: title(of: id), lines: [.init(sender: "You", text: text), .init(sender: nil, text: line)])
    }

    /// Waits while the chat is open and its run streams, or until background time runs low.
    private func waitForRun(in id: String) async {
        while chat.id == id, chat.busy, runTask != nil, UIApplication.shared.backgroundTimeRemaining > 5 {
            try? await Task.sleep(for: .milliseconds(250))
        }
    }

    /// A tool call allowed or denied from its notification.
    func answerFromNotice(sessionId: String, queryId: String, group: Bool, allow: Bool) async {
        if group {
            try? await api().answerGroupApproval(queryId: queryId, allow: allow)
        } else if sessionId == chat.id, let prompt = chat.messages.flatMap(\.approvals).first(where: { $0.queryId == queryId }) {
            answer(prompt, allow ? .allow : .deny)
        } else {
            _ = try? await api().answerApproval(sessionId: sessionId, queryId: queryId, decision: allow ? .allow : .deny)
        }
    }

    /// Something changed on the server that a notice says nothing more about.
    func noticeChanged(_ notice: Notice) {
        Task {
            if notice.group { await refreshGroups() } else { await refreshSessions() }
            if !notice.group, notice.sessionId == chat.id, !chat.busy { await loadHistory() }
            if !notice.group, notice.sessionId.isEmpty { await refreshPlaces() }
        }
    }

    // MARK: Places

    func refreshPlaces() async {
        guard let list = try? await placesStore.list() else { return }
        places = list
        location.watch(list)
    }

    func deletePlace(_ id: String) async throws {
        try await placesStore.delete(id: id)
        places.removeAll { $0.id == id }
        location.watch(places)
    }

    func updatePlace(_ id: String, name: String?, radius: Int?) async throws {
        let place = try await placesStore.update(id: id, name: name, radius: radius)
        if let index = places.firstIndex(where: { $0.id == id }) { places[index] = place } else { places.append(place) }
        location.watch(places)
    }

    /// Tells the server the phone crossed a saved place's edge. iOS may have just
    /// launched the app in the background for this, so it connects first and asks
    /// for time to finish.
    private func crossPlace(_ id: String, _ crossing: PlaceCrossing) {
        let background = UIApplication.shared.beginBackgroundTask(withName: "place crossing")
        Task {
            defer { UIApplication.shared.endBackgroundTask(background) }
            if !connected { connect() }
            let deadline = Date.now.addingTimeInterval(20)
            while !connected, Date.now < deadline { try? await Task.sleep(for: .milliseconds(250)) }
            if (try? await api().crossPlace(id: id, crossing: crossing)) == true { await refreshPlaces() }
        }
    }

    // MARK: Account

    /// Signs in as the account in a key file, after checking the server accepts it.
    func importKey(_ text: String) async throws {
        guard let key = Identity.parse(text) else { throw BeepError.BadRequest(reason: "This file has no key") }
        if key == identity { return }
        let trial = try BeepClient(nodeId: BeepConfig.nodeId, identity: key, appVersion: Self.appVersion)
        trial.setListener(listener: notifier)
        try await trial.health()
        Identity.save(key)
        identity = key
        accountName = Identity.name(key)
        client = trial
        newChat()
        sessions = []
        groups = []
        starters = []
        previews = [:]
        ChatPreview.save(previews)
        // The new account has not been told the timezone, and the old one's places
        // must not keep their region monitors if the new list fails to load.
        reportedTimezone = nil
        places = []
        location.watch([])
        await refreshAfterConnect()
    }

    // MARK: Devices

    func peekInvite(_ code: String) async throws -> JoinPreview { try await api().peekInvite(code: code) }

    func linkDevice(_ offerUri: String) async throws -> LinkedDevice { try await api().linkDevice(offerUri: offerUri) }

    /// A code a new phone or computer joins this account with.
    func inviteDevice() async throws -> DeviceInvite { try await api().inviteDevice(name: UIDevice.current.name) }

    /// Moves this phone into the account that showed `code`, then loads that account's chats.
    func joinAccount(_ code: String) async throws {
        try await api().joinAccount(code: code)
        // The server closes the connection; the next call signs in again.
        try? await Task.sleep(for: .milliseconds(1500))
        newChat()
        await refreshAfterConnect()
    }

    // MARK: Settings

    func setModel(_ id: String) async {
        if let fresh = try? await api().setModel(id: id) { models = fresh }
    }

    func setThinking(_ level: ReasoningLevel) async {
        if let fresh = try? await api().setThinking(level: level) { models = fresh }
    }

    func setAuto(_ on: Bool) async {
        if let fresh = try? await api().setAutoModel(enabled: on) { models = fresh }
    }

    /// Whether an admin turned agents on for everyone; false for anyone else.
    func agentsOn() async -> Bool { (try? await api().getAgents().enabled) ?? false }

    func memories() async throws -> [Memory] { try await api().listMemories() }
    /// Empty on servers from before reads were listed.
    func memoryReads() async -> [MemoryRead] { (try? await api().listMemoryReads()) ?? [] }
    func updateMemory(_ id: String, _ content: String) async throws { try await api().updateMemory(id: id, content: content) }
    func deleteMemory(_ id: String) async throws { try await api().deleteMemory(id: id) }
    func clearMemories() async throws { _ = try await api().clearMemories() }

    func refreshSkills() async throws { skills = try await api().listSkills() }
    /// Changes only the fields given; a name another skill has comes back as `Conflict`.
    func updateSkill(_ id: String, name: String? = nil, description: String? = nil, body: String? = nil,
                     enabled: Bool? = nil) async throws {
        let skill = try await api().updateSkill(id: id, name: name, description: description, body: body, enabled: enabled)
        replaceSkill(skill)
    }
    func undoSkill(_ id: String) async throws { replaceSkill(try await api().undoSkill(id: id)) }
    func deleteSkill(_ id: String) async throws {
        try await api().deleteSkill(id: id)
        skills.removeAll { $0.id == id }
    }

    private func replaceSkill(_ skill: Skill) {
        if let at = skills.firstIndex(where: { $0.id == skill.id }) { skills[at] = skill } else { skills.append(skill) }
    }

    func crons() async throws -> [Cron] { try await api().listCrons() }
    func createCron(label: String, prompt: String, schedule: String) async throws {
        _ = try await api().createCron(label: label, prompt: prompt, schedule: schedule)
    }
    func setCron(_ id: String, enabled: Bool) async throws {
        try await api().updateCron(id: id, enabled: enabled, label: nil, schedule: nil)
    }
    func deleteCron(_ id: String) async throws { try await api().deleteCron(id: id) }

    /// Runs a routine now and opens the chat it ran in.
    func runCron(_ id: String) async throws {
        let sessionId = try await api().runCron(id: id)
        await refreshSessions()
        stopStreaming()
        chat = Chat(id: sessionId, title: sessions.first { $0.id == sessionId }?.title ?? "")
        await loadHistory()
    }

    /// Tells the server the phone's timezone when it differs from the last
    /// one sent, so routines run in local time without a location.
    private func reportTimezone() async {
        let tz = TimeZone.current.identifier
        guard tz != reportedTimezone else { return }
        if (try? await api().setTimezone(timezone: tz)) != nil { reportedTimezone = tz }
    }

    private func sendLocation(_ fix: LocationReporter.Fix) {
        Task {
            try? await api().updateLocation(
                latitude: fix.latitude, longitude: fix.longitude, accuracy: fix.accuracy,
                timezone: TimeZone.current.identifier, place: fix.place)
        }
    }
}

/// The chat list's last line of a chat, with the time the chat had then.
struct ChatPreview: Codable {
    var text: String
    var updatedAt: Int64

    static func load() -> [String: ChatPreview] {
        guard let data = UserDefaults.standard.data(forKey: "chat_previews") else { return [:] }
        return (try? JSONDecoder().decode([String: ChatPreview].self, from: data)) ?? [:]
    }

    static func save(_ previews: [String: ChatPreview]) {
        UserDefaults.standard.set(try? JSONEncoder().encode(previews), forKey: "chat_previews")
    }
}

/// A screen the app opens from outside, such as a notification.
enum Destination: Equatable {
    case chat(String)
    case group(String)
    /// The new routine sheet, set to arriving at this place.
    case placeRoutine(String)
    /// Memories, showing what the newest read of mail or a calendar found.
    case latestRead
    /// A new chat with the keyboard up, from the widget.
    case newChat
    /// A chat someone shared, from its link.
    case shared(String)
}

func nowMillis() -> Int64 { Int64(Date.now.timeIntervalSince1970 * 1000) }

/// A reply as one line: markdown marks, code and product blocks taken out.
func plainLine(_ text: String) -> String {
    var out: [String] = []
    var fenced = false
    for raw in text.components(separatedBy: "\n") {
        let line = raw.trimmingCharacters(in: .whitespaces)
        if line.hasPrefix("```") {
            fenced.toggle()
            continue
        }
        if fenced || line.isEmpty || line.hasPrefix("|") { continue }
        var clean = line
        while let mark = clean.first, "#>-*".contains(mark) { clean = clean.dropFirst().trimmingCharacters(in: .whitespaces) }
        clean = clean.replacingOccurrences(of: "**", with: "").replacingOccurrences(of: "`", with: "")
        if !clean.isEmpty { out.append(clean) }
    }
    return out.joined(separator: " ")
}

/// A picture for `chat`, as the data URI the server takes.
func dataURI(_ jpeg: Data) -> String { "data:image/jpeg;base64," + jpeg.base64EncodedString() }

func runEndError(_ status: String, current: String = "") -> String {
    switch status {
    case "completed", "waiting": ""
    case "cancelled": "Stopped"
    case "interrupted": "The server restarted before Beep finished."
    case "timed_out": "That took too long, so Beep stopped. Try again."
    case "stalled": "The model stopped responding. Try again."
    case "looped": "Beep got stuck repeating itself and was stopped. Try again."
    default: current.isEmpty ? "Run ended: \(status). You can send another message." : current
    }
}

func describe(_ error: Error) -> String {
    switch error {
    case BeepError.BadRequest(let reason), BeepError.Conflict(let reason): reason
    case BeepError.Connection: "Can't reach the server."
    case BeepError.Unauthorized: "The server did not accept this account."
    case BeepError.NotFound: "Not found."
    case BeepError.Internal: "Something went wrong on the server."
    default: error.localizedDescription
    }
}
