import BackgroundTasks
import Foundation

/// iOS wakes the app now and then to look for questions that wait while the feed is closed.
enum BackgroundAsks {
    static let identifier = "com.flashmind.beep.asks"

    @MainActor static func register(model: AppModel) {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
            let work = Task { @MainActor in
                schedule()
                let found = await check(model)
                task.setTaskCompleted(success: found)
            }
            task.expirationHandler = { work.cancel() }
        }
    }

    static func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = .now.addingTimeInterval(15 * 60)
        try? BGTaskScheduler.shared.submit(request)
    }

    /// Posts each question not posted before. No feed runs in the background.
    @MainActor private static func check(_ model: AppModel) async -> Bool {
        model.connect()
        guard let sessions = try? await model.workspace.client().codingSessions(project: nil) else { return false }
        var waiting: Set<String> = []
        for session in sessions {
            guard let question = session.question, NotifyPrefs.on(NotifyPrefs.asks) else { continue }
            waiting.insert(questionKey(machine: session.machineId, agent: session.agent, session: session.id, question: question.id))
            AskAlerts.post(machine: session.machineId, agent: session.agent, session: session.id, question: question.id,
                           title: question.text, detail: [session.title ?? agentName(session.agent), session.machineName].joined(separator: " · "),
                           options: question.options.map { AskOption(id: $0.id, name: $0.name, allow: $0.allow) })
        }
        AskAlerts.keepSeen(waiting: waiting)
        return true
    }
}
