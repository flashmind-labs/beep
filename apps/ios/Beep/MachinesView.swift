import SwiftUI

/// The machines running beep-node on the account, as on Android. A machine shows
/// here once it has connected; linking one is the same as linking a computer, with the link
/// `beep-node link` prints pasted as a code.
struct MachinesView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    @State private var machines: [Machine]?
    @State private var stats: [String: MachineStats] = [:]
    @State private var open: String?
    @State private var error = ""

    var body: some View {
        let shown = machines?.first { $0.id == open }
        VStack(spacing: 0) {
            BackBar(title: shown?.name ?? "Machines", back: { if shown != nil { open = nil } else { back() } })
            if let shown {
                MachineDetail(machine: shown, stats: stats[shown.id])
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
                                Button { open = machine.id } label: { MachineCard(machine: machine, stats: stats[machine.id]) }
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
            async let live = model.machineStats()
            let list = try await model.machines()
            stats = await live
            machines = list
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

/// Bytes in SI units, as the desktop shows them: "212 GB", "9.1 GB".
private func bytes(_ n: UInt64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"]
    var v = Double(n)
    var i = 0
    while v >= 1000 && i < units.count - 1 { v /= 1000; i += 1 }
    return (v >= 10 || i == 0 ? String(Int(v.rounded())) : String(format: "%.1f", v)) + " " + units[i]
}

/// GB from MiB, one decimal under 10: "9.1", "32".
private func gb(_ mb: UInt64) -> String {
    let n = Double(mb) / 1024
    return n >= 10 ? String(Int(n.rounded())) : String(format: "%.1f", n)
}

/// Free space on a volume, low under a tenth free.
private struct DiskFree {
    let text: String
    let low: Bool

    init(free: UInt64, total: UInt64) {
        text = "\(bytes(free)) free"
        low = Double(free) / Double(total) < 0.1
    }

    /// Where the machine keeps the user's files, or nil without a measured volume.
    init?(_ stats: MachineStats?) {
        guard let disk = stats?.disks.first, disk.totalBytes > 0 else { return nil }
        self.init(free: disk.freeBytes, total: disk.totalBytes)
    }
}

/// "9.1 / 32 GB", or nil without a memory reading.
private func memoryUse(_ stats: MachineStats?) -> String? {
    guard let stats, stats.memoryTotalMb > 0 else { return nil }
    return "\(gb(stats.memoryUsedMb)) / \(gb(stats.memoryTotalMb)) GB"
}

/// "1.25 on 8 cores", or nil when the machine gives no load.
private func loadText(_ load: Double?, cpus: UInt32?) -> String? {
    guard let load else { return nil }
    let cores = cpus.flatMap { $0 > 0 ? " on \($0) cores" : nil } ?? ""
    return String(format: "%.2f", load) + cores
}

/// Disk free and memory in use on one line, coral when the disk is low. Nothing when offline or unmeasured.
private struct StatsLine: View {
    let machine: Machine
    let stats: MachineStats?

    var body: some View {
        let disk = machine.online ? DiskFree(stats) : nil
        let memory = machine.online ? memoryUse(stats) : nil
        if disk != nil || memory != nil {
            HStack(spacing: 0) {
                if let disk {
                    Text(disk.text).font(.body(12, disk.low ? .medium : .regular))
                        .foregroundStyle(disk.low ? Mint.error : Mint.muted)
                }
                if disk != nil && memory != nil { Text(" · ").font(.body(12)).foregroundStyle(Mint.dim) }
                if let memory { Text("\(memory) memory").font(.body(12)).foregroundStyle(Mint.muted) }
            }
        }
    }
}

private struct StatusDot: View {
    let online: Bool
    var body: some View { Circle().fill(online ? Mint.ok : Mint.dim).frame(width: 8, height: 8) }
}

private struct MachineCard: View {
    let machine: Machine
    let stats: MachineStats?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            TintIcon(system: "server.rack", tint: Mint.tints[machine.online ? 1 : 2])
            VStack(alignment: .leading, spacing: 3) {
                Text(machine.name).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                HStack(spacing: 6) {
                    StatusDot(online: machine.online)
                    Text(seen(machine)).font(.body(12)).foregroundStyle(Mint.muted)
                }
                StatsLine(machine: machine, stats: stats)
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
    let stats: MachineStats?

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
                if machine.online, let stats { live(stats) }
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

    /// Memory and load, then each volume measured with how much is free.
    @ViewBuilder private func live(_ stats: MachineStats) -> some View {
        let memory = memoryUse(stats)
        let load = loadText(stats.load, cpus: machine.env?.cpus)
        if memory != nil || load != nil {
            section("Memory")
            facts([memory.map { ("In use", $0) }, load.map { ("Load", $0) }].compactMap { $0 })
        }
        let disks = stats.disks.filter { $0.totalBytes > 0 }
        if !disks.isEmpty {
            section("Disk")
            VStack(alignment: .leading, spacing: 8) {
                ForEach(disks, id: \.path) { disk in
                    let free = DiskFree(free: disk.freeBytes, total: disk.totalBytes)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(disk.path).font(.system(size: 13, design: .monospaced)).foregroundStyle(Mint.ink)
                        Text("\(free.text) of \(bytes(disk.totalBytes))").font(.body(12, free.low ? .medium : .regular))
                            .foregroundStyle(free.low ? Mint.error : Mint.muted)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14).padding(.vertical, 10)
            .card()
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
