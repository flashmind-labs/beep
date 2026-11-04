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
                        ForEach(sessions.filter { query.isEmpty || ($0.title ?? $0.agent).localizedCaseInsensitiveContains(query) || $0.machineName.localizedCaseInsensitiveContains(query) }, id: \.id) { session in
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
        .task(id: model.connected) { if model.connected { await load() } }
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
        WorkspacePage(title: "Agent session", back: back) {
            if let session = view?.session, !session.external, session.status != .ended {
                Button("Stop") { perform { try await model.workspace.client().steerCoding(machineId: session.machineId, agent: session.agent, session: session.id, steer: .cancel) } }
                    .buttonStyle(TextAction(color: Mint.error)).disabled(busy)
            }
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceError(text: error, retry: load)
                    WorkspaceError(text: actionError)
                    if let view {
                        HStack { WorkspaceHeading(title: view.session.title ?? agentName(agent)); WorkspaceBadge(text: view.session.status.label, color: view.session.status.color) }
                        WorkspaceFacts(rows: [("Agent", agentName(view.session.agent)), ("Machine", view.session.machineName),
                                              ("Turns", "\(view.turns)"), ("Folder", view.session.cwd)])
                        if let usage {
                            WorkspaceFacts(rows: [("Tokens", usage.tokens.formatted()), ("Cost", usage.costUsd.map { String(format: "$%.2f", $0) } ?? "Not reported")])
                        }
                        if let question = view.session.question {
                            WorkspaceBox {
                                VStack(alignment: .leading, spacing: 10) {
                                    WorkspaceHeading(title: question.text)
                                    if let detail = question.detail { Text(detail).font(.system(size: 12, design: .monospaced)).textSelection(.enabled) }
                                    ForEach(question.options, id: \.id) { option in
                                        Button(option.name) {
                                            perform { try await model.workspace.client().answerCoding(machineId: view.session.machineId, agent: view.session.agent, session: view.session.id, question: question.id, reply: option.id) }
                                        }.buttonStyle(PillButton(fill: option.allow ? Mint.accent : Mint.surface)).disabled(busy)
                                    }
                                }.padding(12)
                            }
                        }
                        if !view.plan.isEmpty {
                            WorkspaceHeading(title: "Plan")
                            WorkspaceBox {
                                ForEach(Array(view.plan.enumerated()), id: \.offset) { _, step in
                                    WorkspaceRow(icon: step.status == .completed ? "checkmark.circle" : "circle", title: step.content)
                                }
                            }
                        }
                        WorkspaceHeading(title: "Session log")
                        if view.earlier > 0 { Text("\(view.earlier) earlier entries omitted").font(.body(11)).foregroundStyle(Mint.muted) }
                        ForEach(Array(view.entries.enumerated()), id: \.offset) { _, entry in
                            transcript(entry)
                        }
                        if let problem = view.error { WorkspaceError(text: problem) }
                        if !view.files.isEmpty {
                            WorkspaceHeading(title: "Changes")
                            WorkspaceBox {
                                ForEach(view.files, id: \.self) { file in
                                    WorkspaceRow(icon: "doc", title: file) {
                                        perform {
                                            diff = try await model.workspace.client().sessionDiff(machineId: view.session.machineId, root: view.session.cwd)
                                            showingDiff = true
                                        }
                                    }
                                }
                            }
                        }
                        if view.session.external {
                            Button("Resume session") {
                                perform {
                                    let resumed = try await model.workspace.client().resumeCoding(machineId: view.session.machineId, agent: view.session.agent, session: view.session.id)
                                    self.view = try await model.workspace.client().codingSession(machineId: resumed.session.machineId, agent: resumed.session.agent, session: resumed.session.id)
                                }
                            }.buttonStyle(PrimaryButton()).disabled(busy)
                        }
                    } else if error.isEmpty { WorkspaceLoading() }
                }.padding(16)
            }.refreshable { await load() }
            if let session = view?.session, !session.external, session.status != .ended {
                HStack(spacing: 8) {
                    TextField("Message agent", text: $draft, axis: .vertical).lineLimit(1...5).workspaceField()
                    Button("Send") {
                        let text = draft
                        perform {
                            try await model.workspace.client().steerCoding(machineId: session.machineId, agent: session.agent, session: session.id, steer: .prompt(text: text))
                            draft = ""
                        }
                    }.buttonStyle(PillButton(fill: Mint.accent)).disabled(busy || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }.padding(12).background(Mint.surface).overlay(alignment: .top) { Divider().overlay(Mint.soft) }
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
    @ViewBuilder private func transcript(_ entry: AgentEntry) -> some View {
        switch entry.kind {
        case .tool, .thought:
            DisclosureGroup(entry.kind == .tool ? "Tool call" : "Thoughts") {
                Text(entry.text).font(.system(size: 11, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            }.font(.body(12)).tint(Mint.muted).padding(12).card()
        default:
            WorkspaceBox {
                VStack(alignment: .leading, spacing: 6) {
                    Text(entry.kind == .user ? "You" : entry.kind == .question ? "Permission" : agentName(agent)).font(.body(11)).foregroundStyle(Mint.muted)
                    Text(entry.text).font(.body(13)).textSelection(.enabled)
                }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
            }
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
