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
    @Environment(\.tabRoot) private var tabRoot
    @Binding var path: [Route]
    @State private var terminals: [Terminal] = []
    @State private var error = ""
    @State private var loaded = false
    @State private var adding: NewTerminalTarget?

    private struct MachineGroup: Identifiable {
        let id: String
        let name: String
        let online: Bool
        let terminals: [Terminal]
    }

    private var groups: [MachineGroup] {
        let known = model.workspace.machines
        var ids = known.map(\.id)
        for terminal in terminals where !ids.contains(terminal.machineId) { ids.append(terminal.machineId) }
        return ids.compactMap { id in
            let mine = terminals.filter { $0.machineId == id }
            let machine = known.first { $0.id == id }
            let online = machine?.online ?? false
            guard online || !mine.isEmpty else { return nil }
            return MachineGroup(id: id, name: machine?.name ?? mine.first?.machineName ?? id, online: online, terminals: mine)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Terminals", back: tabRoot ? nil : { path.removeLast() }) {
                CCIconButton(system: "plus", label: "New terminal") { adding = NewTerminalTarget(machine: "") }
            }
            CCBody {
                WorkspaceError(text: error, retry: load)
                if !loaded { WorkspaceLoading() }
                else if groups.isEmpty { CCEmpty(title: "No terminals", detail: "Open a shell on a linked machine.") }
                ForEach(groups) { group in
                    CCSection(title: group.name, count: group.terminals.count) {
                        if group.online { CCButton(title: "New") { adding = NewTerminalTarget(machine: group.id) } }
                    } content: {
                        CCPanel {
                            ForEach(group.terminals, id: \.id) { terminal in
                                CCRow(title: terminal.name.isEmpty ? "Shell" : terminal.name, subtitle: subtitle(terminal),
                                      chevron: group.online, action: group.online ? { path.append(.terminal(terminal.id)) } : nil) {
                                    CCKindTile(text: ">_")
                                } right: {
                                    CCTag(text: terminal.state.label, kind: terminal.state == .running ? .ok : .plain)
                                }
                            }
                        }
                    }.opacity(group.online ? 1 : 0.5)
                }
            }.refreshable { await load() }
        }
        .background(Mint.page)
        .task { await load() }
        .sheet(item: $adding) { target in
            NewTerminalView(machine: target.machine) { terminal in adding = nil; path.append(.terminal(terminal.id)) }
        }
    }

    private func subtitle(_ terminal: Terminal) -> String {
        let command = terminal.command.isEmpty ? "" : " · " + terminal.command
        return (terminal.cwd.isEmpty ? terminal.project : terminal.cwd) + command
    }

    private func load() async {
        await model.workspace.refresh()
        do { terminals = try await model.workspace.client().allTerminals(); error = "" }
        catch { self.error = describe(error) }
        loaded = true
    }
}

private struct NewTerminalTarget: Identifiable {
    let machine: String
    var id: String { machine }
}

private struct NewTerminalView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let created: (Terminal) -> Void
    @State private var machine: String
    @State private var project = ""
    @State private var folder = ""
    @State private var name = ""
    @State private var command = ""
    @State private var error = ""
    @State private var busy = false

    init(machine: String, created: @escaping (Terminal) -> Void) {
        self.created = created
        _machine = State(initialValue: machine)
    }

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "New terminal", back: { dismiss() })
            CCBody {
                WorkspaceError(text: error)
                Text("Machine").font(.body(12)).foregroundStyle(Mint.muted)
                HStack(spacing: 8) {
                    ForEach(model.workspace.machines.filter(\.online), id: \.id) { item in
                        CCButton(title: item.name, style: item.id == machine ? .primary : .plain) { machine = item.id }
                    }
                }
                Text("Project").font(.body(12)).foregroundStyle(Mint.muted)
                Picker("Project", selection: $project) {
                    Text("No project").tag("")
                    ForEach(Array(Set(model.workspace.projects.map(\.project))).sorted(), id: \.self) { Text($0).tag($0) }
                }.tint(Mint.ink)
                TextField("Folder", text: $folder).font(.mono(13)).workspaceField()
                TextField("Empty opens a shell", text: $command).font(.mono(13)).workspaceField()
                TextField("Name", text: $name).workspaceField()
                CCButton(title: busy ? "Opening..." : "Open", style: .primary, big: true) { Task { await create() } }
                    .disabled(busy || machine.isEmpty)
            }
        }
        .background(Mint.page)
        .task { await model.workspace.refresh() }
        .onChange(of: project) { chooseFolder() }
        .onChange(of: machine) { chooseFolder() }
    }
    private func chooseFolder() { folder = model.workspace.projects.first { $0.project == project && $0.machineId == machine }?.folder ?? folder }
    private func create() async {
        busy = true; defer { busy = false }
        do {
            let config = CommandStart(project: project, machineId: machine, name: name, command: command, cwd: folder)
            created(try await model.workspace.client().startCommand(start: config))
        } catch { self.error = describe(error) }
    }
}

/// How the attached screen stands: the header bar and the key bar follow it.
private enum TerminalPhase: Equatable {
    case connecting, attached, viewOnly, ended(String), disconnected
}

struct TerminalView: View {
    @Environment(AppModel.self) private var model
    let id: String
    let back: () -> Void
    @State private var terminal: Terminal?
    @State private var emulator = TerminalEmulatorModel()
    @State private var connection: TerminalConnection?
    @State private var phase = TerminalPhase.connecting
    @State private var writer = TerminalWriter()
    @State private var error = ""
    @State private var closing = false
    @State private var confirmingClose = false
    @State private var attempt = 0

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: terminal.flatMap { $0.name.isEmpty ? nil : $0.name } ?? "Shell", back: back)
            HStack(spacing: 8) {
                CCDot(online: phase == .attached || phase == .viewOnly || phase == .connecting)
                Text(status).font(.body(12)).foregroundStyle(Mint.muted)
                Spacer()
                switch phase {
                case .ended: CCButton(title: "Close", action: back)
                case .disconnected: CCButton(title: "Retry") { attempt += 1 }
                case .viewOnly: CCButton(title: "Take control", action: takeControl)
                case .attached: CCButton(title: "View only") { phase = .viewOnly; emulator.setReadOnly(true) }
                case .connecting: EmptyView()
                }
                if !isEnded { CCButton(title: "Close…", style: .danger) { confirmingClose = true }.disabled(closing) }
            }
            .padding(.horizontal, 12).padding(.vertical, 6)
            .overlay(alignment: .bottom) { Rectangle().fill(Mint.soft).frame(height: 1) }
            if !error.isEmpty { WorkspaceError(text: error).padding(.horizontal, 12) }
            TerminalEmulator(model: emulator)
                .background(Color(TerminalEmulatorModel.background))
        }
        .background(Mint.page)
        .toolbar(.hidden, for: .navigationBar)
        .task(id: attempt) { await attach() }
        .onDisappear { drop(); writer.stop() }
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

    private var isEnded: Bool { if case .ended = phase { true } else { false } }

    private var status: String {
        switch phase {
        case .connecting: "Connecting"
        case .attached: "Attached · \(emulator.cols)×\(emulator.rows)"
        case .viewOnly: "View only"
        case .disconnected: "Disconnected"
        case .ended(let text): text
        }
    }

    /// Drops the stream and everything that could still write to it.
    private func drop() {
        connection = nil
        emulator.disconnect()
        emulator.setReadOnly(true)
    }

    private func takeControl() {
        if connection != nil { phase = .attached; emulator.setReadOnly(false) }
        else { attempt += 1 }
    }

    private func endedText(_ terminal: Terminal?) -> String {
        if case .exited(let code) = terminal?.state, let code { return "Ended · exit \(code)" }
        return "Ended"
    }

    private func attach() async {
        drop(); writer.stop(); error = ""
        phase = .connecting
        emulator.reset()
        do {
            let client = try model.workspace.client()
            let snapshot = try await client.terminalOutput(id: id)
            terminal = snapshot.terminal
            guard snapshot.terminal.state == .running else {
                emulator.feed(Array(snapshot.lines.joined(separator: "\r\n").utf8))
                phase = .ended(endedText(snapshot.terminal)); return
            }
            guard let (cols, rows) = await emulator.size() else { return }
            let stream = try await client.attachTerminal(id: id, cols: UInt16(clamping: cols), rows: UInt16(clamping: rows))
            connection = stream; phase = .attached
            writer.start(send: { try await stream.keys(text: $0) }, failed: { error = describe($0) })
            emulator.connect(attachedAt: (cols, rows), input: { writer.send($0) }, resize: { cols, rows in
                Task { try? await stream.resize(cols: UInt16(clamping: cols), rows: UInt16(clamping: rows)) }
            })
            emulator.setReadOnly(false)
            while !Task.isCancelled, let frame = try await stream.next() {
                if frame.ended {
                    drop(); writer.stop()
                    phase = .ended("Ended")
                    if let latest = try? await client.terminalOutput(id: id).terminal { terminal = latest; phase = .ended(endedText(latest)) }
                    return
                }
                if frame.viewOnly {
                    drop(); writer.stop(); phase = .viewOnly; return
                }
                emulator.feed(Array(frame.data))
            }
            if !Task.isCancelled { drop(); writer.stop(); phase = .disconnected }
        } catch {
            if !Task.isCancelled { drop(); writer.stop(); phase = .disconnected; self.error = describe(error) }
        }
    }
}
