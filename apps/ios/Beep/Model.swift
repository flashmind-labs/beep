import Foundation
import Network
import Observation
import UIKit

/// One tool call shown as a chip, or as a card once it finished with something to show.
struct ToolCall: Identifiable, Equatable {
    var id: String
    var name: String
    /// The server's words for the call, such as "Recalling memories". From the history only
    /// a server that kept them gives them.
    var description = ""
    private(set) var done = false
    private(set) var success = true
    /// The result, cut to 16 KiB by the server; the card is read from it.
    private(set) var output = ""
    private(set) var card: ToolCard?

    var label: String { toolLabel(name, description: description, output: output) }

    init(id: String, name: String, description: String = "", done: Bool = false, success: Bool = true, output: String = "") {
        self.id = id
        self.name = name
        self.description = description
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
    /// Asked by a helper agent, a background job or a coding agent on a machine rather than
    /// by a turn: no run resumes when it is answered, and it arrives only as a notice.
    var background = false
    /// The background work stopped waiting before the answer reached it.
    var withdrawn = false
    /// A turn's question left unanswered because the user wrote again, as on Android.
    var skipped = false

    /// Still waiting on the user, so a turn's question holds the chat.
    var waiting: Bool { answer == nil && !withdrawn && !skipped }
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
    /// Other files a user message carries, such as a PDF.
    var files: [SentFile] = []
    /// The model an admin reran this prompt on, as a `ModelChoice` id, or the one that wrote
    /// a reply from the history. Empty for the usual model.
    var model = ""
    /// The thinking an admin picked for that rerun; nil keeps their usual one.
    var modelThinking: ReasoningLevel?
    /// When the server saved this prompt, in epoch milliseconds; 0 until the chat reloads.
    /// Names the turn to delete.
    var sentAt: Int64 = 0
    /// Jev's estimate that a reply's turn held something worth remembering.
    var memoryScore: Double?
    /// The specialized agent a reply ran as; empty for General.
    var agent = ""
    /// The model and thinking Jev picked for this prompt under Auto; nil when it picked none.
    var auto: AutoPick?
    /// When the server saved a reply from the history, in epoch milliseconds; 0 otherwise.
    var repliedAt: Int64 = 0
    /// The web pages a reply's turn used, shown as chips under its last reply.
    var sources: [WebSource] = []
}

/// A message typed while a reply runs, sent as its own turn once the reply finishes.
struct QueuedMessage: Equatable {
    var text: String
    var images: [Data]
    var files: [SentFile] = []
    /// Said in live mode, so it goes out as spoken and its reply is read out.
    var spoken = false
    /// The live session's number for that reply, passed back with its run's events.
    var reply: UInt64 = 0
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
    /// Files going out with `pendingText`.
    var pendingFiles: [SentFile] = []
    /// The model and thinking an admin picked to rerun `pendingText` on.
    var pendingModel = ""
    var pendingThinking: ReasoningLevel?
    /// Whether `pendingText` starts a research rather than asking Beep.
    var pendingResearch = false
    /// Whether `pendingText` was said in live mode.
    var pendingSpoken = false
    /// The live session's number for the reply to `pendingText`, when it was said.
    var pendingReply: UInt64 = 0
    var thinking = ""
    var error = ""
    /// Messages waiting for the reply to finish, sent one by one.
    var queued: [QueuedMessage] = []
    /// The chat's `updatedAt` when the history on screen was fetched, 0 before any was.
    var historyAt: Int64 = 0

    /// A message waits to go out or a run is streaming.
    var busy: Bool { !requestId.isEmpty || !runId.isEmpty }

    /// The admin's pick for `pendingText`, as the server takes it.
    var pendingRerun: Rerun? { pendingModel.isEmpty ? nil : Rerun(model: pendingModel, thinking: pendingThinking) }

    /// Nothing waits to go out any more.
    mutating func clearPending() {
        requestId = ""
        pendingText = ""
        pendingImages = []
        pendingFiles = []
        pendingModel = ""
        pendingThinking = nil
        pendingResearch = false
        pendingSpoken = false
        pendingReply = 0
    }
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
    /// Questions from background work by chat id, shown at the end of their chat. The server's
    /// list replaces a chat's on opening it and on reconnecting; the saved copy shows until then.
    /// Answered ones go with the next history reload.
    private(set) var waitingWork: [String: [ApprovalPrompt]] = ChatStore.loadWork() {
        didSet { ChatStore.saveWork(waitingWork) }
    }
    /// Where a tapped notification wants to go; the root view follows it and clears it.
    var destination: Destination?
    /// The group chat on screen instead of the one to one chat, if any.
    var openGroup: String?
    /// Set when a new chat should open with the keyboard up; the chat clears it.
    var composeNext = false
    /// A member to @mention when the group chat shows again, from the member sheet.
    var pendingMention: String?
    /// Text quoted from a research's workspace, for the chat's composer to take when it shows.
    var pendingQuote: String?
    /// What was said in live voice and not sent, for the chat's composer to take when it shows.
    var pendingDraft: String?
    /// The chats that are researches: every one the server listed, and those started here since.
    private(set) var researches: Set<String> = Set(UserDefaults.standard.stringArray(forKey: "researches") ?? [])
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
    /// Live voice: the session, its screen's state and the chat it talks in.
    @ObservationIgnored lazy var live = LiveVoice(model: self)
    /// The run answering the last spoken message, whose events live mode reads out.
    @ObservationIgnored private var liveRunId = ""
    /// The live session's number for that reply, which every call about its run carries.
    @ObservationIgnored private var liveReply: UInt64 = 0
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
    /// The in-app web view the browser screen shows, for links the user opens.
    let browser = PageBrowser()
    /// Whether the browser screen is up over the chat.
    var browserOpen = false
    /// The APNs token iOS gave this install, in hex. The server gets it on every connect.
    var pushToken: String? {
        didSet { Task { await registerPush() } }
    }

    init() {
        let key = Identity.loadOrCreate()
        identity = key
        accountName = Identity.name(key)
        onboarded = UserDefaults.standard.bool(forKey: "onboarded")
        let list = ChatStore.loadList()
        sessions = list.sessions
        groups = list.groups
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
        if !chat.busy { await refreshOpenChat() }
        await refreshQuestions(chat.id)
        await loadStarters(refresh: false)
        models = try? await client?.getModels()
        await reportTimezone()
        await registerPush()
        location.refresh()
        await refreshPlaces()
        if chat.busy { launchRun() }
    }

    // MARK: Push

    /// Names this install's push token to the server, so a notice it cannot send over the
    /// connection goes through APNs.
    private func registerPush() async {
        guard connected, let client, let pushToken else { return }
        try? await client.registerApns(deviceId: PushRegistration.deviceId, token: pushToken,
                                       environment: PushRegistration.environment)
    }

    /// A background push: the server has notices it could not send. Connecting makes it send
    /// them, and the notifier shows each as it comes.
    func wake() async {
        ensureConnected()
        let deadline = Date.now.addingTimeInterval(20)
        while !connected, Date.now < deadline {
            try? await Task.sleep(for: .milliseconds(300))
        }
        // The server sends the queue as soon as the connection opens.
        try? await Task.sleep(for: .seconds(3))
    }

    private func api() throws -> BeepClient {
        guard let client else { throw BeepError.Connection(reason: "Not connected yet") }
        return client
    }

    /// The client once the server has answered on it, reconnecting once when it does not.
    /// For a request that must not be sent twice, so a dropped connection fails here
    /// rather than after the request went out.
    private func liveApi() async throws -> BeepClient {
        let api = try api()
        do {
            try await api.health()
        } catch BeepError.Connection {
            await api.networkChanged()
            try await api.health()
        }
        return api
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
        ChatStore.saveList(sessions: sessions, groups: groups)
        ChatStore.prune(keep: Set(list.map(\.id)))
    }

    func newChat() {
        stopStreaming()
        chat = Chat()
    }

    /// When the open chat last had a turn: now while a reply runs, otherwise the newest of
    /// the chat list's time, the preview's and its last messages'. Nil when none is known.
    var openChatActivity: Date? {
        if chat.busy { return .now }
        let times: [Int64] = [sessions.first { $0.id == chat.id }?.updatedAt ?? 0, previews[chat.id]?.updatedAt ?? 0]
            + chat.messages.suffix(2).map { max($0.sentAt, $0.repliedAt) }
        guard let newest = times.max(), newest > 0 else { return nil }
        return Date(timeIntervalSince1970: TimeInterval(newest) / 1000)
    }

    func open(_ session: Session) {
        guard session.id != chat.id else { return }
        stopStreaming()
        chat = Chat(id: session.id, title: session.title)
        if let cached = ChatStore.loadHistory(session.id) {
            show(cached.entries, images: cached.images, files: cached.files,
                 sources: ChatStore.loadSources(session.id), at: cached.updatedAt)
        }
        Task {
            await refreshOpenChat()
            await refreshQuestions(session.id)
        }
    }

    /// Fetches the open chat when the server has turns the copy on screen lacks.
    private func refreshOpenChat() async {
        guard let session = sessions.first(where: { $0.id == chat.id }) else { return }
        guard chat.historyAt == 0 || session.updatedAt == 0 || chat.historyAt < session.updatedAt else { return }
        await loadHistory()
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
            Task {
                await loadHistory()
                await refreshQuestions(id)
            }
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
        try await liveApi().addMcpServer(name: name, url: url)
    }
    func connectMcpServer(_ id: String) async throws -> McpConnect { try await liveApi().connectMcpServer(id: id) }
    func finishMcpAuth(_ callback: McpCallback) async throws -> McpServer { try await api().finishMcpAuth(callback: callback) }
    func removeMcpServer(_ id: String) async throws { try await api().removeMcpServer(id: id) }
    func mcpTools(_ id: String) async throws -> McpTools { try await api().listMcpTools(id: id) }
    func setMcpTool(_ id: String, tool: String, enabled: Bool) async throws -> McpTools {
        try await api().setMcpTool(id: id, tool: tool, enabled: enabled)
    }

    /// Loads the open chat from the server and saves it for the next time it opens.
    func loadHistory() async {
        let id = chat.id
        // Taken before fetching, so the saved copy is never marked newer than it is.
        let updatedAt = sessions.first { $0.id == id }?.updatedAt ?? 0
        guard let entries = try? await api().getSession(id: id), chat.id == id, !chat.busy else { return }
        // An older server has no pictures to give, which just means none show.
        let images = (try? await api().getSessionImages(id: id)) ?? []
        let files = (try? await api().getSessionFiles(id: id)) ?? []
        // Likewise a server from before it named agents.
        let agents = (try? await api().getSessionAgents(id: id)) ?? []
        // And one from before it kept each step's words, whose steps keep the fallback labels.
        let labels = (try? await api().getToolLabels(sessionId: id)) ?? []
        // And one from before it kept the pages each turn used, whose replies show no sources.
        // A failed fetch keeps the saved ones rather than wiping them.
        let sources = (try? await api().getSessionSources(sessionId: id)) ?? ChatStore.loadSources(id)
        guard chat.id == id, !chat.busy else { return }
        show(entries, images: images, files: files, agents: agents, labels: labels, sources: sources, at: updatedAt)
        ChatStore.saveHistory(id, HistoryCache(updatedAt: updatedAt, entries: entries, images: images, files: files))
        ChatStore.saveSources(id, sources)
        if let session = sessions.first(where: { $0.id == id }) { remember(preview(entries), for: session) }
    }

    /// Puts a fetched or saved history on screen as the open chat's messages. Turns where
    /// the model only called tools fold into the next reply with text, as on Android.
    private func show(_ entries: [HistoryEntry], images: [SentImages], files: [SentFiles], agents: [TurnAgent] = [],
                      labels: [ToolLabel] = [], sources: [TurnSources] = [], at updatedAt: Int64) {
        // The server's words for each step, as the live call carried them; steps from before
        // it kept them have none and get the label from the tool's name.
        var described: [String: String] = [:]
        for label in labels { described[label.callId] = label.description }
        var agentByTurn: [Int64: String] = [:]
        for turn in agents { agentByTurn[turn.message] = turn.agent ?? "" }
        var turnAt: Int64 = 0
        let id = chat.id
        let pictures = picturesByMessage(images)
        var filesByMessage: [Int64: [SentFile]] = [:]
        for sent in files {
            filesByMessage[sent.message] = sent.files.map { SentFile(id: $0.id, name: $0.name, mime: $0.mime, size: Int64($0.size)) }
        }
        var pendingTools: [ToolCall] = []
        var pendingReasoning = ""
        var messages: [Message] = []
        // The index of each turn's last reply with text, which carries the turn's sources.
        var lastReply: [Int64: Int] = [:]
        // The prompt time of the turn the sources go with; a scheduled turn has none, as on Android.
        var sourceTurn: Int64?
        for (index, entry) in entries.enumerated() {
            let mid = "history-\(id)-\(index)"
            switch entry {
            case .user(let content, let timestamp, let auto):
                pendingTools = []
                pendingReasoning = ""
                turnAt = timestamp
                sourceTurn = timestamp
                messages.append(Message(id: mid, role: .user, text: content, pictures: pictures[timestamp] ?? [],
                                        files: filesByMessage[timestamp] ?? [], sentAt: timestamp, auto: auto))
            case .scheduled(let label, let timestamp):
                pendingTools = []
                pendingReasoning = ""
                sourceTurn = nil
                messages.append(Message(id: mid, role: .scheduled, text: label, sentAt: timestamp))
            case .assistant(let content, let timestamp, let tools, let memoryScore, let reasoning, let model):
                let calls = pendingTools + tools.map {
                    ToolCall(id: $0.callId, name: $0.name, description: described[$0.callId] ?? "",
                             done: true, success: $0.success, output: $0.output)
                }
                let thought = [pendingReasoning, reasoning].filter { !$0.isEmpty }.joined(separator: "\n\n")
                if content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    pendingTools = calls
                    pendingReasoning = thought
                } else {
                    pendingTools = []
                    pendingReasoning = ""
                    messages.append(Message(id: mid, role: .assistant, text: content, tools: calls, reasoning: thought,
                                            model: model, memoryScore: memoryScore, agent: agentByTurn[turnAt] ?? "",
                                            repliedAt: timestamp))
                    if let sourceTurn { lastReply[sourceTurn] = messages.count - 1 }
                }
            case .approval(let queryId, let approval):
                messages.append(Message(
                    id: mid, role: .assistant, text: "", tools: pendingTools, reasoning: pendingReasoning,
                    approvals: [ApprovalPrompt(queryId: queryId, approval: approval)]))
                pendingTools = []
                pendingReasoning = ""
            }
        }
        for turn in sources where !turn.sources.isEmpty {
            if let index = lastReply[turn.message] { messages[index].sources = turn.sources }
        }
        chat.messages = keepingLocal(messages)
        chat.historyAt = updatedAt
        if let work = waitingWork[id] {
            let left = work.filter { $0.answer == nil && !$0.withdrawn }
            waitingWork[id] = left.isEmpty ? nil : left
        }
    }

    /// Carries what only this phone knows over to the reloaded history, matching user messages
    /// by text in order: pictures sent from here where the server gave none for that message,
    /// and the model an admin reran it on.
    private func keepingLocal(_ fresh: [Message]) -> [Message] {
        var local = chat.messages.filter {
            $0.role == .user && (!$0.model.isEmpty || $0.pictures.contains { $0.dataUri != nil })
        }
        guard !local.isEmpty else { return fresh }
        var out = fresh
        for index in out.indices where out[index].role == .user {
            guard let match = local.firstIndex(where: { $0.text == out[index].text }) else { continue }
            if out[index].pictures.isEmpty { out[index].pictures = local[match].pictures }
            out[index].model = local[match].model
            out[index].modelThinking = local[match].modelThinking
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
        ChatStore.saveList(sessions: sessions, groups: groups)
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

    /// Sends a message, with up to eight pictures as JPEGs and any other files.
    func send(_ text: String, images: [Data] = [], files: [SentFile] = [], spoken: Bool = false, reply: UInt64 = 0) {
        var trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !images.isEmpty || !files.isEmpty, !chat.busy else { return }
        // The server refuses a message without words; a photo or file sent on its own gets
        // some, as Android does.
        if trimmed.isEmpty {
            trimmed = files.isEmpty ? (images.count > 1 ? "What are these?" : "What is this?")
                : files.count + images.count > 1 ? "Have a look at these files." : "Have a look at this file."
        }
        if chat.messages.isEmpty { chat.title = String((trimmed.isEmpty ? "Photo" : trimmed).prefix(44)) }
        startTurn(trimmed, images: images.map(dataURI), files: files, spoken: spoken, reply: reply)
    }

    /// A message said in live mode, in chat `chatId` or, when nil, a new one, as the live
    /// session's reply number `reply`. A spoken reply still running is stopped first, so only
    /// the new one is read out; a typed one is waited for, as a typed message would.
    /// Returns the chat it went to.
    func sendSpoken(_ text: String, in chatId: String?, reply: UInt64) -> String {
        openGroup = nil
        if let chatId { openChat(chatId) } else { newChat() }
        destination = .chat(chat.id)
        if chat.busy, chat.pendingSpoken || (!liveRunId.isEmpty && chat.runId == liveRunId) { stop() }
        liveRunId = ""
        if chat.busy {
            chat.queued.append(QueuedMessage(text: text, images: [], spoken: true, reply: reply))
        } else {
            send(text, spoken: true, reply: reply)
        }
        return chat.id
    }

    /// The live session cancelled the reply to what was said: stops run `runId`, or the
    /// spoken run of the open chat, or else whatever runs in chat `chatId`, and stops
    /// handing its events to live voice.
    func cancelSpokenRun(_ runId: String?, in chatId: String?) {
        let open = chat.busy && (chat.pendingSpoken || (!liveRunId.isEmpty && chat.runId == liveRunId)
            || (runId != nil && chat.runId == runId))
        liveRunId = ""
        if open { return stop() }
        guard runId != nil || chatId != nil else { return }
        Task {
            do {
                let api = try api()
                var id = runId
                if id == nil, let chatId { id = try await api.activeRun(sessionId: chatId)?.id }
                if let id { try await api.cancelRun(id: id) }
            } catch {
                // The run ended already or the server is out of reach; there is nothing
                // more to stop, and live voice no longer reads it either way.
            }
        }
    }

    /// The approval live voice waits on, answered here or from the chat: its new run is
    /// read out as reply `reply`.
    private func followAnswered(_ queryId: String, run runId: String) {
        guard let reply = live.answered(queryId) else { return }
        liveRunId = runId
        liveReply = reply
    }

    /// Starts live voice on this account's connection.
    func startLive(_ listener: LiveListener, holding: Bool) async throws -> LiveSession {
        ensureConnected()
        // A launch from the Action Button can come as the app starts, before it connected.
        let deadline = Date.now.addingTimeInterval(10)
        while !connected, Date.now < deadline {
            try await Task.sleep(for: .milliseconds(200))
        }
        return try await api().startLive(listener: listener, start: LiveStart(sendOnPause: live.sendOnPause, holding: holding))
    }

    /// Shows `text` as the user's next prompt and sends it, on `model` at `thinking` for a rerun.
    private func startTurn(_ text: String, images: [String], files: [SentFile], model: String = "",
                           thinking: ReasoningLevel? = nil, research: Bool = false, spoken: Bool = false,
                           reply: UInt64 = 0) {
        // A new message instead of an answer skips the call that waits on it.
        for index in chat.messages.indices {
            for card in chat.messages[index].approvals.indices
            where !chat.messages[index].approvals[card].background && chat.messages[index].approvals[card].waiting {
                chat.messages[index].approvals[card].skipped = true
                notifier.removeApproval(chat.messages[index].approvals[card].queryId)
            }
        }
        chat.messages.append(Message(id: UUID().uuidString, role: .user, text: text,
                                     pictures: images.map { Picture(dataUri: $0) }, files: files,
                                     model: model, modelThinking: thinking))
        chat.requestId = UUID().uuidString.lowercased()
        chat.pendingText = text
        chat.pendingImages = images
        chat.pendingFiles = files
        chat.pendingModel = model
        chat.pendingThinking = thinking
        chat.pendingResearch = research
        chat.pendingSpoken = spoken
        chat.pendingReply = reply
        chat.error = ""
        launchRun()
    }

    /// Sends the prompt behind `reply` again on another model, as if the first try never
    /// happened: its turn and every turn after it leave the chat and the model's context.
    /// Admins only.
    func rerun(_ reply: Message, model: String, thinking: ReasoningLevel?) async {
        guard let at = chat.messages.firstIndex(where: { $0.id == reply.id }),
            let prompt = chat.messages[..<at].last(where: { $0.role != .assistant })
        else { return }
        await rerunPrompt(prompt, model: model, thinking: thinking)
    }

    /// Deletes the turn `prompt` started and everything after it, then sends it again on
    /// `model`, thinking at `thinking` or, when nil, the admin's usual level.
    func rerunPrompt(_ prompt: Message, model: String, thinking: ReasoningLevel?) async {
        guard prompt.role == .user, !chat.busy else { return }
        let id = chat.id
        do {
            // A prompt loaded from the server holds its pictures and files there, so read them back first.
            var images: [String] = []
            for picture in prompt.pictures {
                if let uri = picture.dataUri {
                    images.append(uri)
                } else if let fileId = picture.fileId, let file = await pictureFile(fileId) {
                    images.append("data:\(picture.mime);base64," + (try Data(contentsOf: file)).base64EncodedString())
                } else {
                    throw RerunFailure.picture
                }
            }
            for file in prompt.files { _ = try await sentFile(file) }
            try await deleteTurn(prompt, onward: true)
            guard chat.id == id, !chat.busy else { return }
            startTurn(prompt.text, images: images, files: prompt.files, model: model, thinking: thinking)
        } catch {
            if chat.id == id { chat.error = describe(error) }
        }
    }

    private enum RerunFailure: LocalizedError {
        case picture
        var errorDescription: String? { "Couldn't load a picture from that message." }
    }

    /// `deleteTurn` from the message menu, showing a failure in the chat.
    func delete(_ prompt: Message) async {
        let id = chat.id
        do {
            try await deleteTurn(prompt)
        } catch {
            if chat.id == id { chat.error = describe(error) }
        }
    }

    /// Removes `prompt` with the reply and tool calls that followed it, here and on the server.
    /// With `onward`, every later turn goes too.
    func deleteTurn(_ prompt: Message, onward: Bool = false) async throws {
        let id = chat.id
        guard !chat.busy else { return }
        let sentAt = try await serverTime(prompt, in: id)
        if sentAt != 0 { try await api().deleteTurn(sessionId: id, timestamp: sentAt, onward: onward) }
        guard chat.id == id, let start = chat.messages.firstIndex(where: { $0.id == prompt.id }) else { return }
        let end = onward ? chat.messages.endIndex
            : chat.messages[(start + 1)...].firstIndex { $0.role != .assistant } ?? chat.messages.endIndex
        chat.messages.removeSubrange(start..<end)
    }

    /// When the server saved `prompt`: its own time, or else the matching entry of the history.
    private func serverTime(_ prompt: Message, in id: String) async throws -> Int64 {
        if prompt.sentAt != 0 { return prompt.sentAt }
        let nth = chat.messages.prefix { $0.id != prompt.id }.filter { $0.role == prompt.role && $0.text == prompt.text }.count
        let times = try await api().getSession(id: id).compactMap { entry -> Int64? in
            switch entry {
            case .user(let content, let timestamp, _): prompt.role == .user && content == prompt.text ? timestamp : nil
            case .scheduled(let label, let timestamp): prompt.role == .scheduled && label == prompt.text ? timestamp : nil
            case .assistant, .approval: nil
            }
        }
        return times.indices.contains(nth) ? times[nth] : 0
    }

    /// Holds a message until the running reply finishes.
    func queue(_ text: String, images: [Data] = [], files: [SentFile] = []) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !images.isEmpty || !files.isEmpty else { return }
        chat.queued.append(QueuedMessage(text: trimmed, images: Array(images.prefix(ChatPhotos.limit)), files: files))
    }

    /// Drops the queued message at `index` and returns it, so it can go back into the composer.
    func unqueue(_ index: Int) -> QueuedMessage? {
        guard chat.queued.indices.contains(index) else { return nil }
        return chat.queued.remove(at: index)
    }

    /// Sends the first queued message now; the rest follow it.
    func sendQueued() {
        guard !chat.busy, let next = unqueue(0) else { return }
        send(next.text, images: next.images, files: next.files, spoken: next.spoken, reply: next.reply)
    }

    /// Sends the last prompt again after a run that did not finish.
    func retry() {
        guard !chat.busy, let last = chat.messages.last(where: { $0.role == .user }) else { return }
        chat.requestId = UUID().uuidString.lowercased()
        chat.pendingText = last.text
        chat.pendingImages = last.pictures.compactMap(\.dataUri)
        chat.pendingFiles = last.files
        chat.pendingModel = last.model
        chat.pendingThinking = last.modelThinking
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
        chat.clearPending()
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
            let stream = try await sendPending(api, chat)
            while runId.isEmpty, let received = try await stream.next() {
                if case .runStarted(let started, _) = received.event { runId = started }
            }
            guard !runId.isEmpty else { return }
        }
        try await api.cancelRun(id: runId)
    }

    func answer(_ prompt: ApprovalPrompt, _ decision: ApprovalDecision) {
        if prompt.background {
            let sessionId = chat.id
            Task { await answerWork(prompt, in: sessionId, decision) }
            return
        }
        if let index = chat.messages.firstIndex(where: { $0.approvals.contains { $0.queryId == prompt.queryId } }),
            let card = chat.messages[index].approvals.firstIndex(where: { $0.queryId == prompt.queryId }) {
            guard chat.messages[index].approvals[card].waiting else { return }
            chat.messages[index].approvals[card].answer = decision
        } else if live.asking?.queryId != prompt.queryId {
            // Not in the chat and not the one live voice shows in its dock.
            return
        }
        // Answered in the chat, its notification no longer asks, as on Android.
        notifier.removeApproval(prompt.queryId)
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
                followAnswered(prompt.queryId, run: run.id)
                launchRun()
            } catch BeepError.Conflict {
                // The server already has an answer for this call.
            } catch {
                guard chat.id == sessionId else { return }
                chat.error = describe(error)
                // Nothing was answered, so the card asks again.
                for index in chat.messages.indices {
                    if let card = chat.messages[index].approvals.firstIndex(where: { $0.queryId == prompt.queryId }) {
                        chat.messages[index].approvals[card].answer = nil
                    }
                }
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
                        stream = try await sendPending(api, chat)
                    } else {
                        stream = try await api.events(runId: chat.runId, after: chat.cursor)
                    }
                    var first = true
                    while let received = try await stream.next() {
                        guard !Task.isCancelled, chat.id == chatId else { return }
                        if case .runFinished(let status, _) = received.event, first, chat.runId.isEmpty {
                            // A resent message whose run already finished: its reply is in the history.
                            chat.clearPending()
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
                    let spokenRun = !liveRunId.isEmpty && chat.runId == liveRunId
                    if failures >= 4 {
                        if chat.pendingSpoken {
                            live.replyFailed(chat.pendingReply, .notSent)
                        } else if spokenRun {
                            live.replyFailed(liveReply, .connectionLost)
                        }
                        connect()
                        return
                    }
                    if spokenRun { live.reconnecting(liveReply) }
                    try? await Task.sleep(for: .milliseconds(1000 << (failures - 1)))
                }
            }
        }
    }

    /// Sends `chat`'s pending message, or asks for the run it already started.
    private func sendPending(_ api: BeepClient, _ chat: Chat) async throws -> EventStream {
        if chat.pendingResearch {
            return try await api.startResearch(sessionId: chat.id, requestId: chat.requestId, topic: chat.pendingText)
        }
        if chat.pendingSpoken {
            return try await api.chatSpoken(sessionId: chat.id, requestId: chat.requestId, message: chat.pendingText)
        }
        return try await api.chat(
            sessionId: chat.id, requestId: chat.requestId, message: chat.pendingText,
            images: chat.pendingImages, files: try await uploads(chat.pendingFiles), rerun: chat.pendingRerun)
    }

    // MARK: Research

    private func markResearches(_ ids: [String]) {
        let next = researches.union(ids)
        guard next.count != researches.count else { return }
        researches = next
        UserDefaults.standard.set(Array(next), forKey: "researches")
    }

    /// Every research on the account, newest first.
    func listResearches() async throws -> [ResearchSummary] {
        let list = try await api().listResearches()
        markResearches(list.map(\.sessionId))
        return list
    }

    /// Learns which chats are researches, for the drawer to mark them.
    func refreshResearches() async { _ = try? await listResearches() }

    /// The research in chat `id`, with its log entries after `after`; nil while the chat is not one yet.
    func research(_ id: String, after: Int64) async throws -> Research? {
        let research = try await api().getResearch(sessionId: id, after: after)
        if research != nil { markResearches([id]) }
        return research
    }

    func stopResearch(_ id: String) async throws { try await api().stopResearch(sessionId: id) }

    /// Fetches the research's paper as a PDF into a folder of its own and returns the file.
    func researchPdf(_ id: String) async throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appending(path: "research-\(id)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return URL(filePath: try await api().saveResearchPdf(sessionId: id, dir: dir.path(percentEncoded: false)))
    }

    /// Opens a new chat whose first message hands `topic` to a research team.
    func startResearch(_ topic: String) {
        let topic = topic.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !topic.isEmpty else { return }
        openGroup = nil
        newChat()
        markResearches([chat.id])
        chat.title = String(topic.prefix(44))
        startTurn(topic, images: [], files: [], research: true)
    }

    private struct MissedEvent: Error {}

    private func handle(_ received: Event) throws {
        if case .runStarted(let runId, _) = received.event {
            if chat.pendingSpoken {
                liveRunId = runId
                liveReply = chat.pendingReply
            }
            chat.runId = runId
            chat.cursor = 0
            chat.clearPending()
        }
        guard received.seq > chat.cursor else { return }
        guard received.seq == chat.cursor + 1 else { throw MissedEvent() }
        chat.cursor = received.seq
        // Live mode reads out the reply to what was said.
        if !liveRunId.isEmpty, chat.runId == liveRunId {
            live.reply(liveReply, received.event)
            if case .runFinished = received.event { liveRunId = "" }
        }
        let mid = "assistant-\(chat.runId)"

        switch received.event {
        case .textDelta(let delta):
            updateReply(mid) { $0.text += delta }
        case .reasoningDelta(let delta):
            chat.thinking += delta
        case .toolStarted(let name, let callId, let description):
            updateReply(mid) { reply in
                if callId.isEmpty || !reply.tools.contains(where: { $0.id == callId }) {
                    reply.tools.append(ToolCall(id: callId.isEmpty ? UUID().uuidString : callId, name: name,
                                                description: description))
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
            labelAgent(chat.id, reply: mid)
            // Queued messages wait out a run that stopped, failed or waits on an approval, so the
            // user can decide; one said in live voice goes anyway, since live voice waits for its reply.
            let asking = chat.messages.contains { $0.approvals.contains { !$0.background && $0.waiting } }
            if (chat.error.isEmpty && !asking) || chat.queued.first?.spoken == true { sendQueued() }
            Task { await refreshSessions() }
        case .memoryScore(let probability):
            updateReply(mid) { $0.memoryScore = probability }
        case .autoPicked(let pick):
            if let index = chat.messages.lastIndex(where: { $0.role == .user }) { chat.messages[index].auto = pick }
        case .sources(let sources):
            if !sources.isEmpty { updateReply(mid) { $0.sources = sources } }
        case .runStarted, .usage, .transcript:
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

    /// Questions about the phone. Beep no longer drives the browser, so `browser` is refused.
    private func answerDeviceQuery(runId: String, queryId: String, query: DeviceQuery) {
        Task {
            let answer: DeviceAnswer
            switch query {
            case .contacts(let name): answer = .ok(output: await PhoneContacts.search(name))
            case .browser: answer = .err(message: "Beep cannot use the browser on the phone.")
            case .approve, .macFiles: answer = .err(message: "The iPhone app cannot do that yet.")
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
        case .link(let url): openBrowser(url)
        case .alarm(let alarm): Task { await PhoneAlarms.set(alarm) }
        default: break
        }
    }

    /// Shows `url` in the browser screen, over the chat.
    func openBrowser(_ url: URL) {
        browser.open(url)
        browserOpen = true
    }

    /// The bytes to send for `files`, read from the attached copy or downloaded again.
    private func uploads(_ files: [SentFile]) async throws -> [Upload] {
        var out: [Upload] = []
        for file in files {
            let url = try await sentFile(file)
            out.append(Upload(name: file.name, mime: file.mime, data: try Data(contentsOf: url)))
        }
        return out
    }

    /// `file` on this phone: the copy attached here, or else the download.
    func sentFile(_ file: SentFile) async throws -> URL {
        if let local = file.local { return local }
        return try await sentFile(id: file.id, name: file.name)
    }

    /// A file Beep sent, downloaded once into the cache under its own name.
    func sentFile(id: String, name: String) async throws -> URL {
        let url = SentFileCache.local(id: id, name: name)
        if FileManager.default.fileExists(atPath: url.path) { return url }
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        _ = try await api().saveFile(id: id, path: url.path)
        return url
    }

    /// Keeps the list's preview current, and when the app is in the background
    /// shows the reply as a notification, since only routines get notices.
    /// Names on `reply` the specialized agent the newest turn of chat `id` ran as.
    private func labelAgent(_ id: String, reply: String) {
        Task {
            guard let agent = try? await api().getSessionAgents(id: id).last?.agent, chat.id == id,
                let index = chat.messages.firstIndex(where: { $0.id == reply })
            else { return }
            chat.messages[index].agent = agent
        }
    }

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
                sessionId: id, requestId: UUID().uuidString.lowercased(), message: text, images: [], files: [], rerun: nil)
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
        if let prompt = waitingWork[sessionId]?.first(where: { $0.queryId == queryId }) {
            await answerWork(prompt, in: sessionId, allow ? .allow : .deny)
        } else if group {
            try? await api().answerGroupApproval(queryId: queryId, allow: allow)
        } else if sessionId == chat.id, let prompt = chat.messages.flatMap(\.approvals).first(where: { $0.queryId == queryId }) {
            answer(prompt, allow ? .allow : .deny)
        } else {
            _ = try? await api().answerApproval(sessionId: sessionId, queryId: queryId, decision: allow ? .allow : .deny)
        }
    }

    // MARK: Background work

    /// Keeps a question from background work so its chat shows a card for it. Turns ask in
    /// their own run; these only come as a notice, with no run id.
    func askedInBackground(_ notice: Notice) {
        guard !notice.group, let pending = notice.approval, pending.runId.isEmpty else { return }
        var list = waitingWork[notice.sessionId] ?? []
        guard !list.contains(where: { $0.queryId == pending.queryId }) else { return }
        list.append(ApprovalPrompt(queryId: pending.queryId, approval: pending.approval, background: true))
        waitingWork[notice.sessionId] = list
    }

    /// The questions background work in chat `id` waits on, as the server lists them, become its
    /// cards. A card answered here stays until the history reloads. A server from before the
    /// list fails the call, and the saved cards stay.
    func refreshQuestions(_ id: String) async {
        guard !id.isEmpty, let asked = try? await api().listQuestions(sessionId: id) else { return }
        let had = waitingWork[id] ?? []
        let listed = Set(asked.map(\.queryId))
        let answered = had.filter { $0.answer != nil && !listed.contains($0.queryId) }
        let waiting = asked.map { pending in
            had.first { $0.queryId == pending.queryId }
                ?? ApprovalPrompt(queryId: pending.queryId, approval: pending.approval, background: true)
        }
        let cards = answered + waiting
        waitingWork[id] = cards.isEmpty ? nil : cards
    }

    /// A question from background work stopped waiting: answered on another device, or the
    /// work no longer needs it. Its card goes, unless this phone answered it, and so does its
    /// notification.
    func questionResolved(_ queryId: String, in sessionId: String) {
        if var cards = waitingWork[sessionId] {
            cards.removeAll { $0.queryId == queryId && $0.answer == nil }
            waitingWork[sessionId] = cards.isEmpty ? nil : cards
        }
        notifier.removeApproval(queryId)
    }

    /// Answers a question from background work. The server answers it with a conflict, since
    /// no run resumes: either it was delivered or the question was settled before, which a
    /// resolved notice then says.
    private func answerWork(_ prompt: ApprovalPrompt, in sessionId: String, _ decision: ApprovalDecision) async {
        guard let card = waitingWork[sessionId]?.firstIndex(where: { $0.queryId == prompt.queryId }),
            waitingWork[sessionId]?[card].answer == nil, waitingWork[sessionId]?[card].withdrawn == false
        else { return }
        waitingWork[sessionId]?[card].answer = decision
        do {
            _ = try await api().answerApproval(sessionId: sessionId, queryId: prompt.queryId, decision: decision)
        } catch BeepError.Conflict {
            // The card keeps the answer.
        } catch {
            markWork(prompt.queryId, in: sessionId) { $0.answer = nil }
            if chat.id == sessionId { chat.error = describe(error) }
        }
    }

    private func markWork(_ queryId: String, in sessionId: String, _ change: (inout ApprovalPrompt) -> Void) {
        guard let card = waitingWork[sessionId]?.firstIndex(where: { $0.queryId == queryId }) else { return }
        change(&waitingWork[sessionId]![card])
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
        waitingWork = [:]
        ChatStore.clear()
        // The new account has not been told the timezone, and the old one's places
        // must not keep their region monitors if the new list fails to load.
        reportedTimezone = nil
        places = []
        location.watch([])
        await refreshAfterConnect()
    }

    // MARK: Devices

    func peekInvite(_ code: String) async throws -> JoinPreview { try await api().peekInvite(code: code) }

    func devices() async throws -> [LinkedDevice] { try await api().listDevices() }

    func removeDevice(_ id: String) async throws { try await api().removeDevice(id: id) }

    func linkDevice(_ offerUri: String) async throws -> LinkedDevice { try await api().linkDevice(offerUri: offerUri) }

    /// A code a new phone or computer joins this account with.
    func inviteDevice() async throws -> DeviceInvite { try await api().inviteDevice(name: UIDevice.current.name) }

    /// Moves this phone into the account that showed `code`, then loads that account's chats.
    func joinAccount(_ code: String) async throws {
        try await api().joinAccount(code: code)
        // The server closes the connection; the next call signs in again.
        try? await Task.sleep(for: .milliseconds(1500))
        newChat()
        sessions = []
        groups = []
        waitingWork = [:]
        ChatStore.clear()
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

    /// The agents Jev picks from and whether they are on; only admins get an answer.
    func agents() async -> Agents? { try? await api().getAgents() }

    func setAgents(_ on: Bool) async throws -> Agents { try await api().setAgents(enabled: on) }

    func memories() async throws -> [Memory] { try await api().listMemories() }
    /// Empty on servers from before reads were listed.
    /// What each memory not about the user is about, by memory id. Empty on servers from before scopes.
    func memoryScopes() async -> [String: MemoryScope] {
        let list = (try? await api().listMemoryScopes()) ?? []
        return Dictionary(list.map { ($0.id, $0.scope) }, uniquingKeysWith: { first, _ in first })
    }

    /// The machines running beep-node on the account. Admins only.
    func machines() async throws -> [Machine] { try await api().listMachines() }

    /// The server sends model choices only to admins.
    var isAdmin: Bool { models?.choices.isEmpty == false }

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

    /// What routines may do without asking, until each grant ends.
    func grants() async throws -> [Grant] { try await api().listGrants() }
    func revokeGrant(_ id: String) async throws { try await api().revokeGrant(id: id) }

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

/// Like `describe`, but says why the server could not be reached.
func explain(_ error: Error) -> String {
    if case BeepError.Connection(let reason) = error { return "Can't reach the server: \(reason)." }
    return describe(error)
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

