import Foundation
import Observation
import UIKit
import UserNotifications

/// Which notices the user wants as local notifications.
enum NotifyPrefs {
    static let asks = "notify.asks"
    static let finished = "notify.finished"
    static let ci = "notify.ci"

    static func on(_ key: String) -> Bool { UserDefaults.standard.object(forKey: key) as? Bool ?? true }
}

/// Local notifications for agent questions, with Allow and Deny on them.
enum AskAlerts {
    static let category = "ask"
    static let allow = "ask.allow"
    static let deny = "ask.deny"
    private static let seenKey = "asks.seen"

    static var categoryDefinition: UNNotificationCategory {
        UNNotificationCategory(identifier: category, actions: [
            UNNotificationAction(identifier: allow, title: "Allow", options: [.authenticationRequired]),
            UNNotificationAction(identifier: deny, title: "Deny", options: [.destructive]),
        ], intentIdentifiers: [])
    }

    /// Posts the question unless it was posted before. A question stays seen while it waits.
    static func post(machine: String, agent: String, session: String, question: UInt32, title: String, detail: String,
                     options: [AskOption]) {
        let key = questionKey(machine: machine, agent: agent, session: session, question: question)
        var seen = UserDefaults.standard.stringArray(forKey: seenKey) ?? []
        guard !seen.contains(key) else { return }
        seen.append(key)
        UserDefaults.standard.set(Array(seen.suffix(200)), forKey: seenKey)
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = detail
        content.sound = .default
        content.categoryIdentifier = category
        content.userInfo = [
            "askMachine": machine, "askAgent": agent, "askSession": session, "askQuestion": Int(question),
            "askAllow": options.first { $0.allow }?.id ?? "", "askDeny": options.first { !$0.allow }?.id ?? "",
        ]
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: key, content: content, trigger: nil))
    }

    /// Forgets the questions that no longer wait.
    static func keepSeen(waiting: Set<String>) {
        let seen = (UserDefaults.standard.stringArray(forKey: seenKey) ?? []).filter(waiting.contains)
        UserDefaults.standard.set(seen, forKey: seenKey)
    }
}

/// Follows the asks feed while the app runs and keeps the notices the bell lists.
@MainActor @Observable final class AskCenter {
    static let shared = AskCenter()

    private(set) var log = BellLog()
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var client: (() throws -> BeepClient)?

    /// Starts once. The feed ends when iOS suspends the app, and the loop opens it again.
    func start(client: @escaping () throws -> BeepClient) {
        self.client = client
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                do {
                    let feed = try await client().followAsks()
                    while let notice = try await feed.next() { self?.receive(notice) }
                } catch {}
                try? await Task.sleep(for: .seconds(5))
            }
        }
    }

    func receive(_ notice: MobileNotice) {
        log.add(notice)
        switch notice.kind {
        case .ask:
            guard NotifyPrefs.on(NotifyPrefs.asks), let question = notice.question else { return }
            AskAlerts.post(machine: notice.machineId, agent: notice.agent, session: notice.session, question: question,
                           title: notice.title, detail: notice.detail, options: notice.options)
        case .finished, .run:
            guard NotifyPrefs.on(notice.kind == .finished ? NotifyPrefs.finished : NotifyPrefs.ci) else { return }
            let content = UNMutableNotificationContent()
            content.title = notice.title
            content.body = notice.detail
            UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: notice.id, content: content, trigger: nil))
        }
    }

    func clear() { log.clear() }

    /// Drops the questions that no longer wait.
    func keepAsks(waiting: Set<String>) { log.keepAsks(waiting: waiting) }

    /// The row leaves the bell at once and comes back if the answer does not go through.
    func answer(_ notice: MobileNotice, option: AskOption) async -> String? {
        guard let question = notice.question, let client else { return nil }
        log.remove(id: notice.id)
        do {
            try await client().answerCoding(machineId: notice.machineId, agent: notice.agent, session: notice.session,
                                            question: question, reply: option.id)
            return nil
        } catch {
            log.add(notice)
            return describe(error)
        }
    }
}
