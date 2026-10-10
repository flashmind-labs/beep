import Foundation
import Network
import Observation
import UIKit

/// One tool call shown as a chip.
struct ToolCall: Identifiable, Equatable {
    var id: String
    var name: String
    var done = false
    var success = true

    var label: String { toolLabel(name) }
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
    var stopRequested = false

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
    private(set) var connectionError = ""
    var chat = Chat()
    private(set) var sessions: [Session] = []
    private(set) var groups: [Group] = []
    /// The last group opened.
    private(set) var openGroupId: String?
    /// The chat or group id on screen, so its notices are not shown.
    var onScreen: String? {
        didSet { if let onScreen { notifier.clear(onScreen) } }
    }
    /// Where a tapped notification wants to go; the root view follows it and clears it.
    var destination: Destination?
    /// A member to @mention when the group chat shows again, from the member sheet.
    var pendingMention: String?
    /// The last line of each chat for the list, cached by the chat's update time.
    private(set) var previews: [String: ChatPreview] = ChatPreview.load()
    /// Saved places, from the server once it can list them.
    private(set) var places: [SavedPlace] = []
    var placesStore: any PlacesStore = PendingPlaces()
    private(set) var starters: [PromptSuggestion] = []
    private(set) var models: Models?

    private var client: BeepClient?
    private var runTask: Task<Void, Never>?
    private var connectTask: Task<Void, Never>?
    private let pathMonitor = NWPathMonitor()
    let location = LocationReporter()
    let notifier = ChatNotifier()
    private var previewLoads: Set<String> = []
    private var reportedTimezone: String?
    private let pictureCache = PictureCache()

    init() {
        let key = Identity.loadOrCreate()
        identity = key
        accountName = Identity.name(key)
        onboarded = UserDefaults.standard.bool(forKey: "onboarded")
        location.onUpdate = { [weak self] fix in self?.sendLocation(fix) }
        notifier.model = self
        pathMonitor.pathUpdateHandler = { [weak self] path in
            guard path.status == .satisfied else { return }
            Task { @MainActor in await self?.client?.networkChanged() }
        }
        pathMonitor.start(queue: DispatchQueue(label: "beep.path"))
    }

    // MARK: Connection

    /// Connects to the server, retrying with backoff until it answers.
    func connect() {
        connectTask?.cancel()
        connectTask = Task {
            var attempt = 0
            while !Task.isCancelled {
                attempt += 1
                do {
                    let fresh = try client ?? BeepClient(nodeId: BeepConfig.nodeId, identity: identity)
                    if client == nil { fresh.setListener(listener: notifier) }
                    client = fresh
                    try await fresh.health()
                    connected = true
                    connectionError = ""
                    await refreshAfterConnect()
                    return
                } catch {
                    connected = false
                    connectionError = describe(error)
                    if attempt >= 10 { return }
                    try? await Task.sleep(for: .milliseconds(min(1000 * attempt, 15000)))
                }
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

    // MARK: Sessions

    func refreshSessions() async {
        guard let list = try? await api().listSessions() else { return }
        sessions = list.sorted { ($0.pinned ? 1 : 0, $0.updatedAt) > ($1.pinned ? 1 : 0, $1.updatedAt) }
    }

    func newChat() {
        stopStreaming()
        openGroupId = nil
        chat = Chat()
    }

    func open(_ session: Session) {
        openGroupId = nil
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
            openGroupId = nil
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
                let calls = pendingTools + tools.map { ToolCall(id: $0.callId, name: $0.name, done: true, success: $0.success) }
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

    func openGroup(_ id: String) {
        openGroupId = id
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
        if openGroupId == id { openGroupId = nil }
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
        chat.stopRequested = false
        launchRun()
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

    func stop() {
        chat.stopRequested = true
        let runId = chat.runId
        guard !runId.isEmpty else { return }
        Task { try? await api().cancelRun(id: runId) }
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
        runTask = Task {
            defer { if chat.id == chatId { runTask = nil } }
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
            if chat.stopRequested { stop() }
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
        case .toolResult(_, let callId, _, let success):
            updateReply(mid) { reply in
                guard let index = reply.tools.firstIndex(where: { callId.isEmpty ? !$0.done : $0.id == callId }) else { return }
                reply.tools[index].done = true
                reply.tools[index].success = success
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
            chat.stopRequested = false
            chat.error = runEndError(status, current: chat.error)
            finished(chat)
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

    /// Questions about the phone. Contacts and the browser are not wired up on iPhone yet.
    private func answerDeviceQuery(runId: String, queryId: String, query: DeviceQuery) {
        Task {
            let answer = DeviceAnswer.err(message: "The iPhone app cannot do that yet.")
            _ = try? await api().deviceResult(runId: runId, queryId: queryId, answer: answer)
        }
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
    func reply(toChat id: String, text: String) async {
        if id == chat.id {
            send(text)
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
        if let index = places.firstIndex(where: { $0.id == id }) { places[index] = place }
        location.watch(places)
    }

    // MARK: Account

    /// Signs in as the account in a key file, after checking the server accepts it.
    func importKey(_ text: String) async throws {
        guard let key = Identity.parse(text) else { throw BeepError.BadRequest(reason: "This file has no key") }
        if key == identity { return }
        let trial = try BeepClient(nodeId: BeepConfig.nodeId, identity: key)
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

    func memories() async throws -> [Memory] { try await api().listMemories() }
    func updateMemory(_ id: String, _ content: String) async throws { try await api().updateMemory(id: id, content: content) }
    func deleteMemory(_ id: String) async throws { try await api().deleteMemory(id: id) }
    func clearMemories() async throws { _ = try await api().clearMemories() }

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
        openGroupId = nil
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
