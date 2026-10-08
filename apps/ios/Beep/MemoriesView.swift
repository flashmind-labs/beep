import SwiftUI

struct MemoriesView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var memories: [Memory] = []
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
                .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.soft, lineWidth: 1))
                Text("\(shown.count)").font(.body(12)).foregroundStyle(Mint.muted)
            }
            .padding(.horizontal, 20).padding(.vertical, 8)
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 20)
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
                                MemoryCard(memory: memory) {
                                    text = memory.content
                                    editing = memory
                                } forget: { delete(memory) }
                                .padding(.trailing, 3).padding(.bottom, 3)
                            }
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 8)
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

    private var shown: [Memory] {
        guard !query.isEmpty else { return memories }
        return memories.filter {
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
    let edit: () -> Void
    let forget: () -> Void

    var body: some View {
        let today = Date.now.formatted(.iso8601.year().month().day())
        let ended = memory.validUntil.map { $0 < today } ?? false
        HStack(alignment: .top, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Text(memory.content).font(.body(13)).lineSpacing(4)
                    .foregroundStyle(ended ? Mint.muted : Mint.ink)
                    .multilineTextAlignment(.leading)
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
        .padding(14)
        .chunky()
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

/// A `YYYY-MM-DD` date as "9 Oct", with the year when it is not this year.
private func shortDate(_ date: String) -> String {
    guard let day = try? Date(date, strategy: .iso8601.year().month().day()) else { return date }
    let sameYear = Calendar.current.isDate(day, equalTo: .now, toGranularity: .year)
    return sameYear ? day.formatted(.dateTime.day().month(.abbreviated))
        : day.formatted(.dateTime.day().month(.abbreviated).year())
}
