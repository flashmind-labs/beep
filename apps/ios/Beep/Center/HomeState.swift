import Foundation
import Observation

/// The key a question is known by, in the bell, on Home and in the seen list.
func questionKey(machine: String, agent: String, session: String, question: UInt32) -> String {
    "\(machine):\(agent):\(session):\(question)"
}

extension MobileNotice: Identifiable {
    var askKey: String? {
        question.map { questionKey(machine: machineId, agent: agent, session: session, question: $0) }
    }
}

extension Need {
    var key: String {
        switch self {
        case .ask(let session):
            questionKey(machine: session.machineId, agent: session.agent, session: session.id, question: session.question?.id ?? 0)
        case .pull(let item): "\(item.host)/\(item.repo)#\(item.number)"
        case .conflict(let project, let path, _): "\(project):\(path)"
        }
    }
}

/// The notices the bell lists, newest first. It keeps the last hundred.
struct BellLog {
    static let limit = 100
    private(set) var notices: [MobileNotice] = []

    mutating func add(_ notice: MobileNotice) {
        notices.removeAll { $0.id == notice.id }
        notices.append(notice)
        notices.sort { $0.atMs > $1.atMs }
        if notices.count > Self.limit { notices.removeLast(notices.count - Self.limit) }
    }

    mutating func clear() { notices = [] }

    mutating func remove(id: String) { notices.removeAll { $0.id == id } }

    /// Drops questions that no longer wait, given the keys that still do.
    mutating func keepAsks(waiting: Set<String>) {
        notices.removeAll { $0.kind == .ask && !($0.askKey.map(waiting.contains) ?? false) }
    }
}

typealias AnswerCoding = (_ machine: String, _ agent: String, _ session: String, _ question: UInt32, _ reply: String) async throws -> Void

/// What Home shows. An answered question leaves the list at once and comes back if the send fails.
@MainActor @Observable final class HomeModel {
    private let load: () async throws -> MobileHome
    private let send: AnswerCoding
    private(set) var home: MobileHome?
    private(set) var error = ""
    private var hidden: Set<String> = []

    init(load: @escaping () async throws -> MobileHome, send: @escaping AnswerCoding) {
        self.load = load
        self.send = send
    }

    var needs: [Need] { (home?.needs ?? []).filter { !hidden.contains($0.key) } }
    /// The number on the bell.
    var badge: Int { needs.count }
    /// The questions that still wait.
    var waiting: Set<String> { Set((home?.needs ?? []).map(\.key)) }

    func refresh() async {
        do {
            home = try await load()
            error = ""
        } catch {
            self.error = describe(error)
        }
    }

    func answer(_ session: MachineSession, option: AgentOption) async {
        guard let question = session.question else { return }
        let key = Need.ask(session: session).key
        hidden.insert(key)
        do {
            try await send(session.machineId, session.agent, session.id, question.id, option.id)
            home?.needs.removeAll { $0.key == key }
            error = ""
        } catch {
            self.error = describe(error)
        }
        hidden.remove(key)
    }
}
