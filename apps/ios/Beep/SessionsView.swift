import SwiftUI

struct SessionsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var query = ""
    @State private var renaming: Session?
    @State private var newTitle = ""

    var body: some View {
        List {
            let shown = model.sessions.filter {
                query.isEmpty || $0.title.localizedCaseInsensitiveContains(query)
            }
            let pinned = shown.filter(\.pinned)
            let recent = shown.filter { !$0.pinned }
            if !pinned.isEmpty {
                Section("Pinned") { ForEach(pinned, id: \.id, content: row) }
            }
            Section(pinned.isEmpty ? "" : "Recent") { ForEach(recent, id: \.id, content: row) }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Mint.page)
        .overlay {
            if model.sessions.isEmpty {
                ContentUnavailableView("No chats yet", systemImage: "bubble.left.and.bubble.right",
                    description: Text("Start one and it shows up here."))
            }
        }
        .navigationTitle("Chats")
        .searchable(text: $query)
        .refreshable { await model.refreshSessions() }
        .task { await model.refreshSessions() }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { path.append(.settings) } label: { Image(systemName: "gearshape") }
                    .accessibilityLabel("Settings")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    model.newChat()
                    path.removeAll()
                } label: { Image(systemName: "square.and.pencil") }
                    .accessibilityLabel("New chat")
            }
        }
        .alert("Rename chat", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Title", text: $newTitle)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                guard let session = renaming else { return }
                let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                if !title.isEmpty { Task { await model.rename(session, to: title) } }
            }
        }
    }

    private func row(_ session: Session) -> some View {
        Button {
            model.open(session)
            path.removeAll()
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text(session.title.isEmpty ? "New chat" : session.title)
                        .font(.body(16, .medium)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(Self.when(session.updatedAt)).font(.body(12)).foregroundStyle(Mint.muted)
                }
                Spacer()
                if session.id == model.chat.id {
                    Circle().fill(Mint.brand).frame(width: 8, height: 8)
                }
            }
        }
        .swipeActions(edge: .leading) {
            Button { Task { await model.togglePin(session) } } label: {
                Label(session.pinned ? "Unpin" : "Pin", systemImage: session.pinned ? "pin.slash" : "pin")
            }
            .tint(Mint.brand)
        }
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) { Task { await model.delete(session) } } label: {
                Label("Delete", systemImage: "trash")
            }
        }
        .contextMenu {
            Button { startRename(session) } label: { Label("Rename", systemImage: "pencil") }
            Button { Task { await model.togglePin(session) } } label: {
                Label(session.pinned ? "Unpin" : "Pin", systemImage: "pin")
            }
            Button(role: .destructive) { Task { await model.delete(session) } } label: {
                Label("Delete", systemImage: "trash")
            }
        }
    }

    private func startRename(_ session: Session) {
        newTitle = session.title
        renaming = session
    }

    /// `updatedAt` is in milliseconds.
    static func when(_ millis: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(millis) / 1000)
        return date.formatted(.relative(presentation: .named))
    }
}
