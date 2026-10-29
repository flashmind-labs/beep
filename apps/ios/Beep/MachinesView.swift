import SwiftUI

/// The machines running beep-node on the account, for admins, as on Android. A machine shows
/// here once it has connected; linking one is the same as linking a computer, with the link
/// `beep-node link` prints pasted as a code.
struct MachinesView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    @State private var machines: [Machine]?
    @State private var open: String?
    @State private var error = ""

    var body: some View {
        let shown = machines?.first { $0.id == open }
        VStack(spacing: 0) {
            BackBar(title: shown?.name ?? "Machines", back: { if shown != nil { open = nil } else { back() } })
            if let shown {
                MachineDetail(machine: shown)
            } else {
                list
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await refresh() } }
    }

    private var list: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
                    if let machines {
                        if machines.isEmpty {
                            Text("No machines yet.\nRun beep-node link on a Mac or Linux box, then paste its link here.")
                                .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                                .multilineTextAlignment(.center)
                                .frame(maxWidth: .infinity).padding(.vertical, 40).padding(.horizontal, 12)
                        } else {
                            ForEach(machines, id: \.id) { machine in
                                Button { open = machine.id } label: { MachineCard(machine: machine) }
                                    .buttonStyle(.plain)
                            }
                            Text("Beep runs commands and coding agents on these when you ask. Each one runs beep-node and is linked like a computer.")
                                .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(4).padding(.horizontal, 4)
                        }
                    } else if error.isEmpty {
                        ProgressView().frame(maxWidth: .infinity).padding(40)
                    }
                }
                .padding(.horizontal, 16).padding(.vertical, 8)
            }
            .refreshable { await refresh() }
            ComputerCard(machine: true) { Task { await refresh() } }
                .padding(.horizontal, 16).padding(.bottom, 8)
        }
    }

    private func refresh() async {
        do {
            machines = try await model.machines()
            error = ""
        } catch {
            self.error = describe(error)
        }
    }
}

/// "Online", or when it was last seen.
private func seen(_ machine: Machine) -> String {
    machine.online ? "Online" : "Last seen " + machine.seenAt.formatted(.relative(presentation: .named))
}

private func system(_ env: MachineEnv) -> String {
    [env.osVersion.isEmpty ? env.os : env.osVersion, env.arch].filter { !$0.isEmpty }.joined(separator: ", ")
}

/// The CLIs signed in, as "gh as build-user, claude signed in".
private func signIns(_ env: MachineEnv) -> String {
    env.logins.filter(\.signedIn).map { login in
        login.account.map { "\(login.tool) as \($0)" } ?? "\(login.tool) signed in"
    }.joined(separator: ", ")
}

private struct StatusDot: View {
    let online: Bool
    var body: some View { Circle().fill(online ? Mint.ok : Mint.dim).frame(width: 8, height: 8) }
}

private struct MachineCard: View {
    let machine: Machine

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            TintIcon(system: "server.rack", tint: Mint.tints[machine.online ? 1 : 2])
            VStack(alignment: .leading, spacing: 3) {
                Text(machine.name).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                HStack(spacing: 6) {
                    StatusDot(online: machine.online)
                    Text(seen(machine)).font(.body(12)).foregroundStyle(Mint.muted)
                }
                if let env = machine.env {
                    Text(system(env)).font(.body(12)).foregroundStyle(Mint.muted)
                    if !env.tools.isEmpty {
                        HStack(spacing: 6) {
                            ForEach(env.tools.prefix(4), id: \.name) { tool in
                                Text(tool.name).font(.body(11, .medium)).foregroundStyle(Mint.ink)
                                    .padding(.horizontal, 8).padding(.vertical, 2)
                                    .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1))
                            }
                            if env.tools.count > 4 {
                                Text("+\(env.tools.count - 4)").font(.body(11)).foregroundStyle(Mint.muted)
                            }
                        }
                        .padding(.top, 2)
                    }
                    let signed = signIns(env)
                    if !signed.isEmpty { Text(signed).font(.body(12)).foregroundStyle(Mint.muted) }
                } else {
                    Text("No report yet").font(.body(12)).foregroundStyle(Mint.dim)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .card()
        .contentShape(Rectangle())
    }
}

/// Everything a machine reported, in sections.
private struct MachineDetail: View {
    let machine: Machine

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 14) {
                    TintIcon(system: "server.rack", tint: Mint.tints[machine.online ? 1 : 2], size: 52)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 6) {
                            StatusDot(online: machine.online)
                            Text(machine.online ? "Online" : "Offline, " + seen(machine).lowercased())
                                .font(.body(14, .medium)).foregroundStyle(Mint.ink)
                        }
                        Text("beep-node \(machine.version)").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                .padding(.horizontal, 4)
                if let env = machine.env {
                    section("System")
                    facts([("OS", env.osVersion.isEmpty ? env.os : env.osVersion), ("Chip", env.arch),
                           ("CPUs", "\(env.cpus)"), ("Memory", "\(env.memoryMb / 1024) GB"),
                           ("Host", env.hostname), ("Home", env.home)].filter { !$0.1.isEmpty }, mono: ["Home"])
                    if !env.logins.isEmpty {
                        section("Signed in")
                        facts(env.logins.map { ($0.tool, $0.signedIn ? ($0.account ?? "Signed in") : "Not signed in") })
                    }
                    if !env.tools.isEmpty {
                        section("Tools")
                        facts(env.tools.map { ($0.name, $0.version ?? "") })
                    }
                    if !env.avds.isEmpty {
                        section("Android emulators")
                        facts(env.avds.map { ($0, "") })
                    }
                } else {
                    Text("It has not reported what it has yet.").font(.body(13)).foregroundStyle(Mint.muted).padding(4)
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 8)
        }
    }

    private func section(_ title: String) -> some View {
        Text(title).font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.leading, 4).padding(.top, 8)
    }

    private func facts(_ rows: [(String, String)], mono: Set<String> = []) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(rows.indices, id: \.self) { i in
                let (label, value) = rows[i]
                HStack(alignment: .top, spacing: 0) {
                    if value.isEmpty {
                        Text(label).font(.body(13)).foregroundStyle(Mint.ink)
                    } else {
                        Text(label).font(.body(13)).foregroundStyle(Mint.muted).frame(width: 96, alignment: .leading)
                        Text(value).font(mono.contains(label) ? .system(size: 13, design: .monospaced) : .body(13))
                            .foregroundStyle(Mint.ink)
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .card()
    }
}
