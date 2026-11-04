import Foundation
import Observation

@MainActor @Observable final class WorkspaceStore {
    let client: () throws -> BeepClient
    var projects: [ImportedProject] = []
    var projectAgents: [ProjectAgent] = []
    var machines: [Machine] = []
    var sessions: [MachineSession] = []
    var github = false
    var desktopIDs: Set<String> = []
    private var preferenceKey: String
    private var installAccount: String
    private var devicesKnown = false
    private var generation = 0
    private var refreshing = false
    var showAll: Bool {
        didSet { UserDefaults.standard.set(showAll, forKey: preferenceKey) }
    }
    var expanded: Bool { showAll || needsWorkspace }
    var error = ""
    var loaded = false

    init(account: String, client: @escaping () throws -> BeepClient) {
        self.client = client
        installAccount = account
        let savedAccount = UserDefaults.standard.string(forKey: "workspace.account." + account) ?? account
        let key = "workspace.showAll." + savedAccount
        preferenceKey = key
        showAll = UserDefaults.standard.bool(forKey: key)
    }

    var needsWorkspace: Bool {
        WorkspaceMode(hasProject: !projects.isEmpty || !projectAgents.isEmpty, githubConnected: github,
                      hasAdditionalMachine: devicesKnown && machines.contains { !desktopIDs.contains($0.id) }).expanded
    }

    func clear(account: String) {
        generation += 1; refreshing = false; devicesKnown = false
        installAccount = account
        preferenceKey = "workspace.showAll." + account
        showAll = UserDefaults.standard.bool(forKey: preferenceKey)
        projects = []; projectAgents = []; machines = []; sessions = []
        desktopIDs = []; github = false; loaded = false; error = ""
    }

    func refresh() async {
        guard !refreshing else { return }
        refreshing = true
        let stamp = generation
        defer { if generation == stamp { refreshing = false } }
        // Keep a successful source when another source is unavailable on an older server.
        async let imports = capture { try await self.client().importedProjects() }
        async let agents = capture { try await self.client().projectAgents() }
        async let hosts = capture { try await self.client().listMachines() }
        async let devices = capture { try await self.client().listDevices() }
        async let accounts = capture { try await self.client().githubAccounts() }
        async let apps = capture { try await self.client().listApps() }
        let importsResult = await imports
        let agentsResult = await agents
        let hostsResult = await hosts
        let devicesResult = await devices
        let saved = await accounts
        let connected = await apps
        guard generation == stamp, !Task.isCancelled else { return }
        var failures: [String] = []
        switch importsResult {
        case .success(let value): projects = value
        case .failure(let failure): failures.append("Projects: " + describe(failure))
        }
        switch agentsResult {
        case .success(let value): projectAgents = value
        case .failure(let failure): failures.append("Workspace: " + describe(failure))
        }
        switch hostsResult {
        case .success(let value): machines = value
        case .failure(let failure): failures.append("Machines: " + describe(failure))
        }
        switch devicesResult {
        case .success(let value):
            devicesKnown = true
            let desktops = value.filter { $0.kind == .desktop }
            desktopIDs = Set(desktops.map(\.id))
            if let primary = value.first(where: \.primary) {
                UserDefaults.standard.set(primary.id, forKey: "workspace.account." + installAccount)
                let key = "workspace.showAll." + primary.id
                if key != preferenceKey {
                    preferenceKey = key
                    showAll = UserDefaults.standard.bool(forKey: key)
                }
            }
        case .failure(let failure): failures.append("Devices: " + describe(failure))
        }
        switch (saved, connected) {
        case (.success(let accounts), .success(let apps)):
            github = !accounts.isEmpty || apps.contains { $0.slug == "github" && $0.connected }
        case (.success(let accounts), .failure): github = github || !accounts.isEmpty
        case (.failure, .success(let apps)): github = github || apps.contains { $0.slug == "github" && $0.connected }
        case (.failure(let failure), .failure): failures.append("GitHub: " + describe(failure))
        }
        error = failures.joined(separator: "\n")
        loaded = true
    }

    func refreshSessions(project: String? = nil) async throws -> [MachineSession] {
        let current = try await client().codingSessions()
        guard let project else { sessions = current; return current }
        let history = try await client().projectSessions(project: project)
        let folders = projects.filter { $0.project == project }
        let live = current.filter { session in
            folders.contains { $0.machineId == session.machineId && (session.cwd == $0.folder || session.cwd.hasPrefix($0.folder + "/")) }
        }
        let active = Set(live.map { $0.machineId + ":" + $0.id })
        return live + history.filter { !active.contains($0.machineId + ":" + $0.id) }
    }
}

private func capture<T>(_ work: () async throws -> T) async -> Result<T, Error> {
    do { return .success(try await work()) } catch { return .failure(error) }
}
