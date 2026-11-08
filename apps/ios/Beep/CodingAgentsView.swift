import SwiftUI

struct CodingAgentsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var sessions: [MachineSession] = []
    @State private var query = ""
    @State private var loaded = false
    @State private var error = ""
    @State private var starting = false
    @State private var spend: MobileUsage?

    var body: some View {
        WorkspacePage(title: "Coding agents", back: { path.removeLast() }) {
            Button("Start") { starting = true }.buttonStyle(TextAction())
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceSearch(prompt: "Filter sessions", text: $query)
                    WorkspaceError(text: error, retry: load)
                    WorkspaceHeading(title: "Sessions")
                    if !loaded { WorkspaceLoading() }
                    else if sessions.isEmpty { WorkspaceEmpty(title: "No coding sessions", detail: "Start an agent on a linked machine.") }
                    else { WorkspaceBox {
                        ForEach(sessions.filter { query.isEmpty || ($0.title ?? $0.agent).localizedCaseInsensitiveContains(query) || $0.machineName.localizedCaseInsensitiveContains(query) }, id: \.workspaceID) { session in
                            CodingSessionRow(session: session) { path.append(.agentSession(session.machineId, session.agent, session.id)) }
                            if session.id != sessions.last?.id { Divider().overlay(Mint.soft) }
                        }
                    } }
                    if let spend {
                        WorkspaceHeading(title: "Usage today")
                        WorkspaceFacts(rows: [("Estimated cost", String(format: "$%.2f", spend.costUsd)), ("Tokens", spend.tokens.formatted())])
                        WorkspaceBox {
                            ForEach(Array(spend.limits.enumerated()), id: \.offset) { _, limit in
                                WorkspaceRow(icon: "chart.bar", title: agentName(limit.agent), detail: "\(limit.minutes / 60)h window", badge: String(format: "%.0f%% used", limit.usedPercent))
                            }
                        }
                    }
                }.padding(16)
            }.refreshable { await load() }
        }
        .task {
            while !Task.isCancelled {
                await load()
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
            }
        }
        .sheet(isPresented: $starting) {
            StartCodingAgentView { session in starting = false; path.append(.agentSession(session.machineId, session.agent, session.id)) }
        }
    }
    private func load() async {
        do { sessions = try await model.workspace.refreshSessions(); error = "" }
        catch { self.error = describe(error) }
        do { spend = try await model.workspace.client().codingUsage() }
        catch { /* Older servers may not provide usage reports. */ }
        loaded = true
    }
}

struct CodingSessionRow: View {
    let session: MachineSession
    let open: () -> Void
    var body: some View {
        WorkspaceRow(icon: "bubble.left.and.text.bubble.right", title: session.title ?? agentName(session.agent),
                     detail: "\(agentName(session.agent)) · \(session.machineName) · \(session.lastActivity.formatted(.relative(presentation: .named)))",
                     badge: session.status.label, color: session.status.color, action: open)
    }
}

struct StartCodingAgentView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var project: String? = nil
    var prompt = ""
    let started: (MachineSession) -> Void
    @State private var hosts: [AgentHost] = []
    @State private var selectedProject = ""
    @State private var machine = ""
    @State private var agent = ""
    @State private var task = ""
    @State private var cwd = ""
    @State private var newWorktree = true
    @State private var branch = ""
    @State private var models: AgentModels?
    @State private var modelID = ""
    @State private var effort: AgentEffort? = nil
    @State private var error = ""
    @State private var modelError = ""
    @State private var busy = false
    @State private var loaded = false

    private var host: AgentHost? { hosts.first { $0.machineId == machine } }
    private var canStart: Bool { !busy && !machine.isEmpty && !agent.isEmpty && !task.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        WorkspacePage("Start coding agent", back: { dismiss() }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    WorkspaceError(text: error)
                    if !loaded { WorkspaceLoading() }
                    else if hosts.isEmpty { WorkspaceEmpty(title: "No available agents", detail: "A linked machine needs a signed in coding agent.") }
                    else {
                        fieldLabel("Task")
                        TextField("What should the agent do?", text: $task, axis: .vertical).lineLimit(4...8).workspaceField()
                        fieldLabel("Project")
                        Picker("Project", selection: $selectedProject) {
                            Text("No project").tag("")
                            ForEach(Array(Set(model.workspace.projects.map(\.project))).sorted(), id: \.self) { Text($0).tag($0) }
                        }.tint(Mint.ink)
                        fieldLabel("Machine")
                        Picker("Machine", selection: $machine) {
                            ForEach(hosts, id: \.machineId) { Text($0.machineName).tag($0.machineId) }
                        }.tint(Mint.ink)
                        fieldLabel("Folder")
                        TextField("Machine's default folder", text: $cwd).workspaceField()
                        fieldLabel("Coding agent")
                        WorkspaceBox {
                            ForEach(host?.agents ?? [], id: \.self) { slug in
                                WorkspaceRow(icon: "bubble.left.and.text.bubble.right", title: agentName(slug), badge: agent == slug ? "Selected" : "", color: Mint.brand) { agent = slug }
                            }
                        }
                        WorkspaceError(text: modelError)
                        if let models, models.pick != .fixed {
                            fieldLabel("Model")
                            if models.pick == .named {
                                TextField("Default model", text: $modelID).workspaceField()
                            } else {
                                Picker("Model", selection: $modelID) {
                                    Text("Default").tag("")
                                    ForEach(models.models, id: \.id) { Text($0.name).tag($0.id) }
                                }.tint(Mint.ink)
                            }
                        }
                        fieldLabel("Thinking")
                        Picker("Thinking", selection: $effort) {
                            Text("Default").tag(AgentEffort?.none)
                            Text("Low").tag(AgentEffort?.some(.low))
                            Text("Medium").tag(AgentEffort?.some(.medium))
                            Text("High").tag(AgentEffort?.some(.high))
                        }.tint(Mint.ink)
                        if !cwd.isEmpty {
                            Toggle("New worktree", isOn: $newWorktree).font(.body(13)).tint(Mint.accent)
                            if newWorktree { TextField("Branch name (optional)", text: $branch).workspaceField() }
                        }
                        Button(busy ? "Starting..." : "Start agent") { Task { await start() } }
                            .buttonStyle(PrimaryButton()).disabled(!canStart)
                    }
                }.padding(16)
            }
        }
        .task {
            task = prompt; selectedProject = project ?? ""
            do {
                hosts = try await model.workspace.client().codingHosts()
                let checkout = model.workspace.projects.first { checkout in checkout.project == selectedProject && hosts.contains { $0.machineId == checkout.machineId } }
                machine = checkout?.machineId ?? hosts.first?.machineId ?? ""
                chooseFolder(); chooseAgent()
            } catch { self.error = describe(error) }
            loaded = true
        }
        .onChange(of: machine) { chooseFolder(); chooseAgent() }
        .onChange(of: selectedProject) { chooseFolder() }
        .task(id: machine + ":" + agent) {
            models = nil; modelID = ""; modelError = ""
            guard !machine.isEmpty, !agent.isEmpty else { return }
            do { models = try await model.workspace.client().codingModels(machineId: machine, agent: agent) }
            catch { modelError = "Models: " + describe(error) }
        }
    }
    private func fieldLabel(_ title: String) -> some View { Text(title).font(.body(12)).foregroundStyle(Mint.muted) }
    private func chooseAgent() { if !(host?.agents.contains(agent) ?? false) { agent = host?.agents.first ?? "" } }
    private func chooseFolder() {
        cwd = model.workspace.projects.first { $0.project == selectedProject && $0.machineId == machine }?.folder ?? ""
    }
    private func start() async {
        busy = true; defer { busy = false }
        let checkout: AgentCheckoutPlan? = !newWorktree || cwd.isEmpty ? nil : branch.isEmpty ? .newWorktree(base: nil) : .namedWorktree(base: nil, branch: branch)
        do {
            let result = try await model.workspace.client().startCoding(start: CodingStart(machineId: machine, agent: agent,
                cwd: cwd.isEmpty ? nil : cwd, prompt: task, model: modelID.isEmpty ? nil : modelID, effort: effort, checkout: checkout))
            started(result.session)
        } catch { self.error = describe(error) }
    }
}

struct CodingSessionView: View {
    @Environment(AppModel.self) private var model
    let machineID: String
    let agent: String
    let sessionID: String
    let back: () -> Void
    @State private var view: AgentSessionView?
    @State private var usage: AgentSessionUsage?
    @State private var draft = ""
    @State private var error = ""
    @State private var actionError = ""
    @State private var busy = false
    @State private var diff: WorkDiff?
    @State private var showingDiff = false

    var body: some View {
        WorkspacePage(title: view?.session.title ?? agentName(agent), back: back) {
            if let session = view?.session, !session.external, session.status != .ended {
                Button("Stop") { perform { try await model.workspace.client().steerCoding(machineId: session.machineId, agent: session.agent, session: session.id, steer: .cancel) } }
                    .buttonStyle(TextAction(color: Mint.error)).disabled(busy)
            }
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    WorkspaceError(text: error, retry: load)
                    WorkspaceError(text: actionError)
                    if let view {
                        HStack(spacing: 8) {
                            Text(subtitle(view)).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2)
                            Spacer(minLength: 0)
                            WorkspaceBadge(text: view.session.status.label, color: view.session.status.color)
                        }
                        if view.earlier > 0 { Text("\(view.earlier) earlier entries").font(.body(11)).foregroundStyle(Mint.dim) }
                        ForEach(Array(view.entries.enumerated()), id: \.offset) { _, entry in
                            transcript(entry)
                        }
                        if let problem = view.error { WorkspaceError(text: problem) }
                        if !view.plan.isEmpty { plan(view.plan) }
                        if !view.files.isEmpty {
                            Button {
                                perform {
                                    diff = try await model.workspace.client().sessionDiff(machineId: view.session.machineId, root: view.session.cwd)
                                    showingDiff = true
                                }
                            } label: {
                                HStack {
                                    Image(systemName: "doc.text.magnifyingglass")
                                    Text(view.files.count == 1 ? "1 file changed" : "\(view.files.count) files changed")
                                    Spacer()
                                    Text("Review").foregroundStyle(Mint.accent)
                                }.font(.body(13)).padding(12).card()
                            }.buttonStyle(.plain)
                        }
                    } else if error.isEmpty { WorkspaceLoading() }
                }.padding(16)
            }
            .defaultScrollAnchor(.bottom)
            .refreshable { await load() }
            if let view, let question = view.session.question {
                VStack(alignment: .leading, spacing: 9) {
                    Text(question.text).font(.body(14).weight(.semibold))
                    if let detail = question.detail { Text(detail).font(.system(size: 12, design: .monospaced)).foregroundStyle(Mint.muted).lineLimit(6).textSelection(.enabled) }
                    ForEach(question.options, id: \.id) { option in
                        Button(option.name) {
                            perform { try await model.workspace.client().answerCoding(machineId: view.session.machineId, agent: view.session.agent, session: view.session.id, question: question.id, reply: option.id) }
                        }.buttonStyle(PillButton(fill: option.allow ? Mint.accent : Mint.raised)).disabled(busy)
                    }
                }
                .padding(11).frame(maxWidth: .infinity, alignment: .leading)
                .background(Mint.surface, in: RoundedRectangle(cornerRadius: Mint.radius))
                .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.warn.opacity(0.5)))
                .padding(.horizontal, 12).padding(.bottom, 8)
            }
            if let session = view?.session {
                if session.external {
                    Button("Resume session") {
                        perform {
                            let resumed = try await model.workspace.client().resumeCoding(machineId: session.machineId, agent: session.agent, session: session.id)
                            self.view = try await model.workspace.client().codingSession(machineId: resumed.session.machineId, agent: resumed.session.agent, session: resumed.session.id)
                        }
                    }.buttonStyle(PrimaryButton()).disabled(busy).padding(12)
                } else if session.status != .ended {
                    HStack(alignment: .bottom, spacing: 8) {
                        TextField("Message \(agentName(session.agent))", text: $draft, axis: .vertical).lineLimit(1...5).font(.body(14))
                        Button {
                            let text = draft
                            perform {
                                try await model.workspace.client().steerCoding(machineId: session.machineId, agent: session.agent, session: session.id, steer: .prompt(text: text))
                                draft = ""
                            }
                        } label: {
                            Image(systemName: "arrow.up").font(.system(size: 15, weight: .semibold)).foregroundStyle(Mint.onAccent)
                                .frame(width: 32, height: 32).background(Mint.accent, in: Circle())
                        }.disabled(busy || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .padding(.leading, 12).padding(.trailing, 6).padding(.vertical, 6)
                    .background(Mint.surface, in: RoundedRectangle(cornerRadius: 18))
                    .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.soft))
                    .padding(.horizontal, 12).padding(.vertical, 8)
                }
            }
        }
        .task {
            await load()
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(3)) } catch { return }
                await load()
            }
        }
        .sheet(isPresented: $showingDiff) {
            WorkspacePage("Changes", back: { showingDiff = false }) { ScrollView { if let diff { WorkDiffView(diff: diff).padding(16) } } }
        }
    }
    private func subtitle(_ view: AgentSessionView) -> String {
        var parts = [agentName(view.session.agent), view.session.machineName, view.session.cwd]
        if let usage { parts.append(usage.costUsd.map { String(format: "$%.2f", $0) } ?? "\(usage.tokens.formatted()) tokens") }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
    private func plan(_ steps: [AgentPlanStep]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Plan").font(.body(12).weight(.semibold))
            ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                Text("\(index + 1). \(step.content)").font(.body(12))
                    .foregroundStyle(step.status == .inProgress ? Mint.ink : Mint.muted)
                    .strikethrough(step.status == .completed, color: Mint.dim)
            }
        }.padding(11).frame(maxWidth: .infinity, alignment: .leading).card()
    }
    @ViewBuilder private func transcript(_ entry: AgentEntry) -> some View {
        switch entry.kind {
        case .user:
            HStack {
                Spacer(minLength: 40)
                MarkdownText(text: entry.text, size: 13.5).textSelection(.enabled)
                    .padding(.horizontal, 11).padding(.vertical, 8)
                    .background(Mint.accent.opacity(0.22), in: UnevenRoundedRectangle(topLeadingRadius: 12, bottomLeadingRadius: 12, bottomTrailingRadius: 4, topTrailingRadius: 12))
            }.padding(.top, 6)
        case .agent:
            MarkdownText(text: entry.text, size: 13.5).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
        case .tool:
            TranscriptLine(entry: entry)
        case .thought:
            TranscriptLine(entry: entry)
        case .question:
            Text(entry.text).font(.body(12)).foregroundStyle(Mint.warn).frame(maxWidth: .infinity, alignment: .leading)
        default:
            Text(entry.text).font(.body(12)).foregroundStyle(Mint.dim).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private func perform(_ action: @escaping () async throws -> Void) {
        busy = true; actionError = ""
        Task {
            defer { busy = false }
            do { try await action(); await load() } catch { actionError = describe(error) }
        }
    }
    private func load() async {
        let current = view?.session
        do {
            view = try await model.workspace.client().codingSession(machineId: current?.machineId ?? machineID, agent: current?.agent ?? agent, session: current?.id ?? sessionID)
            error = ""
        } catch { self.error = describe(error) }
        do { usage = try await model.workspace.client().codingSessionUsage(machineId: current?.machineId ?? machineID, agent: current?.agent ?? agent, session: current?.id ?? sessionID) }
        catch { /* Usage is optional on servers from before session metering. */ }
    }
}

/// A tool call or a thought as one muted line, tapped open for the full text.
private struct TranscriptLine: View {
    let entry: AgentEntry
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { open.toggle() } label: { label.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle()) }
                .buttonStyle(.plain)
            if open {
                Text(entry.text).font(.system(size: 11, design: .monospaced)).foregroundStyle(Mint.muted).textSelection(.enabled)
                    .padding(10).frame(maxWidth: .infinity, alignment: .leading)
                    .background(Mint.surface, in: RoundedRectangle(cornerRadius: Mint.smallRadius))
            }
        }
    }

    @ViewBuilder private var label: some View {
        if case .thought(let ms) = entry.kind {
            Text(ms.map { "Thought for \(max(1, $0 / 1000)) s" } ?? "Thought").font(.body(12)).italic().foregroundStyle(Mint.dim)
        } else {
            let line = entry.text.split(separator: "\n", maxSplits: 1).first.map(String.init) ?? ""
            let (name, rest) = line.firstIndex(of: ":").map { (String(line[..<$0]), line[line.index(after: $0)...].trimmingCharacters(in: .whitespaces)) } ?? (line, "")
            HStack(spacing: 8) {
                Text(String(name.prefix(1)).uppercased()).font(.system(size: 10, weight: .semibold, design: .monospaced)).foregroundStyle(Mint.muted)
                    .frame(width: 18, height: 18).background(Mint.raised, in: RoundedRectangle(cornerRadius: 4))
                Text(name).font(.body(12)).foregroundStyle(Mint.muted)
                Text(rest).font(.system(size: 11.5, weight: .medium, design: .monospaced)).foregroundStyle(Mint.ink).lineLimit(1).truncationMode(.middle)
            }
        }
    }
}
