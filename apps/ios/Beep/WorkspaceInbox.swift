import SwiftUI

struct WorkspaceInbox: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    var activity = false
    @State private var sessions: [MachineSession] = []
    @State private var approvals: [ChatApproval] = []
    @State private var answered: [String: ApprovalDecision] = [:]
    @State private var busy: Set<String> = []
    @State private var error = ""
    @State private var loaded = false

    var body: some View {
        WorkspacePage(activity ? "Activity" : "Notifications", back: { path.removeLast() }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceError(text: error, retry: load)
                    if !loaded { WorkspaceLoading() }
                    else {
                        let shown = activity ? sessions : sessions.filter { $0.question != nil }
                        if !shown.isEmpty {
                            WorkspaceHeading(title: activity ? "Recent sessions" : "Needs input")
                            WorkspaceBox {
                                ForEach(shown, id: \.id) { session in
                                    CodingSessionRow(session: session) { path.append(.agentSession(session.machineId, session.agent, session.id)) }
                                    if session.id != shown.last?.id { Divider().overlay(Mint.soft) }
                                }
                            }
                        }
                        ForEach(approvals, id: \.pending.queryId) { item in
                            Text(item.sessionTitle.isEmpty ? "Chat approval" : item.sessionTitle).font(.body(12)).foregroundStyle(Mint.muted)
                            ApprovalCard(prompt: ApprovalPrompt(queryId: item.pending.queryId, approval: item.pending.approval,
                                                                answer: answered[item.pending.queryId])) { decision in answer(item, decision) }
                                .disabled(busy.contains(item.pending.queryId))
                        }
                        if !model.updates.isEmpty {
                            UpdatesCard(updates: model.updates) { update in
                                Task { if await model.openUpdateChat(update) { path = [] } }
                            } dismiss: { model.dismissUpdates($0) }
                        }
                        if shown.isEmpty, approvals.isEmpty, model.updates.isEmpty {
                            WorkspaceEmpty(title: activity ? "No recent activity" : "You're caught up", detail: "Agent questions and app updates appear here.")
                        }
                    }
                }.padding(16)
            }.refreshable { await load() }
        }.task { await load() }
    }
    private func answer(_ item: ChatApproval, _ decision: ApprovalDecision) {
        let key = item.pending.queryId
        busy.insert(key)
        Task {
            defer { busy.remove(key) }
            do {
                _ = try await model.workspace.client().answerApproval(sessionId: item.sessionId, queryId: key, decision: decision)
                answered[key] = decision
            } catch { self.error = describe(error) }
        }
    }
    private func load() async {
        error = ""
        do { sessions = try await model.workspace.refreshSessions() } catch { self.error = describe(error) }
        do { approvals = try await model.workspace.client().workspaceApprovals() } catch { self.error += (self.error.isEmpty ? "" : "\n") + describe(error) }
        await model.loadUpdates()
        loaded = true
    }
}
