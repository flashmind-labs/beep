import SwiftUI
import UIKit

// A research is a chat with its lead agent. The chat holds the plan and the result; this file draws the rest:
// its status in the chat, the workspace (paper, notes, sources, team) and the list of every research.

/// How often a research is asked for news while its team works, and otherwise.
private let pollBusy: Duration = .milliseconds(2500)
private let pollIdle: Duration = .seconds(10)
private let workingStatuses: Set<ResearchStatus> = [.researching, .reviewing, .writing]
private var scoutColors: [Color] { [Mint.brand, Mint.ok, Mint.warn, Mint.error, Mint.muted] }

/// A research as last fetched, without its log, and every log entry so far.
struct ResearchLive: Equatable {
    var research: Research
    var entries: [ResearchEntry]

    var status: ResearchStatus { research.status }
    var running: Bool { workingStatuses.contains(research.status) }
    var round: Int { max(1, Int(research.round)) }

    func agent(_ id: String) -> ResearchAgent {
        research.agents.first { $0.id == id } ?? ResearchAgent(id: id, name: id, role: "", model: "", round: 0)
    }
}

/// Follows a research chat while its view is up: its status, team and log, fetched again every
/// few seconds while the team works. When the status changes the chat is reloaded too, since the
/// lead posts the result there.
@MainActor @Observable
final class ResearchWatch {
    private(set) var live: ResearchLive?

    func follow(_ model: AppModel, _ id: String) async {
        live = nil
        var after: Int64 = 0
        var entries: [ResearchEntry] = []
        var status: ResearchStatus?
        while !Task.isCancelled {
            if let research = try? await model.research(id, after: after) {
                entries += research.entries
                if let last = research.entries.last { after = last.seq }
                if let status, status != research.status, model.chat.id == id { await model.loadHistory() }
                status = research.status
                var bare = research
                bare.entries = []
                live = ResearchLive(research: bare, entries: entries)
            }
            let busy = status == nil || status == .planning || status.map { workingStatuses.contains($0) } == true
            try? await Task.sleep(for: busy ? pollBusy : pollIdle)
        }
    }
}

// MARK: Words

private func minutes(_ from: Int64, _ to: Int64?) -> String {
    let end = to ?? Int64(Date().timeIntervalSince1970 * 1000)
    return "\(max(1, (end - from) / 60_000)) min"
}

private func kind(_ id: String) -> String { id.hasPrefix("scout-") ? "scout" : id }

private enum Doing { case run, ok, wait, none }

extension ResearchLive {
    /// A scout of an earlier round, done while a later one works.
    fileprivate func earlier(_ id: String) -> Bool { kind(id) == "scout" && max(1, Int(agent(id).round)) < round }

    /// When the newest round started: its scouts' first entry, else the research's start.
    fileprivate var roundStart: Int64 {
        guard round > 1 else { return research.startedAt }
        return entries.first { kind($0.agent) == "scout" && Int(agent($0.agent).round) == round }?.timestamp ?? research.startedAt
    }

    /// What agent `id` is doing, in a line, and how far along it is.
    fileprivate func doing(_ id: String) -> (String, Doing) {
        let mine = entries.filter { $0.agent == id }
        let notes = mine.filter { if case .note = $0.body { true } else { false } }.count
        let sources = research.sources.filter { $0.agent == id }.count
        let said = mine.filter { if case .said(let to, _) = $0.body { to != nil } else { false } }.count
        let summary = switch kind(id) {
        case "scout": "Done: \(notes) notes, \(sources) sources"
        case "critic": "Done: \(said) challenges"
        case "writer": "Done: the paper"
        default: "Posted the result"
        }
        if id == "lead" {
            if status == .planning { return ("Waiting for your go", .wait) }
            if status == .reviewing { return (round > 1 ? "Coordinating round \(round)" : "Coordinating", .run) }
            if running { return ("Watching the room", .run) }
            if status == .done { return (summary, .ok) }
            return ("", .none)
        }
        if status == .done { return (summary, .ok) }
        if !running { return (mine.isEmpty ? "" : String(summary.trimmingPrefix("Done: ")), .none) }
        if earlier(id) { return (summary, .ok) }
        let active: [String] = switch status {
        case .researching: ["scout"]
        case .reviewing: ["critic", "scout"]
        default: ["writer", "critic"]
        }
        if active.contains(kind(id)) {
            let step = mine.last { if case .step = $0.body { true } else { false } }
            if case .step(let label, _) = step?.body { return (label, .run) }
            return ("Starting", .run)
        }
        let order: [ResearchStatus] = [.researching, .reviewing, .writing]
        let starts: ResearchStatus = switch kind(id) { case "critic": .reviewing; case "writer": .writing; default: .researching }
        if (order.firstIndex(of: starts) ?? 0) > (order.firstIndex(of: status) ?? 0) {
            return (kind(id) == "critic" ? "Starts after the scouts" : "Starts after review", .none)
        }
        return (summary, .ok)
    }

    /// The research's state in a few words, for under its title.
    var headline: String {
        let working = research.agents.filter { $0.id != "lead" && doing($0.id).1 == .run }.count
        switch status {
        case .planning: return "Waiting for your go"
        case .researching: return "\(round > 1 ? "Round \(round)" : "Researching"), \(working) agent\(working == 1 ? "" : "s") working"
        case .reviewing: return "Reviewing the findings"
        case .writing: return "Writing the paper"
        case .done: return "Done in \(minutes(roundStart, research.finishedAt))"
        case .failed: return "The team failed"
        case .stopped: return "Stopped"
        case .interrupted: return "Interrupted by a server restart"
        }
    }

    /// What the team is doing, for the row over the composer while it works.
    fileprivate var progress: String {
        let took = minutes(roundStart, nil)
        switch status {
        case .researching:
            let scouts = research.agents.filter { kind($0.id) == "scout" && !earlier($0.id) }
            let busy = scouts.filter { doing($0.id).1 == .run }.count
            let line = "\(busy) of \(scouts.count) scout\(scouts.count == 1 ? "" : "s") working"
            return round > 1 ? "Round \(round): \(line), \(took)" : "Researching: \(line), \(took)"
        case .reviewing: return "The critic and the lead are reviewing, \(took)"
        default: return round > 1 ? "The writer is revising the paper, \(took)" : "The writer is drafting, \(took)"
        }
    }
}

/// The composer's hint in a research chat.
func researchHint(_ live: ResearchLive?) -> String {
    switch live?.status {
    case .planning: "Say go, or what to change"
    case .done: "Ask a follow-up"
    default: "Message the lead"
    }
}

private func agentColor(_ id: String) -> Color {
    switch id {
    case "lead": Mint.accent
    case "critic": Mint.brand
    case "writer": Mint.ink
    default: scoutColors[((Int(id.dropFirst("scout-".count)) ?? 1) - 1 + scoutColors.count * 8) % scoutColors.count]
    }
}

private func agentInitial(_ id: String) -> String {
    switch id {
    case "lead": "L"
    case "critic": "R"
    case "writer": "W"
    default: id.hasPrefix("scout-") ? String(id.dropFirst("scout-".count)) : "1"
    }
}

private struct AgentDot: View {
    let id: String
    var size: CGFloat = 22

    var body: some View {
        Text(agentInitial(id)).font(.system(size: size / 2, weight: .bold))
            .foregroundStyle(id == "writer" ? Mint.page : Mint.page)
            .frame(width: size, height: size)
            .background(Circle().fill(agentColor(id)))
    }
}

private struct DoingIcon: View {
    let doing: Doing

    var body: some View {
        switch doing {
        case .run: ProgressView().controlSize(.small)
        case .ok: Image(systemName: "checkmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.ok)
        case .wait: Circle().fill(Mint.accent).frame(width: 8, height: 8)
        case .none: EmptyView()
        }
    }
}

/// The research's state as the line under the chat's title.
struct ResearchHeadline: View {
    let live: ResearchLive

    var body: some View {
        HStack(spacing: 6) {
            if live.running {
                ProgressView().controlSize(.mini)
            } else if live.status == .planning {
                Circle().fill(Mint.accent).frame(width: 7, height: 7)
            } else if live.status == .done {
                Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(Mint.ok)
            }
            Text(live.headline).font(.body(12, .medium)).foregroundStyle(Mint.muted).lineLimit(1)
        }
    }
}

// MARK: In the chat

/// Under the lead's newest reply: Start while it waits for a go, the paper once it is written.
struct ResearchActions: View {
    let live: ResearchLive
    let go: () -> Void
    let paper: () -> Void
    let share: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            switch live.status {
            case .planning:
                Button("Start", action: go).buttonStyle(PillButton(fill: Mint.user))
            case .done:
                Button("Open the paper", action: paper).buttonStyle(PillButton())
                if !live.research.draft.isEmpty { Button("Share PDF", action: share).buttonStyle(PillButton()) }
            default:
                EmptyView()
            }
        }
        .padding(.top, 8)
    }
}

/// While the team works, what it is doing, over the composer. Tapping it opens the workspace.
struct ResearchProgressRow: View {
    let live: ResearchLive
    let open: () -> Void
    let stop: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: open) {
                HStack(spacing: 10) {
                    ProgressView().controlSize(.small)
                    Text(live.progress).font(.body(13)).foregroundStyle(Mint.ink).lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 12)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens the workspace")
            Button("Stop", action: stop).buttonStyle(TextAction(color: Mint.error))
        }
        .padding(.leading, 14).padding(.trailing, 4)
        .chunky(radius: 12)
        .padding(.horizontal, 14).padding(.top, 6)
    }
}

// MARK: Sharing the paper

/// Fetches a research's paper as a PDF for the share sheet, with a line to show while it waits.
@MainActor @Observable
final class PaperShare {
    var notice: String?
    var failed = false
    var file: SheetURL?

    func share(_ model: AppModel, _ id: String) {
        guard notice == nil || failed else { return }
        notice = "Typesetting the PDF"
        failed = false
        Task {
            do {
                file = SheetURL(url: try await model.researchPdf(id))
                notice = nil
            } catch {
                notice = describe(error)
                failed = true
                try? await Task.sleep(for: .seconds(8))
                if failed { notice = nil }
            }
        }
    }
}

/// A short line over the bottom of a screen, with a spinner while it waits on something.
struct PaperNotice: View {
    let text: String
    let failed: Bool

    var body: some View {
        HStack(spacing: 10) {
            if !failed { ProgressView().controlSize(.small).tint(Mint.page) }
            Text(text).font(.body(13, .medium)).foregroundStyle(failed ? .white : Mint.page)
        }
        .padding(.horizontal, Mint.pageInset).padding(.vertical, 12)
        .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(failed ? Mint.error : Mint.ink))
        .padding(.horizontal, Mint.pageInset).padding(.bottom, 24)
        .transition(.opacity.combined(with: .move(edge: .bottom)))
    }
}

extension View {
    /// Shows `share`'s notice and its share sheet over this view.
    func paperShare(_ share: PaperShare) -> some View {
        overlay(alignment: .bottom) {
            if let text = share.notice { PaperNotice(text: text, failed: share.failed) }
        }
        .animation(.snappy, value: share.notice)
        .sheet(item: Binding(get: { share.file }, set: { share.file = $0 })) { ActivitySheet(items: [$0.url]) }
    }
}

// MARK: The workspace

enum WorkspaceTab: String, Hashable, CaseIterable {
    case paper = "Paper", notes = "Notes", sources = "Sources", team = "Team"
}

/// One agent's thread, or the room where they all talk, opened from the Team tab.
private enum Opened: Equatable {
    case agent(String)
    case room
}

/// The team's work on research chat `id`: the paper, the notes, the sources and the team. The paper
/// and notes are selectable, and Quote in the selection menu sends the text to `quote` for the chat.
struct ResearchWorkspace: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let id: String
    let start: WorkspaceTab?
    let back: () -> Void
    let quote: (String) -> Void
    @State private var watch = ResearchWatch()
    @State private var chosen: WorkspaceTab?
    @State private var opened: Opened?
    @State private var share = PaperShare()

    var body: some View {
        let live = watch.live
        // Until one is picked, the tab follows the research: notes while it works, the paper once written.
        let tab = chosen ?? start ?? (live?.research.draft.isEmpty != false ? .notes : .paper)
        VStack(spacing: 0) {
            header(live)
            if opened == nil {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(WorkspaceTab.allCases, id: \.self) { t in
                            let count = live?.research.sources.count ?? 0
                            FilterChip(label: t == .sources && count > 0 ? "Sources \(count)" : t.rawValue, selected: t == tab) { chosen = t }
                        }
                    }
                    .padding(.horizontal, Mint.pageInset)
                }
                .padding(.bottom, 8)
            }
            if let live {
                switch opened {
                case .agent(let agent): AgentThread(live: live, id: agent)
                case .room: RoomView(live: live)
                case nil:
                    switch tab {
                    case .paper: PaperView(live: live, quote: quote)
                    case .notes: NotesView(live: live, quote: quote)
                    case .sources: SourcesView(live: live) { openURL($0) }
                    case .team: TeamView(live: live) { opened = .agent($0) } room: { opened = .room }
                    }
                }
            } else {
                ProgressView().padding(40)
                Spacer()
            }
        }
        .background(Mint.page)
        .paperShare(share)
        .task(id: id) { await watch.follow(model, id) }
    }

    private func header(_ live: ResearchLive?) -> some View {
        let agent: ResearchAgent? = if case .agent(let a) = opened { live?.agent(a) } else { nil }
        return HStack(spacing: 4) {
            BackArrow { opened == nil ? back() : (opened = nil) }
            VStack(alignment: .leading, spacing: 2) {
                Text(agent?.name ?? (opened == .room ? "Room" : live?.research.topic ?? "Research"))
                    .font(.body(16, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                if let agent {
                    Text([agent.role, agent.model].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                } else if opened == .room {
                    Text("Where the agents talk").font(.body(12)).foregroundStyle(Mint.muted)
                } else if let live {
                    ResearchHeadline(live: live)
                }
            }
            Spacer(minLength: 0)
            if opened == nil, live?.research.draft.isEmpty == false {
                Button { share.share(model, id) } label: {
                    Image(systemName: "square.and.arrow.up").font(.system(size: 18)).foregroundStyle(Mint.muted)
                        .frame(width: 44, height: 44).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Share PDF")
            }
        }
        .padding(.leading, 6).padding(.trailing, 18).frame(minHeight: 56)
    }
}

private struct EmptyLine: View {
    let text: String

    var body: some View {
        VStack {
            Text(text).font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                .multilineTextAlignment(.center).padding(.horizontal, 40).padding(.vertical, 60)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }
}

private func waitingLine(_ live: ResearchLive, _ none: String) -> String {
    live.status == .planning ? "The workspace fills in once the team starts." : none
}

private struct PaperView: View {
    let live: ResearchLive
    let quote: (String) -> Void

    var body: some View {
        let r = live.research
        if r.draft.isEmpty {
            EmptyLine(text: live.status == .planning ? "The workspace fills in once the team starts."
                      : live.running ? "The writer starts after the critic's review." : "No paper was written.")
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Written by \(live.agent("writer").name), reviewed by \(live.agent("critic").name). \(r.sources.count) sources.")
                        .font(.body(12)).foregroundStyle(Mint.muted)
                    QuotableText(markdown: r.draft, quote: quote)
                }
                .padding(.horizontal, 18).padding(.bottom, 32)
            }
        }
    }
}

private struct NotesView: View {
    let live: ResearchLive
    let quote: (String) -> Void

    var body: some View {
        let notes = live.entries.filter { if case .note = $0.body { true } else { false } }
        if notes.isEmpty {
            EmptyLine(text: waitingLine(live, "No notes yet."))
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    ForEach(notes, id: \.seq) { NoteRow(live: live, entry: $0, label: live.agent($0.agent).name, quote: quote) }
                }
                .padding(.horizontal, 18).padding(.bottom, 32)
            }
        }
    }
}

private struct NoteRow: View {
    let live: ResearchLive
    let entry: ResearchEntry
    let label: String
    var quote: ((String) -> Void)?

    var body: some View {
        // The scout cites its sources in the text itself.
        if case .note(let text, _) = entry.body {
            HStack(alignment: .top, spacing: 12) {
                Rectangle().fill(agentColor(entry.agent)).frame(width: 3)
                VStack(alignment: .leading, spacing: 2) {
                    Text(label).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                    if let quote {
                        QuotableText(markdown: text, size: 14, quote: quote)
                    } else {
                        Text(text).font(.body(14)).foregroundStyle(Mint.ink).lineSpacing(4)
                    }
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        }
    }
}

private struct SourcesView: View {
    let live: ResearchLive
    let open: (URL) -> Void

    var body: some View {
        let sources = live.research.sources
        if sources.isEmpty {
            EmptyLine(text: waitingLine(live, "No sources yet."))
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(sources, id: \.number) { s in
                        Button { if let url = URL(string: s.url) { open(url) } } label: {
                            HStack(alignment: .top, spacing: 10) {
                                Text("\(s.number)").font(.body(14, .semibold)).foregroundStyle(Mint.brand).frame(width: 22, alignment: .leading)
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(s.title.isEmpty ? s.url : s.title).font(.body(14, .medium)).foregroundStyle(Mint.brand)
                                        .multilineTextAlignment(.leading)
                                    Text("\(host(s.url)), kept by \(live.agent(s.agent).name)").font(.body(12)).foregroundStyle(Mint.muted)
                                    if !s.quote.isEmpty {
                                        Text("“\(s.quote)”").font(.body(13)).italic().foregroundStyle(Mint.ink).lineSpacing(3).lineLimit(4)
                                            .multilineTextAlignment(.leading)
                                    }
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(.vertical, 10)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        Rectangle().fill(Mint.soft).frame(height: 1)
                    }
                }
                .padding(.horizontal, 18).padding(.bottom, 32)
            }
        }
    }

    private func host(_ url: String) -> String {
        guard let host = URL(string: url)?.host() else { return url }
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }
}

private struct TeamView: View {
    let live: ResearchLive
    let agent: (String) -> Void
    let room: () -> Void

    var body: some View {
        let planning = live.status == .planning
        let agents = live.research.agents
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 8) {
                ForEach(agents.filter { $0.round <= 1 && $0.id != "critic" && $0.id != "writer" }, id: \.id) { a in
                    AgentRow(live: live, agent: a, enabled: !planning || a.id == "lead") { agent(a.id) }
                }
                if live.round > 1 {
                    ForEach(2...live.round, id: \.self) { n in
                        let of = agents.filter { Int($0.round) == n }
                        if !of.isEmpty {
                            Text("Round \(n)").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                                .padding(.top, 8).padding(.leading, 4)
                            ForEach(of, id: \.id) { a in AgentRow(live: live, agent: a, enabled: true) { agent(a.id) } }
                        }
                    }
                }
                ForEach(agents.filter { $0.id == "critic" || $0.id == "writer" }, id: \.id) { a in
                    AgentRow(live: live, agent: a, enabled: !planning) { agent(a.id) }
                }
                let said = live.entries.filter { if case .said = $0.body { true } else { false } }.count
                Button(action: room) {
                    HStack(spacing: 12) {
                        Text("#").font(.system(size: 12, weight: .bold)).foregroundStyle(Mint.ink)
                            .frame(width: 22, height: 22).background(Circle().fill(Mint.soft))
                        VStack(alignment: .leading, spacing: 1) {
                            Text("Room").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                            Text(planning ? "Opens when the team starts" : "\(said) message\(said == 1 ? "" : "s")")
                                .font(.body(12)).foregroundStyle(Mint.muted)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 10)
                    .card(radius: 12)
                }
                .buttonStyle(.plain)
                .disabled(planning)
                .opacity(planning ? 0.5 : 1)
            }
            .padding(.horizontal, Mint.pageInset).padding(.bottom, 32)
        }
    }
}

private struct AgentRow: View {
    let live: ResearchLive
    let agent: ResearchAgent
    let enabled: Bool
    let open: () -> Void

    var body: some View {
        let (now, state) = live.doing(agent.id)
        Button(action: open) {
            HStack(spacing: 12) {
                AgentDot(id: agent.id)
                VStack(alignment: .leading, spacing: 1) {
                    Text(agent.name).font(.body(14, .medium)).foregroundStyle(Mint.ink)
                    Text(now.isEmpty ? agent.role : now).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                    if !agent.model.isEmpty { Text(agent.model).font(.body(11)).foregroundStyle(Mint.dim) }
                }
                Spacer(minLength: 0)
                DoingIcon(doing: state)
            }
            .padding(.horizontal, 12).padding(.vertical, 10)
            .card(radius: 12)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityHint("Opens \(agent.name)'s thread")
    }
}

private struct SaidRow: View {
    let live: ResearchLive
    let entry: ResearchEntry

    var body: some View {
        if case .said(let to, let content) = entry.body {
            HStack(alignment: .top, spacing: 10) {
                AgentDot(id: entry.agent)
                VStack(alignment: .leading, spacing: 2) {
                    Text(live.agent(entry.agent).name + (to.map { " to " + live.agent($0).name } ?? ""))
                        .font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                    Text(content).font(.body(14)).foregroundStyle(Mint.ink).lineSpacing(4).textSelection(.enabled)
                }
            }
        }
    }
}

private struct StepRow: View {
    let label: String
    let success: Bool
    var running = false

    var body: some View {
        HStack(spacing: 8) {
            if running {
                ProgressView().controlSize(.mini)
            } else {
                Image(systemName: success ? "checkmark" : "xmark").font(.system(size: 11, weight: .bold))
                    .foregroundStyle(success ? Mint.ok : Mint.error)
            }
            Text(label).font(.body(13)).foregroundStyle(Mint.muted)
        }
        .padding(.leading, 32)
    }
}

private struct AgentThread: View {
    let live: ResearchLive
    let id: String

    var body: some View {
        let mine = live.entries.filter { entry in
            if entry.agent == id { return true }
            if case .said(let to, _) = entry.body { return to == id }
            return false
        }
        let (now, state) = live.doing(id)
        if mine.isEmpty, state != .run {
            EmptyLine(text: "Nothing yet.")
        } else {
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(mine, id: \.seq) { entry in
                            switch entry.body {
                            case .said: SaidRow(live: live, entry: entry)
                            case .step(let label, let success): if entry.agent == id { StepRow(label: label, success: success) }
                            case .note: NoteRow(live: live, entry: entry, label: "Note")
                            }
                        }
                        if state == .run { StepRow(label: now, success: true, running: true) }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 18).padding(.bottom, 32)
                }
                .onChange(of: mine.count, initial: true) { scroller.scrollTo("end", anchor: .bottom) }
            }
        }
    }
}

private struct RoomView: View {
    let live: ResearchLive

    var body: some View {
        let said = live.entries.filter { if case .said = $0.body { true } else { false } }
        if said.isEmpty {
            EmptyLine(text: "Nobody has said anything yet.")
        } else {
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 14) {
                        ForEach(said, id: \.seq) { SaidRow(live: live, entry: $0) }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 18).padding(.bottom, 32)
                }
                .onChange(of: said.count, initial: true) { scroller.scrollTo("end", anchor: .bottom) }
            }
        }
    }
}

// MARK: Every research

/// Every research on the account, newest first, from any device. The field on top starts a new one.
struct ResearchList: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    let open: (String) -> Void
    let start: (String) -> Void
    @State private var list: [ResearchSummary]?
    @State private var error = ""
    @State private var topic = ""

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Research", back: back)
            HStack(alignment: .bottom, spacing: 8) {
                TextField("", text: $topic, prompt: Text("What should the team research?").foregroundStyle(Mint.dim), axis: .vertical)
                    .font(.body(14)).foregroundStyle(Mint.ink).tint(Mint.ink)
                    .lineLimit(1...4)
                    .submitLabel(.go)
                    .onSubmit(send)
                    .padding(.vertical, 10).padding(.horizontal, Mint.pageInset)
                    .frame(minHeight: 50)
                    .chunky()
                Button(action: send) { Image(systemName: "arrow.up") }
                    .buttonStyle(ChunkyButton(fill: Mint.user, size: 50, radius: 18))
                    .accessibilityLabel("Research")
            }
            .padding(.horizontal, Mint.pageInset).padding(.bottom, 8)
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
            }
            if let list {
                if list.isEmpty {
                    EmptyLine(text: "No research yet.\nA team of agents digs into a question and writes it up as a paper.")
                } else {
                    ScrollView {
                        LazyVStack(spacing: 8) {
                            ForEach(list, id: \.sessionId) { r in
                                ResearchRow(research: r) { open(r.sessionId) }.padding(.trailing, 3).padding(.bottom, 3)
                            }
                        }
                        .padding(.horizontal, Mint.pageInset).padding(.top, 8).padding(.bottom, 24)
                    }
                    .refreshable { await load() }
                }
            } else if error.isEmpty {
                ProgressView().padding(40)
                Spacer()
            } else {
                Spacer()
            }
        }
        .background(Mint.page)
        .task(id: model.connected) {
            guard model.connected else { return }
            while !Task.isCancelled {
                await load()
                let busy = list?.contains { $0.status == .planning || workingStatuses.contains($0.status) } == true
                try? await Task.sleep(for: busy ? .seconds(5) : .seconds(30))
            }
        }
    }

    private func send() {
        let text = topic.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        topic = ""
        start(text)
    }

    private func load() async {
        do {
            list = try await model.listResearches()
            error = ""
        } catch {
            if list == nil { self.error = describe(error) }
        }
    }
}

private struct ResearchRow: View {
    let research: ResearchSummary
    let open: () -> Void

    var body: some View {
        let r = research
        let off = r.status == .stopped || r.status == .failed || r.status == .interrupted
        Button(action: open) {
            HStack(spacing: 12) {
                SwiftUI.Group {
                    if workingStatuses.contains(r.status) {
                        ProgressView().controlSize(.small)
                    } else if r.status == .planning {
                        Circle().fill(Mint.accent).frame(width: 8, height: 8)
                    } else if r.status == .done {
                        Image(systemName: "checkmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(Mint.ok)
                    } else {
                        Image(systemName: "magnifyingglass").font(.system(size: 14)).foregroundStyle(Mint.muted)
                    }
                }
                .frame(width: 20)
                VStack(alignment: .leading, spacing: 2) {
                    Text(r.topic).font(.body(14, .medium)).foregroundStyle(Mint.ink).lineLimit(3)
                        .multilineTextAlignment(.leading)
                    Text(line).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .card(radius: 12)
            .opacity(off ? 0.6 : 1)
        }
        .buttonStyle(.plain)
    }

    /// "Round 2 · Researching", "Done · 18 sources · Yesterday".
    private var line: String {
        let r = research
        let round = r.round > 1 ? "Round \(r.round) · " : ""
        let sources = r.sources > 0 ? " · \(r.sources) source\(r.sources == 1 ? "" : "s")" : ""
        switch r.status {
        case .planning: return "Waiting for your go"
        case .researching: return "\(round)Researching\(sources)"
        case .reviewing: return "\(round)Reviewing\(sources)"
        case .writing: return "\(round)Writing\(sources)"
        case .done: return "Done\(sources) · \(day(r.finishedAt ?? r.startedAt))"
        case .failed: return "Failed\(sources) · \(day(r.startedAt))"
        case .stopped: return "Stopped\(sources) · \(day(r.startedAt))"
        case .interrupted: return "Interrupted\(sources) · \(day(r.startedAt))"
        }
    }

    private func day(_ millis: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(millis) / 1000)
        let calendar = Calendar.current
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: Date())
        return date.formatted(sameYear ? .dateTime.day().month(.abbreviated) : .dateTime.day().month(.abbreviated).year())
    }
}

// MARK: Quoting a selection

/// Markdown as selectable text whose selection menu has Quote, which hands the selected text to
/// `quote`. SwiftUI's own selection has no way to add to that menu, so this is a text view.
struct QuotableText: UIViewRepresentable {
    let markdown: String
    var size: CGFloat = 15
    let quote: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> UITextView {
        let view = UITextView()
        view.isEditable = false
        view.isSelectable = true
        view.isScrollEnabled = false
        view.backgroundColor = .clear
        view.textContainerInset = .zero
        view.textContainer.lineFragmentPadding = 0
        view.delegate = context.coordinator
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return view
    }

    func updateUIView(_ view: UITextView, context: Context) {
        context.coordinator.quote = quote
        if context.coordinator.shown != markdown {
            context.coordinator.shown = markdown
            view.attributedText = Self.render(markdown, size: size)
        }
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        let width = proposal.width ?? UIScreen.main.bounds.width
        let fit = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        return CGSize(width: width, height: ceil(fit.height))
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var quote: (String) -> Void = { _ in }
        var shown = ""

        func textView(_ view: UITextView, editMenuForTextIn range: NSRange, suggestedActions: [UIMenuElement]) -> UIMenu? {
            guard range.length > 0, let text = (view.text as NSString?)?.substring(with: range) else { return nil }
            let action = UIAction(title: "Quote", image: UIImage(systemName: "quote.opening")) { [weak self, weak view] _ in
                view?.selectedRange = NSRange(location: range.location, length: 0)
                self?.quote(text)
            }
            return UIMenu(children: [action] + suggestedActions)
        }
    }

    private static func font(_ size: CGFloat, bold: Bool = false, italic: Bool = false, mono: Bool = false) -> UIFont {
        if mono { return .monospacedSystemFont(ofSize: size - 1, weight: .regular) }
        let base = Appearance.palette.body.flatMap { UIFont(name: $0, size: size) } ?? .systemFont(ofSize: size)
        var traits = UIFontDescriptor.SymbolicTraits()
        if bold { traits.insert(.traitBold) }
        if italic { traits.insert(.traitItalic) }
        guard !traits.isEmpty else { return base }
        return base.fontDescriptor.withSymbolicTraits(traits).map { UIFont(descriptor: $0, size: size) }
            ?? (bold ? .boldSystemFont(ofSize: size) : .italicSystemFont(ofSize: size))
    }

    /// Paragraphs, headings and bullets, with bold, italics, code and links inside them.
    static func render(_ markdown: String, size: CGFloat) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let ink = UIColor(Mint.ink)
        var first = true
        var inCode = false
        for raw in markdown.components(separatedBy: "\n") {
            var line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("```") { inCode.toggle(); continue }
            if line.isEmpty, !inCode { continue }
            var fontSize = size
            var bold = false
            var prefix = ""
            if !inCode {
                if let hashes = line.firstIndex(where: { $0 != "#" }), hashes != line.startIndex, line[hashes] == " " {
                    let level = line.distance(from: line.startIndex, to: hashes)
                    fontSize = size + CGFloat(max(0, 4 - level)) * 2
                    bold = true
                    line = String(line[hashes...]).trimmingCharacters(in: .whitespaces)
                } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                    prefix = "•  "
                    line = String(line.dropFirst(2))
                }
            }
            let style = NSMutableParagraphStyle()
            style.lineSpacing = fontSize * 0.4
            style.paragraphSpacingBefore = first ? 0 : (bold ? fontSize * 0.9 : fontSize * 0.6)
            if !prefix.isEmpty { style.headIndent = fontSize }
            first = false
            if out.length > 0 { out.append(NSAttributedString(string: "\n")) }
            let start = out.length
            out.append(NSAttributedString(string: prefix, attributes: [.font: font(fontSize), .foregroundColor: ink]))
            if inCode {
                out.append(NSAttributedString(string: raw, attributes: [.font: font(fontSize, mono: true), .foregroundColor: ink]))
            } else {
                out.append(inline(line, size: fontSize, bold: bold, ink: ink))
            }
            out.addAttribute(.paragraphStyle, value: style, range: NSRange(location: start, length: out.length - start))
        }
        return out
    }

    private static func inline(_ line: String, size: CGFloat, bold: Bool, ink: UIColor) -> NSAttributedString {
        guard let parsed = try? AttributedString(markdown: line, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))
        else { return NSAttributedString(string: line, attributes: [.font: font(size, bold: bold), .foregroundColor: ink]) }
        let out = NSMutableAttributedString()
        for run in parsed.runs {
            let intent = run.inlinePresentationIntent ?? []
            var attributes: [NSAttributedString.Key: Any] = [
                .font: font(size, bold: bold || intent.contains(.stronglyEmphasized), italic: intent.contains(.emphasized),
                            mono: intent.contains(.code)),
                .foregroundColor: ink,
            ]
            if let link = run.link {
                attributes[.link] = link
                attributes[.foregroundColor] = UIColor(Mint.brand)
            }
            out.append(NSAttributedString(string: String(parsed[run.range].characters), attributes: attributes))
        }
        return out
    }
}
