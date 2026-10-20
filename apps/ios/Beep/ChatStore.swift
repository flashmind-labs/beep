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

    private static func historyFile(_ id: String) -> URL {
        folder.appendingPathComponent(id.replacingOccurrences(of: "/", with: "_") + ".bin")
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

    /// Drops the histories of chats no longer in `keep`.
    static func prune(keep: Set<String>) {
        let names = Set(keep.map { historyFile($0).lastPathComponent })
        writes.async {
            let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
            for file in files where file != listFile && !names.contains(file.lastPathComponent) {
                try? FileManager.default.removeItem(at: file)
            }
        }
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
