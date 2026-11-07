import SwiftUI

enum HomeTab: Hashable { case overview, prs, machines }

/// The first tab: machines, what waits on the user, what runs, open PRs and issues.
struct HomeView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let home: HomeModel
    @Binding var path: [Route]
    let openMachines: () -> Void
    @State private var tab = HomeTab.overview
    @State private var showPulls = true
    @State private var filter = "All"
    @State private var usage: MobileUsage?
    @State private var showUsage = false
    @State private var starting = false

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Home") {
                if let usage {
                    CCPill(action: { showUsage = true }) {
                        Text(String(format: "$%.2f", usage.costUsd)).fontWeight(.medium).foregroundStyle(Mint.ink)
                        Text("today")
                    }
                }
                CCIconButton(system: "bell", label: "Notifications", badge: home.badge) { path.append(.bell) }
                CCIconButton(system: "gearshape", label: "Settings") { path.append(.centerSettings) }
            }
            CCBody {
                strip
                CCSegmented(items: [.init(value: .overview, label: "Overview"),
                                    .init(value: .prs, label: "PRs and issues", count: pullCount),
                                    .init(value: .machines, label: "Machines")], selection: $tab)
                WorkspaceError(text: ([home.error] + (home.home?.errors ?? [])).filter { !$0.isEmpty }.joined(separator: "\n"))
                if home.home == nil { WorkspaceLoading() }
                else {
                    switch tab {
                    case .overview: overview
                    case .prs: pulls
                    case .machines: machineTable
                    }
                }
            }
            .refreshable { await load() }
        }
        .background(Mint.page).foregroundStyle(Mint.ink)
        .task {
            while !Task.isCancelled {
                await load()
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
            }
        }
        .sheet(isPresented: $showUsage) { if let usage { UsageSheet(usage: usage) } }
        .sheet(isPresented: $starting) {
            StartCodingAgentView { session in
                starting = false
                path.append(.agentSession(session.machineId, session.agent, session.id))
            }
        }
    }

    private func load() async {
        async let refreshed: Void = home.refresh()
        async let spend = try? model.workspace.client().codingUsage()
        await refreshed
        if let spend = await spend { usage = spend }
        AskCenter.shared.keepAsks(waiting: home.waiting)
    }

    private var pullCount: Int? { home.home.map { $0.pulls.count + $0.issues.count } }

    // MARK: Overview

    private var strip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(home.home?.machines ?? [], id: \.id) { machine in
                    Button(action: openMachines) {
                        CCStripCell(name: machine.name, detail: stripDetail(machine), online: machine.online)
                    }.buttonStyle(.plain)
                }
            }.padding(.horizontal, 12)
        }.padding(.horizontal, -12)
    }

    private func stripDetail(_ machine: MachineCell) -> String {
        guard machine.online else { return "offline" }
        let agents = machine.agents == 0 ? "idle" : "\(machine.agents) agent\(machine.agents == 1 ? "" : "s")"
        guard let free = machine.freeBytes else { return agents }
        return "\(agents) · \(bytes(free)) free"
    }

    @ViewBuilder private var overview: some View {
        if home.needs.isEmpty {
            CCPanel { CCEmpty(title: "Nothing needs you", detail: "Agents ask here when they need an answer.") }
        } else {
            CCSection("Needs you", count: home.needs.count) {
                CCPanel { ForEach(home.needs, id: \.key) { need in needRow(need) } }
            }
        }
        CCSection(title: "Running", count: home.home?.running.count, trailing: {
            Button("Start an agent") { starting = true }.buttonStyle(.plain)
        }) {
            let running = home.home?.running ?? []
            CCPanel {
                if running.isEmpty { CCEmpty(title: "Nothing running", detail: "Start an agent on one of your machines.") }
                ForEach(running, id: \.workspaceID) { session in
                    CCRow(title: session.title ?? agentName(session.agent), subtitle: place(session), chevron: true,
                          action: { open(session) }) { CCAgentTile(agent: session.agent) } right: {
                        if session.status == .asking { CCTag(text: "Asks", kind: .wait) }
                        else if let doing = session.doing { Text(doing).lineLimit(1) }
                        else { CCTag(text: session.status.label) }
                    }
                }
            }
        }
    }

    @ViewBuilder private func needRow(_ need: Need) -> some View {
        switch need {
        case .ask(let session):
            if let question = session.question {
                VStack(spacing: 0) {
                    CCRow(title: question.text, subtitle: place(session) + " · " + ago(session.lastActivity), chevron: true,
                          action: { open(session) }) { CCAgentTile(agent: session.agent) } right: { EmptyView() }
                    VStack(spacing: 6) {
                        ForEach(question.options, id: \.id) { option in
                            CCButton(title: option.name, style: option.id == question.options.first { $0.allow }?.id ? .primary : .plain,
                                     leading: true) { Task { await home.answer(session, option: option) } }
                        }
                    }.padding(.horizontal, 11).padding(.bottom, 10)
                }
            }
        case .pull(let item):
            let reason = pullReason(item)
            CCRow(title: "#\(item.number) \(item.title)", subtitle: "\(item.repo) · \(reason.text.lowercased())", chevron: true,
                  action: { if let url = URL(string: item.url) { openURL(url) } }) {
                CCKindTile(text: "PR", kind: reason.kind)
            } right: { CCTag(text: reason.text, kind: reason.kind) }
        case .conflict(let project, let path, let machines):
            CCRow(title: "\(path) changed on \(machines.count) machines", subtitle: "\(project) · \(machines.joined(separator: " and "))") {
                CCKindTile(text: "\u{21C4}", kind: .bad)
            } right: { CCTag(text: "Conflict", kind: .bad) }
        }
    }

    // MARK: PRs and issues

    @ViewBuilder private var pulls: some View {
        let data = home.home
        CCSegmented(items: [.init(value: true, label: "Pull requests", count: data?.pulls.count),
                            .init(value: false, label: "Issues", count: data?.issues.count)], selection: Binding(
            get: { showPulls }, set: { showPulls = $0; filter = "All" }))
        CCChips(items: showPulls ? ["All", "Waiting on you", "Yours", "Failing"]
                : ["All"] + Array(Set((data?.issues ?? []).flatMap(\.labels))).sorted(), selection: $filter)
        let shown = showPulls ? (data?.pulls ?? []).filter(matchesPull) : (data?.issues ?? []).filter(matchesIssue)
        CCPanel {
            if shown.isEmpty { CCEmpty(title: showPulls ? "No pull requests" : "No issues", detail: "Nothing open here.") }
            ForEach(shown, id: \.url) { item in
                let reason = pullReason(item)
                CCRow(title: "#\(item.number) \(item.title)", subtitle: showPulls ? "\(item.repo) · \(item.authored ? "yours" : "by others")" : ([item.repo] + item.labels).joined(separator: " · "),
                      chevron: true, action: { if let url = URL(string: item.url) { openURL(url) } }) {
                    CCKindTile(text: showPulls ? "PR" : "#", kind: showPulls ? reason.kind : .plain)
                } right: { if showPulls { CCTag(text: reason.text, kind: reason.kind) } }
            }
        }
        CCNote(text: "Across GitHub and Forgejo.")
    }

    private func matchesPull(_ item: MobileGitItem) -> Bool {
        switch filter {
        case "Waiting on you": item.reviewRequested
        case "Yours": item.authored
        case "Failing": item.checksFailing
        default: true
        }
    }

    private func matchesIssue(_ item: MobileGitItem) -> Bool { filter == "All" || item.labels.contains(filter) }

    private func pullReason(_ item: MobileGitItem) -> (text: String, kind: CCKind) {
        if item.changesRequested { return ("Changes requested", .bad) }
        if item.checksFailing { return ("Checks failing", .bad) }
        if item.reviewRequested { return ("Review", .wait) }
        if item.draft { return ("Draft", .plain) }
        return ("Open", .plain)
    }

    // MARK: Machines

    private var machineTable: some View {
        CCPanel {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    Text("Machine").gridColumnAlignment(.leading); Text("Agents"); Text("Disk free")
                    Text("Memory").gridColumnAlignment(.trailing)
                }
                .font(.body(11.5, .semibold)).foregroundStyle(Mint.muted).padding(.vertical, 7).padding(.horizontal, 11)
                ForEach(home.home?.machines ?? [], id: \.id) { machine in
                    Divider().overlay(Mint.soft).gridCellUnsizedAxes(.horizontal)
                    GridRow {
                        HStack(spacing: 6) { CCDot(online: machine.online); Text(machine.name).fontWeight(.medium).lineLimit(1) }
                        Text(machine.online ? "\(machine.agents)" : "-")
                        Text(machine.online ? machine.freeBytes.map(bytes) ?? "-" : "-")
                        Text(machine.online ? memory(machine) : "-")
                    }
                    .font(.body(12)).padding(.vertical, 7).padding(.horizontal, 11)
                }
            }
        }
    }

    private func memory(_ machine: MachineCell) -> String {
        guard let used = machine.memUsed, let total = machine.memTotal else { return "-" }
        return "\(used / 1_000_000_000)/\(total / 1_000_000_000) GB"
    }

    // MARK: Helpers

    private func open(_ session: MachineSession) { path.append(.agentSession(session.machineId, session.agent, session.id)) }

    private func place(_ session: MachineSession) -> String {
        let folder = session.cwd.split(separator: "/").last.map(String.init) ?? session.cwd
        return [folder, session.machineName].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

func bytes(_ count: UInt64) -> String { ByteCountFormatter.string(fromByteCount: Int64(clamping: count), countStyle: .file) }

func ago(_ date: Date) -> String {
    let formatter = RelativeDateTimeFormatter()
    formatter.unitsStyle = .abbreviated
    return formatter.localizedString(for: date, relativeTo: .now)
}

/// Plan limits and today's spend.
struct UsageSheet: View {
    let usage: MobileUsage

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Usage").font(.body(16, .semibold))
                if !usage.limits.isEmpty {
                    CCSection("Plan limits") {
                        CCPanel {
                            ForEach(Array(usage.limits.enumerated()), id: \.offset) { _, limit in
                                HStack(spacing: 10) {
                                    VStack(alignment: .leading, spacing: 5) {
                                        Text("\(agentName(limit.agent)) \u{00B7} \(window(limit.minutes))").font(.body(13, .medium))
                                        CCBar(fraction: limit.usedPercent / 100, warn: limit.usedPercent >= 80)
                                    }
                                    Text(String(format: "%.0f%%", limit.usedPercent)).font(.body(11.5))
                                        .foregroundStyle(limit.usedPercent >= 80 ? Mint.warn : Mint.muted)
                                }
                                .padding(.horizontal, 11).padding(.vertical, 9).frame(minHeight: 46)
                                .overlay(alignment: .top) { Rectangle().fill(Mint.soft).frame(height: 1) }
                            }
                        }
                    }
                }
                CCSection("Spend") {
                    CCPanel {
                        pair("Today", String(format: "$%.2f", usage.costUsd))
                        pair("Tokens today", usage.tokens.formatted())
                    }
                }
            }
            .padding(.horizontal, 14).padding(.top, 18).padding(.bottom, 18)
        }
        .background(Mint.surface).foregroundStyle(Mint.ink)
        .presentationDetents([.medium, .large]).presentationBackground(Mint.surface)
    }

    private func pair(_ key: String, _ value: String) -> some View {
        HStack {
            Text(key).foregroundStyle(Mint.muted)
            Spacer()
            Text(value)
        }
        .font(.body(13)).padding(.horizontal, 11).padding(.vertical, 8)
        .overlay(alignment: .top) { Rectangle().fill(Mint.soft).frame(height: 1) }
    }

    private func window(_ minutes: UInt32) -> String {
        switch minutes {
        case 300: "5 hours"
        case 10080: "week"
        case let m where m % 60 == 0: "\(m / 60) hours"
        default: "\(minutes) min"
        }
    }
}
