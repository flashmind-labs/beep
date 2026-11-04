import SwiftUI

extension TerminalState {
    var label: String {
        switch self {
        case .running: "Running"; case .exited(let code): code.map { "Exited (\($0))" } ?? "Exited"
        case .stopped: "Stopped"; case .offline: "Offline"; case .gone: "Gone"
        }
    }
}

struct TerminalRows: View {
    let terminals: [Terminal]
    let open: (Terminal) -> Void
    var body: some View {
        if terminals.isEmpty { WorkspaceEmpty(title: "No terminals", detail: "Open a shell on a linked machine.") }
        else { WorkspaceBox {
            ForEach(terminals, id: \.id) { terminal in
                WorkspaceRow(icon: "terminal", title: terminal.name.isEmpty ? "Shell" : terminal.name,
                             detail: terminal.machineName + " · " + terminal.cwd, badge: terminal.state.label,
                             color: terminal.state == .running ? Mint.brand : Mint.muted) { open(terminal) }
                if terminal.id != terminals.last?.id { Divider().overlay(Mint.soft) }
            }
        } }
    }
}

struct TerminalsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var terminals: [Terminal] = []
    @State private var error = ""
    @State private var loaded = false
    @State private var adding = false
    @State private var query = ""

    var body: some View {
        WorkspacePage(title: "Terminals", back: { path.removeLast() }) {
            Button("New") { adding = true }.buttonStyle(TextAction())
        } content: {
            ScrollView {
                VStack(spacing: 14) {
                    WorkspaceSearch(prompt: "Filter terminals", text: $query)
                    WorkspaceError(text: error, retry: load)
                    if !loaded { WorkspaceLoading() }
                    else { TerminalRows(terminals: terminals.filter { query.isEmpty || ($0.name + $0.machineName + $0.project).localizedCaseInsensitiveContains(query) }) { path.append(.terminal($0.id)) } }
                }.padding(16)
            }.refreshable { await load() }
        }
        .task { await load() }
        .sheet(isPresented: $adding) { NewTerminalView { terminal in adding = false; path.append(.terminal(terminal.id)) } }
    }
    private func load() async {
        do { terminals = try await model.workspace.client().terminals(); error = "" }
        catch { self.error = describe(error) }
        loaded = true
    }
}

private struct NewTerminalView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let created: (Terminal) -> Void
    @State private var machine = ""
    @State private var project = ""
    @State private var folder = ""
    @State private var name = ""
    @State private var command = ""
    @State private var error = ""
    @State private var busy = false
    var body: some View {
        WorkspacePage("New terminal", back: { dismiss() }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    WorkspaceError(text: error)
                    Text("Machine").font(.body(12)).foregroundStyle(Mint.muted)
                    Picker("Machine", selection: $machine) {
                        Text("Choose a machine").tag("")
                        ForEach(model.workspace.machines.filter(\.online), id: \.id) { Text($0.name).tag($0.id) }
                    }.tint(Mint.ink)
                    Text("Project").font(.body(12)).foregroundStyle(Mint.muted)
                    Picker("Project", selection: $project) {
                        Text("No project").tag("")
                        ForEach(Array(Set(model.workspace.projects.map(\.project))).sorted(), id: \.self) { Text($0).tag($0) }
                    }.tint(Mint.ink)
                    TextField("Folder (optional)", text: $folder).workspaceField()
                    TextField("Name (optional)", text: $name).workspaceField()
                    TextField("Command (empty for shell)", text: $command).workspaceField()
                    Button(busy ? "Opening..." : "Open terminal") { Task { await create() } }
                        .buttonStyle(PrimaryButton()).disabled(busy || machine.isEmpty)
                }.padding(16)
            }
        }
        .task { await model.workspace.refresh() }
        .onChange(of: project) { chooseFolder() }
        .onChange(of: machine) { chooseFolder() }
    }
    private func chooseFolder() { folder = model.workspace.projects.first { $0.project == project && $0.machineId == machine }?.folder ?? "" }
    private func create() async {
        busy = true; defer { busy = false }
        do {
            let config = MobileTerminalStart(project: project, machineId: machine, name: name, command: command, cwd: folder)
            created(try await model.workspace.client().startTerminal(config: config))
        } catch { self.error = describe(error) }
    }
}

struct TerminalView: View {
    @Environment(AppModel.self) private var model
    let id: String
    let back: () -> Void
    @State private var terminal: Terminal?
    @State private var stream: MobileTerminal?
    @State private var transcript = TerminalTranscript()
    @State private var draft = ""
    @State private var error = ""
    @State private var attached = false
    @State private var closing = false
    @State private var confirmingClose = false
    @State private var follow = true

    var body: some View {
        WorkspacePage(title: terminal.flatMap { $0.name.isEmpty ? nil : $0.name } ?? "Shell", back: back) {
            Button("Close") { confirmingClose = true }.buttonStyle(TextAction(color: Mint.error)).disabled(closing)
        } content: {
            if let terminal {
                HStack {
                    Text(terminal.machineName).font(.body(12)).foregroundStyle(Mint.muted)
                    Spacer()
                    WorkspaceBadge(text: terminal.state.label, color: attached ? Mint.ok : Mint.muted)
                }.padding(.horizontal, 16).padding(.vertical, 8)
            }
            WorkspaceError(text: error).padding(.horizontal, 16)
            ScrollViewReader { proxy in
                ScrollView([.horizontal, .vertical]) {
                    Text(transcript.text).font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(14)
                    Color.clear.frame(height: 1).id("end")
                }
                .onChange(of: transcript.text) { if follow { proxy.scrollTo("end", anchor: .bottom) } }
                .onTapGesture { follow = false }
                .overlay(alignment: .bottomTrailing) {
                    if !follow { Button("Latest") { follow = true; proxy.scrollTo("end", anchor: .bottom) }.buttonStyle(PillButton()).padding(12) }
                }
            }
            if attached {
                HStack(spacing: 8) {
                    TextField("Command", text: $draft).workspaceField().onSubmit { send() }
                    Button("Run", action: send).buttonStyle(PillButton(fill: Mint.accent))
                    Button("Ctrl-C") { write("\u{03}") }.buttonStyle(PillButton()).accessibilityLabel("Interrupt command")
                }.padding(12).background(Mint.surface)
            }
        }
        .task { await attach() }
        .onDisappear {
            let current = stream; stream = nil; attached = false
            Task { do { try await current?.detach() } catch { self.error = describe(error) } }
        }
        .alert("Close terminal?", isPresented: $confirmingClose) {
            Button("Cancel", role: .cancel) {}
            Button("Close terminal", role: .destructive) {
                closing = true
                Task {
                    defer { closing = false }
                    do { try await model.workspace.client().closeTerminal(id: id); back() }
                    catch { self.error = describe(error) }
                }
            }
        } message: { Text("The command will stop and the terminal will be removed.") }
    }
    private func send() { let text = draft; draft = ""; write(text + "\n") }
    private func write(_ text: String) {
        Task { do { try await stream?.input(text: text) } catch { self.error = describe(error) } }
    }
    private func attach() async {
        do {
            let snapshot = try await model.workspace.client().terminalOutput(id: id)
            terminal = snapshot.terminal
            guard terminal?.state == .running else { transcript.append(Array(snapshot.lines.joined(separator: "\n").utf8)); return }
            let connection = try await model.workspace.client().attachTerminal(id: id)
            stream = connection; attached = true
            while !Task.isCancelled, let event = try await connection.next() {
                switch event {
                case .output(let data): transcript.append(Array(data))
                case .ended(let state): terminal?.state = state; attached = false; return
                case .viewOnly: transcript.append(Array(snapshot.lines.joined(separator: "\n").utf8)); attached = false; return
                }
            }
        } catch { if !Task.isCancelled { self.error = describe(error) } }
        attached = false
    }
}
