import Foundation

/// The chat list and each chat's history as last fetched, under Application Support/chats,
/// so chats show at once and open without a connection.
enum ChatStore {
    private static let writes = DispatchQueue(label: "beep.chat-store")

    private static var folder: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("chats", isDirectory: true)
    }

    private static var listFile: URL { folder.appendingPathComponent("list.bin") }

    private static var workFile: URL { folder.appendingPathComponent("work.json") }

    private static func historyFile(_ id: String) -> URL {
        folder.appendingPathComponent(id.replacingOccurrences(of: "/", with: "_") + ".bin")
    }

    private static func sourcesFile(_ id: String) -> URL {
        folder.appendingPathComponent(id.replacingOccurrences(of: "/", with: "_") + ".sources.json")
    }

    static func loadList() -> ChatListCache {
        guard let data = try? Data(contentsOf: listFile) else { return ChatListCache(sessions: [], groups: []) }
        return decodeChatListCache(bytes: data)
    }

    static func saveList(sessions: [Session], groups: [Group]) {
        write(encodeChatListCache(cache: ChatListCache(sessions: sessions, groups: groups)), to: listFile)
    }

    static func loadHistory(_ id: String) -> HistoryCache? {
        guard let data = try? Data(contentsOf: historyFile(id)) else { return nil }
        return decodeHistoryCache(bytes: data)
    }

    static func saveHistory(_ id: String, _ cache: HistoryCache) {
        write(encodeHistoryCache(cache: cache), to: historyFile(id))
    }

    /// The web pages each turn of a chat used, saved beside its history since the bridge's
    /// cache has no place for them. Empty when none were saved.
    static func loadSources(_ id: String) -> [TurnSources] {
        guard let data = try? Data(contentsOf: sourcesFile(id)),
            let saved = try? JSONDecoder().decode([SavedSources].self, from: data)
        else { return [] }
        return saved.map(\.turn)
    }

    static func saveSources(_ id: String, _ sources: [TurnSources]) {
        guard let data = try? JSONEncoder().encode(sources.map { SavedSources($0) }) else { return }
        write(data, to: sourcesFile(id))
    }

    /// Drops the histories of chats no longer in `keep`.
    static func prune(keep: Set<String>) {
        let names = Set(keep.flatMap { [historyFile($0).lastPathComponent, sourcesFile($0).lastPathComponent] })
        writes.async {
            let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
            for file in files where file != listFile && file != workFile && !names.contains(file.lastPathComponent) {
                try? FileManager.default.removeItem(at: file)
            }
        }
    }

    /// The questions from background work still waiting, by chat id. They come only as
    /// notices, so nothing on the server gives them back after a relaunch.
    static func loadWork() -> [String: [ApprovalPrompt]] {
        guard let data = try? Data(contentsOf: workFile),
            let saved = try? JSONDecoder().decode([String: [SavedWork]].self, from: data)
        else { return [:] }
        return saved.mapValues { $0.map(\.prompt) }
    }

    static func saveWork(_ work: [String: [ApprovalPrompt]]) {
        let waiting = work.mapValues { $0.filter { $0.answer == nil && !$0.withdrawn }.map { SavedWork($0) } }
            .filter { !$0.value.isEmpty }
        guard let data = try? JSONEncoder().encode(waiting) else { return }
        write(data, to: workFile)
    }

    /// Forgets everything, as when the phone signs in to another account.
    static func clear() {
        writes.async { try? FileManager.default.removeItem(at: folder) }
    }

    private static func write(_ data: Data, to url: URL) {
        writes.async {
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try? data.write(to: url, options: .atomic)
        }
    }
}

/// A waiting question from background work as saved; the bridge's records are not Codable.
private struct SavedWork: Codable {
    struct Field: Codable {
        var label: String
        var value: String
    }

    var queryId: String
    var app: String
    var action: String
    var fields: [Field]

    init(_ prompt: ApprovalPrompt) {
        queryId = prompt.queryId
        app = prompt.approval.app
        action = prompt.approval.action
        fields = prompt.approval.fields.map { Field(label: $0.label, value: $0.value) }
    }

    var prompt: ApprovalPrompt {
        ApprovalPrompt(
            queryId: queryId,
            approval: Approval(app: app, action: action, fields: fields.map { ApprovalField(label: $0.label, value: $0.value) }),
            background: true)
    }
}

/// One turn's web pages as saved; the bridge's records are not Codable.
private struct SavedSources: Codable {
    struct Page: Codable {
        var url: String
        var title: String
    }

    var message: Int64
    var pages: [Page]

    init(_ turn: TurnSources) {
        message = turn.message
        pages = turn.sources.map { Page(url: $0.url, title: $0.title) }
    }

    var turn: TurnSources {
        TurnSources(message: message, sources: pages.map { WebSource(url: $0.url, title: $0.title) })
    }
}
