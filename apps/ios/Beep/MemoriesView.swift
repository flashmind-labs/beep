import SwiftUI

struct MemoriesView: View {
    @Environment(AppModel.self) private var model
    @State private var memories: [Memory] = []
    @State private var loaded = false
    @State private var error = ""
    @State private var editing: Memory?
    @State private var text = ""
    @State private var confirmClear = false

    var body: some View {
        List {
            if !error.isEmpty {
                Text(error).foregroundStyle(Mint.error)
            }
            ForEach(groups, id: \.name) { group in
                Section(group.name) {
                    ForEach(group.items, id: \.id) { memory in
                        Button {
                            text = memory.content
                            editing = memory
                        } label: {
                            Text(memory.content).foregroundStyle(Mint.ink).multilineTextAlignment(.leading)
                        }
                        .swipeActions {
                            Button(role: .destructive) { delete(memory) } label: { Label("Forget", systemImage: "trash") }
                        }
                    }
                }
            }
            if !memories.isEmpty {
                Section {
                    Button("Forget all", role: .destructive) { confirmClear = true }
                }
            }
        }
        .font(.body(15))
        .scrollContentBackground(.hidden)
        .background(Mint.page)
        .overlay {
            if loaded, memories.isEmpty, error.isEmpty {
                ContentUnavailableView("Nothing remembered yet", systemImage: "brain",
                    description: Text("Ask Beep to remember something."))
            }
        }
        .navigationTitle("Memories")
        .task(id: model.connected) { if model.connected { await load() } }
        .refreshable { await load() }
        .confirmationDialog("Forget everything Beep remembers?", isPresented: $confirmClear, titleVisibility: .visible) {
            Button("Forget all", role: .destructive) {
                Task {
                    do { try await model.clearMemories() } catch { self.error = describe(error) }
                    await load()
                }
            }
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

    private struct Group {
        var name: String
        var items: [Memory]
    }

    private var groups: [Group] {
        Dictionary(grouping: memories) { $0.category.isEmpty ? "Other" : $0.category.capitalized }
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
