import SwiftUI

/// The chats drawer that slides in from the left, as on Android.
struct ChatsDrawer: View {
    @Environment(AppModel.self) private var model
    @Binding var open: Bool
    @Binding var path: [Route]
    @Binding var sheet: GroupSheet?
    @State private var query = ""
    @State private var renaming: Session?
    @State private var newTitle = ""
    @State private var deleting: Session?
    @State private var leaving: Group?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Your chats").font(.display(26)).foregroundStyle(Mint.ink)
                Spacer()
                Button { go { sheet = .new } } label: { Image(systemName: "person.3").font(.system(size: 15, weight: .semibold)) }
                    .buttonStyle(ChunkyButton())
                    .accessibilityLabel("New group")
                Button { go { model.newChat() } } label: { Image(systemName: "plus") }
                    .buttonStyle(ChunkyButton(fill: Mint.accent))
                    .accessibilityLabel("New chat")
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 12)
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").font(.system(size: 15)).foregroundStyle(Mint.muted)
                TextField("", text: $query, prompt: Text("Find a conversation").foregroundStyle(Mint.dim))
                    .font(.body(13)).foregroundStyle(Mint.ink)
            }
            .padding(.horizontal, 14)
            .frame(minHeight: 50)
            .background(RoundedRectangle(cornerRadius: 18).fill(Mint.surface))
            .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.soft, lineWidth: 1))
            .padding(.horizontal, 20)
            .padding(.bottom, 14)
            ScrollView {
                LazyVStack(spacing: 4) {
                    ForEach(shown) { item in
                        switch item {
                        case .chat(let session): row(session)
                        case .group(let group): groupRow(group)
                        }
                    }
                }
                .padding(.horizontal, 12)
            }
            .overlay {
                if model.sessions.isEmpty, model.groups.isEmpty {
                    Text("No chats yet").font(.body(13)).foregroundStyle(Mint.muted)
                }
            }
            .refreshable {
                await model.refreshSessions()
                await model.refreshGroups()
            }
            Rectangle().fill(Mint.soft).frame(height: 1)
                .padding(.horizontal, 20).padding(.vertical, 8)
            HStack(spacing: 8) {
                tile("Memories", "brain", Mint.tints[0]) { path = [.memories] }
                tile("Routines", "clock", Mint.tints[1]) { path = [.routines] }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 8)
            HStack(spacing: 6) {
                Button { go { path = [.settings] } } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "gearshape").font(.system(size: 16))
                        Text("Settings").font(.body(14, .medium))
                    }
                }
                .buttonStyle(TextAction(color: Mint.muted))
                Spacer()
                ConnectionDot(connected: model.connected)
                Text(model.connected ? "Connected" : "Reconnecting")
                    .font(.body(11)).foregroundStyle(Mint.muted).lineLimit(1)
                    .padding(.trailing, 12)
            }
            .padding(.horizontal, 10)
            .padding(.bottom, 12)
        }
        .frame(maxWidth: 320, maxHeight: .infinity, alignment: .top)
        .background(UnevenRoundedRectangle(bottomTrailingRadius: 18, topTrailingRadius: 18)
            .fill(Mint.page).ignoresSafeArea())
        .task {
            await model.refreshSessions()
            await model.refreshGroups()
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
        .alert("Delete this conversation?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) {
                guard let session = deleting else { return }
                Task { await model.delete(session) }
            }
        } message: {
            Text("This will permanently delete it and all its messages.")
        }
        .alert("Leave \(leaving?.name ?? "the group")?", isPresented: Binding(get: { leaving != nil }, set: { if !$0 { leaving = nil } })) {
            Button("Cancel", role: .cancel) {}
            Button("Leave", role: .destructive) {
                guard let group = leaving else { return }
                Task { try? await model.leaveGroup(group.id) }
            }
        } message: {
            Text((leaving?.members.count ?? 0) <= 1
                ? "You are the last member, so the group and its messages are deleted."
                : "You stop getting its messages. Someone in it can invite you again.")
        }
    }

    /// Chats and groups together, pinned first, then the most recent, as on Android.
    private var shown: [DrawerItem] {
        let chats = model.sessions
            .filter { query.isEmpty || $0.title.localizedCaseInsensitiveContains(query) }
            .map(DrawerItem.chat)
        let groups = model.groups
            .filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) }
            .map(DrawerItem.group)
        return (chats + groups).sorted { ($0.pinned ? 1 : 0, $0.updatedAt) > ($1.pinned ? 1 : 0, $1.updatedAt) }
    }

    private func groupRow(_ group: Group) -> some View {
        let current = group.id == model.openGroupId
        return Button { go { model.openGroup(group.id); path.removeAll() } } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(group.name).font(.body(14)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(groupSummary(group)).font(.body(12)).lineLimit(1)
                        .foregroundStyle(current ? Mint.ink.opacity(0.7) : Mint.muted)
                }
                Spacer(minLength: 0)
                if group.pinned {
                    Image(systemName: "pin.fill").font(.system(size: 13)).foregroundStyle(Mint.brand)
                }
                Image(systemName: "person.2").font(.system(size: 13)).foregroundStyle(Mint.brand)
                    .accessibilityLabel("Group")
            }
            .padding(.horizontal, 16).padding(.vertical, 8)
            .frame(minHeight: 56)
            .background(RoundedRectangle(cornerRadius: 12).fill(current ? Mint.user : .clear))
            .contentShape(RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button { Task { await model.pinGroup(group.id, pinned: !group.pinned) } } label: {
                Label(group.pinned ? "Unpin" : "Pin", systemImage: group.pinned ? "pin.slash" : "pin")
            }
            Button(role: .destructive) { leaving = group } label: {
                Label("Leave group", systemImage: "rectangle.portrait.and.arrow.right")
            }
        }
    }

    private func row(_ session: Session) -> some View {
        let current = model.openGroupId == nil && session.id == model.chat.id
        return Button { go { model.open(session) } } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(session.title.isEmpty ? "New chat" : session.title)
                        .font(.body(14)).foregroundStyle(Mint.ink).lineLimit(2)
                        .multilineTextAlignment(.leading)
                    Text(chatTime(session.updatedAt)).font(.body(12))
                        .foregroundStyle(current ? Mint.ink.opacity(0.7) : Mint.muted)
                }
                Spacer(minLength: 0)
                if session.pinned {
                    Image(systemName: "pin.fill").font(.system(size: 13)).foregroundStyle(Mint.brand)
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 8)
            .frame(minHeight: 56)
            .background(RoundedRectangle(cornerRadius: 12).fill(current ? Mint.user : .clear))
            .contentShape(RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button {
                newTitle = session.title
                renaming = session
            } label: { Label("Rename", systemImage: "pencil") }
            Button { Task { await model.togglePin(session) } } label: {
                Label(session.pinned ? "Unpin" : "Pin", systemImage: session.pinned ? "pin.slash" : "pin")
            }
            Button(role: .destructive) { deleting = session } label: { Label("Delete", systemImage: "trash") }
        }
    }

    private func tile(_ label: String, _ icon: String, _ tint: Color, action: @escaping () -> Void) -> some View {
        Button { go(action) } label: {
            VStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 20))
                Text(label).font(.body(12, .semibold)).lineLimit(1)
            }
            .foregroundStyle(Mint.ink)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .chunky(tint, radius: 12)
        }
        .buttonStyle(.plain)
        .padding(.trailing, 3).padding(.bottom, 3)
    }

    /// Runs the action and closes the drawer.
    private func go(_ action: () -> Void) {
        action()
        withAnimation(.snappy) { open = false }
    }
}

/// When a chat last changed: "Today, 14:32", "Yesterday, 09:10", "3 Oct, 18:05".
func chatTime(_ millis: Int64) -> String {
    let date = Date(timeIntervalSince1970: Double(millis) / 1000)
    let calendar = Calendar.current
    let day: String
    if calendar.isDateInToday(date) {
        day = "Today"
    } else if calendar.isDateInYesterday(date) {
        day = "Yesterday"
    } else if calendar.isDate(date, equalTo: .now, toGranularity: .year) {
        day = date.formatted(.dateTime.day().month(.abbreviated))
    } else {
        day = date.formatted(.dateTime.day().month(.abbreviated).year())
    }
    return day + ", " + date.formatted(date: .omitted, time: .shortened)
}

/// A row in the drawer: a chat with Beep or a group.
private enum DrawerItem: Identifiable {
    case chat(Session)
    case group(Group)

    var id: String {
        switch self {
        case .chat(let session): session.id
        case .group(let group): "group-" + group.id
        }
    }

    var pinned: Bool {
        switch self {
        case .chat(let session): session.pinned
        case .group(let group): group.pinned
        }
    }

    var updatedAt: Int64 {
        switch self {
        case .chat(let session): session.updatedAt
        case .group(let group): group.updatedAt
        }
    }
}
