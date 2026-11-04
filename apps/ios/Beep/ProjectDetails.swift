import SwiftUI

struct ThreadDetailView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    let project: String
    let projectID: String
    let number: UInt64
    @State private var detail: ThreadDetail?
    @State private var diff: WorkDiff?
    @State private var error = ""
    @State private var diffError = ""
    @State private var starting = false

    var body: some View {
        WorkspacePage(title: "\(detail?.kind == .pullRequest ? "PR" : "Issue") #\(number)", back: { dismiss() }) {
            if let url = detail.flatMap({ URL(string: $0.url) }) {
                Button { openURL(url) } label: { Image(systemName: "arrow.up.right.square") }
                    .buttonStyle(TextAction()).accessibilityLabel("Open in browser")
            }
        } content: {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceError(text: error, retry: load)
                    if let detail {
                        WorkspaceHeading(title: detail.title)
                        WorkspaceFacts(rows: [("Status", state(detail.state)), ("Author", detail.author),
                                              ("Labels", detail.labels.isEmpty ? "None" : detail.labels.joined(separator: ", ")),
                                              ("Opened", detail.openedAt.formatted(date: .abbreviated, time: .omitted))])
                        if detail.kind == .pullRequest {
                            HStack {
                                if let checks = detail.checks { WorkspaceBadge(text: "Checks: " + checks.label, color: checks.color) }
                                WorkspaceBadge(text: review(detail.review), color: Mint.brand)
                            }
                        }
                        if !detail.body.isEmpty { prose(detail.body) }
                        if detail.kind == .pullRequest {
                            WorkspaceHeading(title: "Branch changes")
                            WorkspaceError(text: diffError)
                            if let diff { WorkDiffView(diff: diff) }
                        }
                        WorkspaceHeading(title: "Activity")
                        if detail.earlier > 0 { Text("\(detail.earlier) earlier comments omitted").font(.body(11)).foregroundStyle(Mint.muted) }
                        ForEach(Array(detail.comments.enumerated()), id: \.offset) { _, post in
                            WorkspaceBox {
                                VStack(alignment: .leading, spacing: 8) {
                                    HStack {
                                        Text(post.author).font(.body(12, .medium))
                                        Spacer()
                                        Text(post.at.formatted(date: .abbreviated, time: .shortened)).font(.body(10)).foregroundStyle(Mint.muted)
                                    }
                                    Text(post.body).font(.body(12)).textSelection(.enabled)
                                }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        Button("Start agent") { starting = true }.buttonStyle(PrimaryButton())
                    } else if error.isEmpty { WorkspaceLoading() }
                }.padding(16)
            }.refreshable { await load() }
        }
        .task { await load() }
        .sheet(isPresented: $starting) {
            StartCodingAgentView(project: project, prompt: "Work on \(project) #\(number): \(detail?.title ?? "")") { _ in starting = false }
        }
    }

    private func prose(_ text: String) -> some View {
        // Host content is displayed as text so it cannot execute markup.
        Text(text).font(.body(13)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(12).card()
    }
    private func load() async {
        error = ""; diffError = ""
        do {
            detail = try await model.workspace.client().threadDetail(projectId: projectID, number: number)
            if detail?.kind == .pullRequest {
                do {
                    let head = try await model.workspace.client().pullHead(projectId: projectID, number: number)
                    guard !head.fork else { diffError = "This pull request is from a fork. Open its diff in your browser."; return }
                    diff = try await model.workspace.client().branchDiff(project: project, branch: head.branch)
                } catch { diffError = describe(error) }
            }
        } catch { self.error = describe(error) }
    }
    private func state(_ state: ThreadState) -> String {
        switch state { case .open: "Open"; case .closed: "Closed"; case .merged: "Merged" }
    }
    private func review(_ review: GitReview) -> String {
        switch review { case .none: "No review decision"; case .approved: "Approved"; case .changesRequested: "Changes requested" }
    }
}

struct WorkDiffView: View {
    let diff: WorkDiff
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("\(diff.files.count) files").foregroundStyle(Mint.muted)
                Text("+\(diff.additions)").foregroundStyle(Mint.ok)
                Text("-\(diff.deletions)").foregroundStyle(Mint.error)
            }.font(.body(12))
            Text("Compared with \(diff.base)").font(.body(11)).foregroundStyle(Mint.muted)
            ForEach(diff.files, id: \.path) { file in
                DisclosureGroup {
                    if let reason = file.leftOut {
                        Text(omitted(reason)).font(.body(12)).foregroundStyle(Mint.muted).padding(12)
                    } else if file.status == .binary {
                        Text("Binary file").font(.body(12)).foregroundStyle(Mint.muted).padding(12)
                    } else {
                        ScrollView(.horizontal) {
                            VStack(alignment: .leading, spacing: 0) {
                                ForEach(Array(file.hunks.enumerated()), id: \.offset) { _, hunk in
                                    Text("@@ -\(hunk.oldStart),\(hunk.oldLines) +\(hunk.newStart),\(hunk.newLines) @@ \(hunk.section)")
                                        .foregroundStyle(Mint.muted).padding(.vertical, 4)
                                    ForEach(Array(hunk.lines.enumerated()), id: \.offset) { index, line in
                                        let number = lineNumber(hunk, index)
                                        Text(String(format: "%4d ", number) + marker(line.kind) + line.text)
                                            .foregroundStyle(line.kind == .added ? Mint.ok : line.kind == .removed ? Mint.error : Mint.ink)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .background(line.kind == .added ? Mint.ok.opacity(0.1) : line.kind == .removed ? Mint.error.opacity(0.1) : .clear)
                                    }
                                }
                            }.font(.system(size: 11, design: .monospaced)).textSelection(.enabled).padding(10)
                        }
                    }
                } label: {
                    HStack {
                        Text(file.path).font(.system(size: 11, design: .monospaced)).foregroundStyle(Mint.ink).lineLimit(2)
                        Spacer()
                        Text("+\(file.additions) -\(file.deletions)").font(.body(10)).foregroundStyle(Mint.muted)
                    }
                }.tint(Mint.muted).padding(10).card()
            }
            if diff.more > 0 { Text("\(diff.more) more files omitted").font(.body(11)).foregroundStyle(Mint.muted) }
        }
    }
    private func marker(_ kind: DiffLineKind) -> String {
        switch kind { case .context: " "; case .added: "+"; case .removed: "-" }
    }
    private func lineNumber(_ hunk: DiffHunk, _ index: Int) -> UInt32 {
        let line = hunk.lines[index]
        let previous = hunk.lines.prefix(index)
        return line.kind == .removed ? hunk.oldStart + UInt32(previous.filter { $0.kind != .added }.count)
            : hunk.newStart + UInt32(previous.filter { $0.kind != .removed }.count)
    }
    private func omitted(_ reason: DiffLeftOut) -> String {
        switch reason { case .tooLarge: "File too large to display"; case .lockfile: "Lockfile omitted"; case .overTotal: "Diff display limit reached" }
    }
}

struct RunDetailView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let projectID: String
    let runID: UInt64
    @State private var detail: MobileRunDetail?
    @State private var error = ""

    var body: some View {
        WorkspacePage("Run #\(detail?.run.number ?? runID)", back: { dismiss() }) {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    WorkspaceError(text: error, retry: load)
                    if let detail {
                        HStack { WorkspaceHeading(title: detail.run.workflow); WorkspaceBadge(text: detail.run.state.label, color: detail.run.state.color) }
                        WorkspaceFacts(rows: [("Branch", detail.run.branch), ("Trigger", detail.run.event),
                                              ("Started", detail.run.startedAt?.formatted(date: .abbreviated, time: .shortened) ?? "Queued"),
                                              ("Finished", detail.run.finishedAt?.formatted(date: .omitted, time: .shortened) ?? "Running")])
                        WorkspaceHeading(title: "Jobs")
                        WorkspaceBox {
                            ForEach(Array(detail.jobs.enumerated()), id: \.offset) { _, job in
                                DisclosureGroup {
                                    ForEach(Array(job.steps.enumerated()), id: \.offset) { _, step in
                                        HStack { Text(step.name).font(.body(12)); Spacer(); WorkspaceBadge(text: step.state.label, color: step.state.color) }.padding(10)
                                    }
                                } label: {
                                    HStack { Text(job.name).font(.body(13)); Spacer(); WorkspaceBadge(text: job.state.label, color: job.state.color) }
                                }.tint(Mint.muted).padding(12)
                            }
                        }
                        if let log = detail.log {
                            WorkspaceHeading(title: "Failure log")
                            Text(log).font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading).padding(12).card()
                        }
                        WorkspaceError(text: detail.error ?? "")
                    } else if error.isEmpty { WorkspaceLoading() }
                }.padding(16)
            }.refreshable { await load() }
        }.task { await load() }
    }
    private func load() async {
        do { detail = try await model.workspace.client().projectRun(projectId: projectID, runId: runID); error = "" }
        catch { self.error = describe(error) }
    }
}
