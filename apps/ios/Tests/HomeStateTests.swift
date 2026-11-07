import XCTest
@testable import Beep

@MainActor
final class HomeStateTests: XCTestCase {
    private func session(_ id: String, question: UInt32? = 1) -> MachineSession {
        MachineSession(machineId: "build-box", machineName: "build-box", id: id, agent: "claude", title: "Task \(id)",
                       cwd: "/srv/project", status: .asking, doing: nil, startedAt: .now, lastActivity: .now, external: false,
                       question: question.map {
                           AgentQuestion(id: $0, text: "Run tests?", detail: nil,
                                         options: [AgentOption(id: "once", name: "Yes", allow: true),
                                                   AgentOption(id: "no", name: "No", allow: false)])
                       })
    }

    private func item() -> MobileGitItem {
        MobileGitItem(host: "github.com", repo: "example/app", number: 7, kind: .pullRequest, title: "Fix", url: "https://example.com/7",
                      labels: [], updatedMs: 0, authored: false, reviewRequested: true, changesRequested: false,
                      checksFailing: false, draft: false)
    }

    private func home(_ needs: [Need]) -> MobileHome {
        MobileHome(machines: [], needs: needs, running: [], pulls: [], issues: [], errors: [])
    }

    private func notice(_ n: Int) -> MobileNotice {
        MobileNotice(id: "n\(n)", kind: .finished, title: "Done \(n)", detail: "", machineId: "build-box", agent: "claude",
                     session: "s\(n)", question: nil, options: [], atMs: UInt64(n))
    }

    private struct Failed: Error {}

    func test_badge_counts_needs() async {
        let all = home([.ask(session: session("a")), .pull(item: item()),
                        .conflict(project: "docs", path: "plan.md", machines: ["a", "b"])])
        let model = HomeModel(load: { all }, send: { _, _, _, _, _ in })
        XCTAssertEqual(model.badge, 0)
        await model.refresh()
        XCTAssertEqual(model.badge, 3)
    }

    func test_answer_removes_row_optimistically() async {
        let waiting = session("a")
        let all = home([.ask(session: waiting), .pull(item: item())])
        var sent: [String] = []
        let model = HomeModel(load: { all }, send: { _, _, id, question, reply in sent.append("\(id):\(question):\(reply)") })
        await model.refresh()
        await model.answer(waiting, option: waiting.question!.options[0])
        XCTAssertEqual(sent, ["a:1:once"])
        XCTAssertEqual(model.badge, 1)

        // The row is gone while the send is in flight and returns when it throws.
        let failing = HomeModel(load: { all }, send: { _, _, _, _, _ in throw Failed() })
        await failing.refresh()
        await failing.answer(waiting, option: waiting.question!.options[1])
        XCTAssertEqual(failing.badge, 2)
        XCTAssertFalse(failing.error.isEmpty)
    }

    func test_row_hidden_while_sending() async {
        let waiting = session("a")
        let all = home([.ask(session: waiting)])
        var model: HomeModel!
        var during = -1
        model = HomeModel(load: { all }, send: { _, _, _, _, _ in await MainActor.run { during = model.badge } })
        await model.refresh()
        await model.answer(waiting, option: waiting.question!.options[0])
        XCTAssertEqual(during, 0)
    }

    func test_bell_keeps_last_hundred() {
        var log = BellLog()
        for n in 0..<130 { log.add(notice(n)) }
        XCTAssertEqual(log.notices.count, 100)
        XCTAssertEqual(log.notices.first?.id, "n129")
        XCTAssertEqual(log.notices.last?.id, "n30")
    }
}
