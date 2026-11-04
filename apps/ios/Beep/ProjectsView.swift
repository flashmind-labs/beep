import SwiftUI

struct ProjectsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var query = ""
    @State private var adding = false

    private var names: [String] {
        Set(model.workspace.projects.map(\.project) + model.workspace.projectAgents.map(\.name))
            .filter { query.isEmpty || $0.localizedCaseInsensitiveContains(query) }.sorted()
    }

    var body: some View {
        WorkspacePage(title: "Projects", back: { path.removeLast() }) {
            Button("Add") { adding = true }.buttonStyle(TextAction())
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceSearch(prompt: "Filter projects", text: $query)
                    WorkspaceError(text: model.workspace.error, retry: { await model.workspace.refresh() })
                    WorkspaceHeading(title: "Repositories")
                    if !model.workspace.loaded { WorkspaceLoading() }
                    else if names.isEmpty {
                        WorkspaceEmpty(title: query.isEmpty ? "No projects yet" : "No matches", detail: "Add a repository on one of your machines.")
                    } else {
                        WorkspaceBox {
                            ForEach(names, id: \.self) { name in
                                let project = model.workspace.projects.first { $0.project == name }
                                WorkspaceRow(icon: "folder", title: name.split(separator: "/").last.map(String.init) ?? name,
                                             detail: [name, project?.machineName].compactMap { $0 }.joined(separator: " · ")) {
                                    path.append(.project(name))
                                }
                                if name != names.last { Divider().overlay(Mint.soft) }
                            }
                        }
                    }
                    WorkspaceHeading(title: "Workspace")
                    WorkspaceFacts(rows: [("Projects", "\(Set(model.workspace.projects.map(\.project) + model.workspace.projectAgents.map(\.name)).count)"),
                                          ("Open tasks", "\(model.workspace.projectAgents.reduce(0) { $0 + $1.openTasks })"),
                                          ("Working agents", "\(model.workspace.projectAgents.reduce(0) { $0 + $1.openWorkers })"),
                                          ("Machines", "\(model.workspace.machines.filter(\.online).count) online")])
                    WorkspaceBox {
                        WorkspaceRow(icon: "bubble.left.and.text.bubble.right", title: "Coding agents", detail: "Sessions across your machines") { path.append(.codingAgents) }
                        Divider().overlay(Mint.soft)
                        WorkspaceRow(icon: "server.rack", title: "Machines", badge: "\(model.workspace.machines.filter(\.online).count) online", color: Mint.ok) { path.append(.machines) }
                        Divider().overlay(Mint.soft)
                        WorkspaceRow(icon: "terminal", title: "Terminals") { path.append(.terminals) }
                    }
                }.padding(16)
            }.refreshable { await model.workspace.refresh() }
        }
        .task(id: model.connected) { if model.connected { await model.workspace.refresh() } }
        .sheet(isPresented: $adding) { AddProjectView { name in adding = false; path.append(.project(name)) } }
    }
}

private enum ProjectTab: String, CaseIterable {
    case overview = "Overview", pulls = "PRs", issues = "Issues", runs = "CI runs", agents = "Agents", terminals = "Terminals", activity = "Activity"
}

struct ProjectView: View {
    @Environment(AppModel.self) private var model
    let project: String
    @Binding var path: [Route]
    @State private var tab = ProjectTab.overview
    @State private var agent: ProjectAgent?
    @State private var branches: ProjectBranches?
    @State private var checkouts: [ProjectMachine] = []
    @State private var sync: ProjectSyncDetail?
    @State private var pulls: [ProjectPullRequest] = []
    @State private var issues: [ProjectIssue] = []
    @State private var runs: [MobileRun] = []
    @State private var sessions: [MachineSession] = []
    @State private var terminals: [Terminal] = []
    @State private var error = ""
    @State private var loaded = false
    @State private var query = ""
    @State private var thread: ThreadSelection?
    @State private var run: RunSelection?
    @State private var starting = false

    var body: some View {
        WorkspacePage(title: project.split(separator: "/").last.map(String.init) ?? project, back: { path.removeLast() }) {
            Button("Start agent") { starting = true }.buttonStyle(TextAction())
        } content: {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 0) {
                    ForEach(ProjectTab.allCases, id: \.self) { item in
                        Button { tab = item; query = "" } label: {
                            Text(item.rawValue).font(.body(12)).foregroundStyle(tab == item ? Mint.ink : Mint.muted)
                                .padding(.horizontal, 10).padding(.vertical, 12)
                                .overlay(alignment: .bottom) { Rectangle().fill(tab == item ? Mint.accent : .clear).frame(height: 2) }
                        }.buttonStyle(.plain).accessibilityAddTraits(tab == item ? .isSelected : [])
                    }
                }.padding(.horizontal, 8)
            }.overlay(alignment: .bottom) { Rectangle().fill(Mint.soft).frame(height: 1) }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceError(text: error, retry: load)
                    if !loaded { WorkspaceLoading() }
                    else {
                        if tab != .overview { WorkspaceSearch(prompt: "Filter \(tab.rawValue.lowercased())", text: $query) }
                        contents
                    }
                }.padding(16)
            }.refreshable { await load() }
        }
        .task(id: tab) { await load() }
        .sheet(item: $thread) { selected in
            if let agent { ThreadDetailView(project: project, projectID: agent.projectId, number: selected.number) }
        }
        .sheet(item: $run) { selected in
            if let agent { RunDetailView(projectID: agent.projectId, runID: selected.number) }
        }
        .sheet(isPresented: $starting) { StartCodingAgentView(project: project) { session in starting = false; open(session) } }
    }

    @ViewBuilder private var contents: some View {
        switch tab {
        case .overview:
            WorkspaceFacts(rows: [("Branch", branches?.defaultBranch ?? "Not available"), ("Checkout", model.workspace.projects.first { $0.project == project }?.machineName ?? "None"),
                                  ("Pull requests", "\(pulls.count) open"), ("Issues", "\(issues.count) open")])
            WorkspaceHeading(title: "Coding agents")
            sessionRows
            WorkspaceHeading(title: "Pull requests")
            pullRows
            if !runs.isEmpty { WorkspaceHeading(title: "Latest run"); runRows }
            WorkspaceHeading(title: "Checkouts")
            WorkspaceBox {
                ForEach(checkouts.filter { $0.folder != nil }, id: \.machineId) { checkout in
                    let state = sync?.machines.first { $0.sync.machineId == checkout.machineId }
                    WorkspaceRow(icon: "folder", title: checkout.machineName, detail: checkout.folder ?? "",
                                 badge: state.map { syncLabel($0.sync.status) } ?? (checkout.online ? "Online" : "Offline"),
                                 color: state?.sync.status == .inSync ? Mint.ok : Mint.muted)
                    if let failure = state?.sync.error { WorkspaceError(text: failure).padding(12) }
                }
            }
        case .pulls: pullRows
        case .issues:
            if issues.isEmpty { WorkspaceEmpty(title: "No open issues", detail: "Issues from this repository appear here.") }
            WorkspaceBox {
                ForEach(issues.filter { matches($0.title) }, id: \.number) { issue in
                    WorkspaceRow(icon: "exclamationmark.circle", title: issue.title,
                                 detail: "#\(issue.number) · " + issue.labels.joined(separator: ", "), badge: "Open") { thread = ThreadSelection(number: issue.number) }
                    if issue.number != issues.last?.number { Divider().overlay(Mint.soft) }
                }
            }
        case .runs: runRows
        case .agents: sessionRows
        case .terminals:
            TerminalRows(terminals: terminals.filter { matches($0.name) }) { path.append(.terminal($0.id)) }
            Button("New terminal") { path.append(.terminals) }.buttonStyle(PillButton())
        case .activity:
            WorkspaceHeading(title: "Recent activity")
            sessionRows
            runRows
        }
    }

    @ViewBuilder private var pullRows: some View {
        if pulls.isEmpty { WorkspaceEmpty(title: "No open pull requests", detail: "Pull requests from this repository appear here.") }
        else { WorkspaceBox {
            ForEach(pulls.filter { matches($0.title) }, id: \.number) { pull in
                WorkspaceRow(icon: "arrow.triangle.pull", title: pull.title,
                             detail: "#\(pull.number) · \(pull.author)", badge: pull.draft ? "Draft" : "Open", color: Mint.brand) { thread = ThreadSelection(number: pull.number) }
                if pull.number != pulls.last?.number { Divider().overlay(Mint.soft) }
            }
        } }
    }

    @ViewBuilder private var sessionRows: some View {
        if sessions.isEmpty { WorkspaceEmpty(title: "No coding sessions", detail: "Start an agent to work on this project.") }
        else { WorkspaceBox {
            ForEach(sessions.filter { matches($0.title ?? $0.agent) }, id: \.id) { session in
                CodingSessionRow(session: session) { open(session) }
                if session.id != sessions.last?.id { Divider().overlay(Mint.soft) }
            }
        } }
    }

    @ViewBuilder private var runRows: some View {
        if runs.isEmpty { WorkspaceEmpty(title: "No CI runs", detail: "Workflow runs appear when the repository has CI.") }
        else { WorkspaceBox {
            ForEach(runs.filter { matches($0.workflow + $0.title + $0.branch) }, id: \.id) { item in
                WorkspaceRow(icon: "checkmark.circle", title: item.workflow, detail: "#\(item.number) · \(item.branch)",
                             badge: item.state.label, color: item.state.color) { run = RunSelection(number: item.id) }
                if item.id != runs.last?.id { Divider().overlay(Mint.soft) }
            }
        } }
    }

    private func syncLabel(_ state: SyncStatus) -> String {
        switch state { case .offline: "Offline"; case .scanning: "Scanning"; case .syncing: "Syncing"; case .inSync: "Synced"; case .failed: "Sync failed" }
    }
    private func matches(_ text: String) -> Bool { query.isEmpty || text.localizedCaseInsensitiveContains(query) }
    private func open(_ session: MachineSession) { path.append(.agentSession(session.machineId, session.agent, session.id)) }

    private func load() async {
        error = ""
        do {
            if agent == nil {
                agent = model.workspace.projectAgents.first { $0.name == project }
                if agent == nil { agent = try await model.workspace.client().projectAgent(project: project) }
            }
            guard let agent else { return }
            switch tab {
            case .overview:
                // Each section reports its own failure without discarding the others.
                async let branchLoad: Void = fetch { branches = try await model.workspace.client().projectBranches(project: project) }
                async let checkoutLoad: Void = fetch { checkouts = try await model.workspace.client().projectMachines(project: project) }
                async let syncLoad: Void = fetch { sync = try await model.workspace.client().projectSync(project: project) }
                async let listLoad: Void = fetch { try await loadLists(agent.projectId) }
                async let sessionLoad: Void = fetch { sessions = try await model.workspace.refreshSessions(project: project) }
                async let runLoad: Void = fetch { try await loadRuns(agent.projectId) }
                await branchLoad; await checkoutLoad; await syncLoad; await listLoad
                await sessionLoad; await runLoad
            case .pulls, .issues: try await loadLists(agent.projectId)
            case .runs: try await loadRuns(agent.projectId)
            case .agents: sessions = try await model.workspace.refreshSessions(project: project)
            case .terminals: terminals = try await model.workspace.client().allTerminals().filter { $0.project == project }
            case .activity:
                await fetch { sessions = try await model.workspace.refreshSessions(project: project) }
                await fetch { try await loadRuns(agent.projectId) }
            }
        } catch { self.error = describe(error) }
        loaded = true
    }
    private func fetch(_ action: () async throws -> Void) async {
        do { try await action() } catch { self.error += (self.error.isEmpty ? "" : "\n") + describe(error) }
    }
    private func loadLists(_ id: String) async throws {
        let lists = try await model.workspace.client().projectLists(projectId: id, refresh: false)
        pulls = lists.pullRequests
        issues = lists.issues
        if let failed = lists.prsError ?? lists.issuesError { error = failed }
    }
    private func loadRuns(_ id: String) async throws {
        let result = try await model.workspace.client().projectRuns(projectId: id, refresh: false)
        runs = result.runs
        if let note = result.error { error += (error.isEmpty ? "" : "\n") + note }
    }
}

private struct ThreadSelection: Identifiable { let number: UInt64; var id: UInt64 { number } }
private struct RunSelection: Identifiable { let number: UInt64; var id: UInt64 { number } }

private struct AddProjectView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var added: (String) -> Void
    @State private var repo = ""
    @State private var machine = ""
    @State private var folder = ""
    @State private var saving = false
    @State private var error = ""

    var body: some View {
        WorkspacePage("Add project", back: { dismiss() }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    WorkspaceError(text: error)
                    Text("Repository").font(.body(12)).foregroundStyle(Mint.muted)
                    TextField("owner/repository or repository URL", text: $repo).workspaceField()
                    Text("Machine").font(.body(12)).foregroundStyle(Mint.muted)
                    Picker("Machine", selection: $machine) {
                        Text("Choose a machine").tag("")
                        ForEach(model.workspace.machines.filter(\.online), id: \.id) { Text($0.name).tag($0.id) }
                    }.tint(Mint.ink)
                    Text("Folder (optional)").font(.body(12)).foregroundStyle(Mint.muted)
                    TextField("Use the machine's default folder", text: $folder).workspaceField()
                    Button(saving ? "Adding..." : "Add project") { Task { await save() } }
                        .buttonStyle(PrimaryButton()).disabled(saving || repo.trimmingCharacters(in: .whitespaces).isEmpty || machine.isEmpty)
                }.padding(16)
            }
        }
    }
    private func save() async {
        saving = true; defer { saving = false }
        do {
            let result = try await model.workspace.client().importProject(repo: repo.trimmingCharacters(in: .whitespacesAndNewlines), machineId: machine, folder: folder.isEmpty ? nil : folder)
            await model.workspace.refresh()
            added(result.project)
        } catch { self.error = describe(error) }
    }
}
