import SwiftUI

struct MemoriesView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    var latestRead = false
    @State private var memories: [Memory] = []
    @State private var reads: [MemoryRead] = []
    @State private var readId: String?
    /// What memories not about the user are about, by id, and the subject picked:
    /// nil for everything, `userSubject` or a `scopeKey`.
    @State private var scopes: [String: MemoryScope] = [:]
    @State private var about: String?
    @State private var loaded = false
    @State private var error = ""
    @State private var query = ""
    @State private var editing: Memory?
    @State private var text = ""
    @State private var confirmClear = false

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Memories", back: { path.removeAll() }) {
                if !memories.isEmpty {
                    Button("Clear all") { confirmClear = true }
                        .buttonStyle(TextAction(color: Mint.accent))
                        .font(.body(13))
                }
            }
            HStack(spacing: 12) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").font(.system(size: 15)).foregroundStyle(Mint.muted)
                    TextField("", text: $query, prompt: Text("Search memories").foregroundStyle(Mint.dim))
                        .font(.body(13)).foregroundStyle(Mint.ink)
                }
                .padding(.horizontal, 14)
                .frame(minHeight: 50)
                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.surface))
                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.soft, lineWidth: 1))
                Text("\(shown.count)").font(.body(12)).foregroundStyle(Mint.muted)
            }
            .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
            if !reads.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ReadChip(label: "All", count: nil, selected: read == nil) { readId = nil }
                        ForEach(reads.prefix(3), id: \.id) { r in
                            ReadChip(label: readLabel(r), count: r.memories.count, selected: r.id == readId) { readId = r.id }
                        }
                    }
                    .padding(.horizontal, Mint.pageInset)
                }
            }
            if !scopes.isEmpty { subjectChips }
            if let read {
                Text("What Beep learned or confirmed reading \(read.source) \(readDate(read)).")
                    .font(.body(12)).foregroundStyle(Mint.muted)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, Mint.pageInset).padding(.vertical, 6)
            }
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, Mint.pageInset)
            }
            if !loaded, memories.isEmpty {
                ProgressView().padding(40)
                Spacer()
            } else if shown.isEmpty {
                Text(memories.isEmpty
                     ? "Nothing remembered yet.\nBeep learns as you chat, or tell it \"remember that...\""
                     : "No matches.")
                    .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                    .multilineTextAlignment(.center)
                    .padding(40)
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        ForEach(groups, id: \.name) { group in
                            Text(group.name).font(.body(12, .semibold)).foregroundStyle(Mint.dim)
                                .padding(.leading, 4).padding(.top, 10).padding(.bottom, 2)
                            ForEach(group.items, id: \.id) { memory in
                                MemoryCard(memory: memory, about: scopes[memory.id]) {
                                    text = memory.content
                                    editing = memory
                                } forget: { delete(memory) }
                                .padding(.trailing, 3).padding(.bottom, 3)
                            }
                        }
                    }
                    .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
                }
                .refreshable { await load() }
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await load() } }
        .alert("Forget everything?", isPresented: $confirmClear) {
            Button("Cancel", role: .cancel) {}
            Button("Forget all", role: .destructive) {
                Task {
                    do { try await model.clearMemories() } catch { self.error = describe(error) }
                    await load()
                }
            }
        } message: {
            Text("Beep will forget all \(memories.count) memories. This cannot be undone.")
        }
        .alert("Edit memory", isPresented: Binding(get: { editing != nil }, set: { if !$0 { editing = nil } })) {
            TextField("Memory", text: $text, axis: .vertical)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                guard let memory = editing else { return }
                let content = text.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !content.isEmpty, content != memory.content else { return }
                Task {
                    do { try await model.updateMemory(memory.id, content) } catch { self.error = describe(error) }
                    await load()
                }
            }
        }
    }

    private var read: MemoryRead? { reads.first { $0.id == readId } }

    private func subject(_ memory: Memory) -> String { scopes[memory.id].map(scopeKey) ?? userSubject }

    @ViewBuilder private var subjectChips: some View {
        let counts = Dictionary(grouping: memories, by: subject).mapValues(\.count)
        let named = Dictionary(scopes.values.map { (scopeKey($0), $0) }, uniquingKeysWith: { first, _ in first })
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ReadChip(label: "Everything", count: nil, selected: about == nil) { about = nil }
                if let mine = counts[userSubject] {
                    ReadChip(label: "You", count: mine, selected: about == userSubject) { about = userSubject }
                }
                ForEach(named.sorted { scopeLabel($0.value).lowercased() < scopeLabel($1.value).lowercased() }, id: \.key) { entry in
                    ReadChip(label: scopeLabel(entry.value), count: counts[entry.key], selected: about == entry.key) { about = entry.key }
                }
            }
            .padding(.horizontal, Mint.pageInset)
        }
        .padding(.vertical, 4)
        if let about {
            let what = about == userSubject ? "you" : named[about].map(scopeSubject) ?? "this"
            Text("What Beep knows about \(what).")
                .font(.body(12)).foregroundStyle(Mint.muted)
                .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, Mint.pageInset).padding(.vertical, 6)
        }
    }

    private var shown: [Memory] {
        let ids = read.map { Set($0.memories) }
        let inReadAll = ids.map { ids in memories.filter { ids.contains($0.id) } } ?? memories
        let inRead = about.map { about in inReadAll.filter { subject($0) == about } } ?? inReadAll
        guard !query.isEmpty else { return inRead }
        return inRead.filter {
            $0.content.localizedCaseInsensitiveContains(query) || $0.category.localizedCaseInsensitiveContains(query)
        }
    }

    private struct Group {
        var name: String
        var items: [Memory]
    }

    private var groups: [Group] {
        Dictionary(grouping: shown) { $0.category.isEmpty ? "Other" : $0.category.capitalized }
            .map { Group(name: $0.key, items: $0.value) }
            .sorted { $0.name < $1.name }
    }

    private func load() async {
        do {
            memories = try await model.memories()
            reads = await model.memoryReads()
            scopes = await model.memoryScopes()
            if latestRead, readId == nil { readId = reads.first?.id }
            error = ""
        } catch {
            self.error = describe(error)
        }
        loaded = true
    }

    private func delete(_ memory: Memory) {
        memories.removeAll { $0.id == memory.id }
        Task {
            do { try await model.deleteMemory(memory.id) } catch {
                self.error = describe(error)
                await load()
            }
        }
    }
}

private struct MemoryCard: View {
    let memory: Memory
    let about: MemoryScope?
    let edit: () -> Void
    let forget: () -> Void

    var body: some View {
        let today = Date.now.formatted(localDay)
        let ended = memory.validUntil.map { $0 < today } ?? false
        HStack(alignment: .top, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(memory.content).font(.body(13)).lineSpacing(4)
                    .foregroundStyle(ended ? Mint.muted : Mint.ink)
                    .multilineTextAlignment(.leading)
                if let about {
                    HStack(spacing: 4) {
                        Image(systemName: scopeIcon(about)).font(.system(size: 10))
                        Text(scopeLabel(about)).font(.body(11, .medium))
                    }
                    .foregroundStyle(Mint.ink)
                }
                Text(details(today: today)).font(.body(11)).foregroundStyle(Mint.dim)
                if let until = memory.validUntil {
                    HStack(spacing: 4) {
                        Image(systemName: "clock").font(.system(size: 10))
                        Text((ended ? "Expired " : "Expires ") + shortDate(until)).font(.body(11, .semibold))
                    }
                    .foregroundStyle(ended ? Mint.dim : Mint.accent)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Button(action: forget) {
                Image(systemName: "xmark").font(.system(size: 13, weight: .medium)).foregroundStyle(Mint.dim)
                    .frame(width: 36, height: 36)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Forget")
        }
        .padding(Mint.spacing)
        .card()
        .contentShape(Rectangle())
        .onTapGesture(perform: edit)
    }

    private func details(today: String) -> String {
        let origin = switch memory.source {
        case .user: "You told Beep"
        case .email: "From your email"
        case .slack: "From Slack"
        case .learned: "Learned"
        }
        var parts = [origin, shortDate(memory.updated)]
        if let from = memory.validFrom, from > today { parts.append("Starts " + shortDate(from)) }
        return parts.joined(separator: " · ")
    }
}

/// The subject filter for memories about the user, who has no scope.
private let userSubject = "user"

/// One key per subject, so memories about the same machine, project or account group together.
private func scopeKey(_ scope: MemoryScope) -> String {
    switch scope {
    case .machine(let id, _): "machine:\(id)"
    case .project(let name): "project:\(name)"
    case .account(let app): "account:\(app)"
    }
}

/// What a memory is about, as its card and chip say it: "build-box", "Project beep", "Github account".
private func scopeLabel(_ scope: MemoryScope) -> String {
    switch scope {
    case .machine(_, let name): name ?? "A machine"
    case .project(let name): "Project \(name)"
    case .account(let app): "\(app.prefix(1).uppercased() + app.dropFirst()) account"
    }
}

/// The subject in a sentence: "the machine build-box".
private func scopeSubject(_ scope: MemoryScope) -> String {
    switch scope {
    case .machine(_, let name): name.map { "the machine \($0)" } ?? "a machine no longer linked"
    case .project(let name): "the project \(name)"
    case .account(let app): "your \(app.prefix(1).uppercased() + app.dropFirst()) account"
    }
}

private func scopeIcon(_ scope: MemoryScope) -> String {
    switch scope {
    case .machine: "server.rack"
    case .project: "folder"
    case .account: "person.crop.circle"
    }
}

/// `YYYY-MM-DD` in this phone's time zone, so a date means the local day rather than the UTC one.
private let localDay = Date.ISO8601FormatStyle(timeZone: .current).year().month().day()

/// A `YYYY-MM-DD` date as "9 Oct", with the year when it is not this year.
private func shortDate(_ date: String) -> String {
    guard let day = try? Date(date, strategy: localDay) else { return date }
    let sameYear = Calendar.current.isDate(day, equalTo: .now, toGranularity: .year)
    return sameYear ? day.formatted(.dateTime.day().month(.abbreviated))
        : day.formatted(.dateTime.day().month(.abbreviated).year())
}

private struct ReadChip: View {
    let label: String
    let count: Int?
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Text(label).foregroundStyle(Mint.ink)
                if let count { Text("\(count)").foregroundStyle(Mint.dim) }
            }
            .font(.body(12))
            .padding(.horizontal, 12).frame(minHeight: 32)
            .background(Capsule().fill(selected ? Mint.accent.opacity(0.18) : Mint.surface))
            .overlay(Capsule().stroke(selected ? Mint.accent : Mint.soft, lineWidth: 1))
        }
        .buttonStyle(.plain)
    }
}

private func readFinished(_ read: MemoryRead) -> Date? { try? Date(read.finishedAt, strategy: .iso8601) }

/// "Gmail · 10 Oct".
private func readLabel(_ read: MemoryRead) -> String {
    guard let day = readFinished(read) else { return read.source }
    return "\(read.source) · \(day.formatted(.dateTime.day().month(.abbreviated)))"
}

private func readDate(_ read: MemoryRead) -> String {
    guard let day = readFinished(read) else { return "earlier" }
    return Calendar.current.isDateInToday(day) ? "today" : "on \(day.formatted(.dateTime.day().month(.wide)))"
}
