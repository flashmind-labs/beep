import SwiftUI

/// The app opens on a list like a messenger: Beep's chats and the groups, pinned
/// first, then the newest. Places, memories, routines, skills and settings are in the menu.
struct HomeView: View {
    @Environment(AppModel.self) private var model
    let openChat: (Session?) -> Void
    let openGroup: (String) -> Void
    let go: (Route) -> Void
    let sheet: (GroupSheet) -> Void
    @State private var searching = false
    @State private var query = ""
    @State private var matches: [SessionMatch] = []
    @State private var selected: Set<String> = []
    @State private var loaded = false
    @State private var renaming: Session?
    @State private var newTitle = ""
    @State private var confirmDelete = false
    @State private var confirmLeave = false
    @FocusState private var searchFocus: Bool

    var body: some View {
        VStack(spacing: 0) {
            if searching {
                searchBar
                searchResults
            } else {
                if selected.isEmpty { topBar } else { selectionBar }
                list
            }
        }
        .background(Mint.page)
        .overlay(alignment: .bottomTrailing) {
            if !searching, selected.isEmpty {
                Button { sheet(.start) } label: { Image(systemName: "plus") }
                    .buttonStyle(ChunkyButton(fill: Mint.accent, size: 56, radius: 18))
                    .accessibilityLabel("New")
                    .padding(.trailing, 19).padding(.bottom, 21)
            }
        }
        .toolbar(.hidden, for: .navigationBar)
        .task(id: model.connected) {
            guard model.connected else { return }
            await model.refreshSessions()
            await model.refreshGroups()
            loaded = true
        }
        .task(id: query) {
            let text = query.trimmingCharacters(in: .whitespaces)
            guard !text.isEmpty else {
                matches = []
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            matches = await model.search(text)
        }
        .alert("Rename chat", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Title", text: $newTitle)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                guard let session = renaming else { return }
                let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                selected = []
                if !title.isEmpty { Task { await model.rename(session, to: title) } }
            }
        }
        .alert(selected.count == 1 ? "Delete this conversation?" : "Delete \(selected.count) conversations?",
               isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) {
                let sessions = selectedItems.compactMap(\.session)
                selected = []
                Task { for session in sessions { await model.delete(session) } }
            }
        } message: {
            Text("This will permanently delete it and all its messages.")
        }
        .alert(leaveTitle, isPresented: $confirmLeave) {
            Button("Cancel", role: .cancel) {}
            Button("Leave", role: .destructive) {
                let groups = selectedItems.compactMap(\.group)
                selected = []
                Task { for group in groups { try? await model.leaveGroup(group.id) } }
            }
        } message: {
            Text("You stop getting its messages. To come back, someone has to send you the code again.")
        }
    }

    // MARK: Bars

    private var topBar: some View {
        HStack(spacing: 2) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Beep").font(.display(28)).foregroundStyle(Mint.ink)
                HStack(spacing: 6) {
                    ConnectionDot(connected: model.connected)
                    if !model.connected {
                        Text("Tap to reconnect").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                    }
                }
                .contentShape(Rectangle())
                .onTapGesture { if !model.connected { model.connect() } }
            }
            Spacer()
            iconButton("magnifyingglass", "Search") {
                searching = true
                searchFocus = true
            }
            Menu {
                Button { go(.places) } label: { Label("Places", systemImage: "mappin.and.ellipse") }
                Button { go(.memories) } label: { Label("Memories", systemImage: "brain") }
                Button { go(.routines) } label: { Label("Routines", systemImage: "clock") }
                Button { go(.skills) } label: { Label("Skills", systemImage: "wand.and.stars") }
                Button { go(.settings) } label: { Label("Apps", systemImage: "square.grid.2x2") }
                Button { go(.settings) } label: { Label("Settings", systemImage: "gearshape") }
            } label: {
                Image(systemName: "ellipsis").font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(Mint.ink).frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .accessibilityLabel("More")
        }
        .padding(.leading, 18).padding(.trailing, 6).padding(.vertical, 6)
    }

    private var selectionBar: some View {
        let items = selectedItems
        let chats = items.allSatisfy { $0.session != nil }
        let groups = items.allSatisfy { $0.group != nil }
        return HStack(spacing: 2) {
            iconButton("xmark", "Clear selection") { selected = [] }
            Text("\(selected.count)").font(.display(22)).foregroundStyle(Mint.ink).padding(.leading, 4)
            Spacer()
            if chats, items.count == 1, let session = items.first?.session {
                iconButton("pencil", "Rename") {
                    newTitle = session.title
                    renaming = session
                }
            }
            iconButton(items.allSatisfy(\.pinned) ? "pin.slash" : "pin", "Pin") { pinSelected() }
            if chats {
                iconButton("trash", "Delete", color: Mint.error) { confirmDelete = true }
            } else if groups {
                iconButton("rectangle.portrait.and.arrow.right", "Leave group", color: Mint.error) { confirmLeave = true }
            }
        }
        .padding(.horizontal, 6).padding(.vertical, 6)
        .frame(minHeight: 64)
        .background(Mint.user)
    }

    private var leaveTitle: String {
        let groups = selectedItems.compactMap(\.group)
        return groups.count == 1 ? "Leave \(groups[0].name)?" : "Leave \(groups.count) groups?"
    }

    private func iconButton(_ icon: String, _ label: String, color: Color = Mint.ink,
                            action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 18, weight: .semibold))
                .foregroundStyle(color).frame(width: 44, height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }

    // MARK: List

    private var items: [HomeItem] {
        let chats = model.sessions.map(HomeItem.chat)
        let groups = model.groups.map(HomeItem.group)
        return (chats + groups).sorted { ($0.pinned ? 1 : 0, $0.updatedAt) > ($1.pinned ? 1 : 0, $1.updatedAt) }
    }

    private var selectedItems: [HomeItem] { items.filter { selected.contains($0.id) } }

    @ViewBuilder private var list: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                if items.isEmpty, loaded {
                    HomeRow(avatar: .beep, name: "Beep", time: "now", preview: "Hi! Ask me anything, or tell me where you are.",
                            pinned: true)
                        .contentShape(Rectangle())
                        .onTapGesture { openChat(nil) }
                    Text("Your chats with Beep and your groups show up here.\nStart one with the + button.")
                        .font(.body(14)).foregroundStyle(Mint.muted).lineSpacing(5)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 32).padding(.vertical, 40)
                } else if items.isEmpty {
                    ProgressView().padding(.top, 60)
                }
                ForEach(items) { item in
                    row(item)
                }
            }
            .padding(.bottom, 84)
        }
        .refreshable {
            await model.refreshSessions()
            await model.refreshGroups()
        }
    }

    private func row(_ item: HomeItem) -> some View {
        let held = selected.contains(item.id)
        return SwiftUI.Group {
            switch item {
            case .chat(let session):
                HomeRow(avatar: .beep, name: session.title.isEmpty ? "New chat" : session.title,
                        time: listTime(session.updatedAt), preview: chatPreview(session), pinned: session.pinned, held: held)
                    .task(id: session.updatedAt) { await model.loadPreview(session) }
            case .group(let group):
                HomeRow(avatar: .group(groupInitials(group.name), Mint.tint(for: group.id)), name: group.name, group: true,
                        time: listTime(group.updatedAt), preview: groupSummary(group), pinned: group.pinned, held: held)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if !selected.isEmpty {
                if held { selected.remove(item.id) } else { selected.insert(item.id) }
            } else {
                open(item)
            }
        }
        .onLongPressGesture(minimumDuration: 0.4) {
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            selected.insert(item.id)
        }
    }

    private func chatPreview(_ session: Session) -> String {
        if session.id == model.chat.id, model.chat.busy { return "Beep is typing…" }
        return model.previews[session.id]?.text ?? ""
    }

    private func open(_ item: HomeItem) {
        switch item {
        case .chat(let session): openChat(session)
        case .group(let group): openGroup(group.id)
        }
    }

    private func pinSelected() {
        let items = selectedItems
        let pin = !items.allSatisfy(\.pinned)
        selected = []
        Task {
            for item in items {
                switch item {
                case .chat(let session): if session.pinned != pin { await model.togglePin(session) }
                case .group(let group): await model.pinGroup(group.id, pinned: pin)
                }
            }
        }
    }

    // MARK: Search

    private var searchBar: some View {
        HStack(spacing: 4) {
            iconButton("arrow.left", "Close search") {
                searching = false
                query = ""
            }
            HStack(spacing: 8) {
                TextField("", text: $query, prompt: Text("Search chats and places").foregroundStyle(Mint.dim))
                    .font(.body(15)).foregroundStyle(Mint.ink).tint(Mint.ink)
                    .focused($searchFocus)
                    .submitLabel(.search)
                if !query.isEmpty {
                    Button { query = "" } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(Mint.dim)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear")
                }
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 46)
            .background(Capsule().fill(Mint.surface))
            .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1.5))
        }
        .padding(.leading, 6).padding(.trailing, 14).padding(.vertical, 8)
    }

    @ViewBuilder private var searchResults: some View {
        let text = query.trimmingCharacters(in: .whitespaces)
        let groups = text.isEmpty ? [] : model.groups.filter {
            $0.name.localizedCaseInsensitiveContains(text) || groupSummary($0).localizedCaseInsensitiveContains(text)
        }
        let places = text.isEmpty ? [] : model.places.filter {
            $0.name.localizedCaseInsensitiveContains(text) || ($0.address ?? "").localizedCaseInsensitiveContains(text)
        }
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if !matches.isEmpty || !groups.isEmpty {
                    sectionLabel("Chats")
                    ForEach(matches, id: \.session.id) { match in
                        HomeRow(avatar: .beep, name: match.session.title.isEmpty ? "New chat" : match.session.title,
                                time: listTime(match.session.updatedAt),
                                preview: (match.fromUser ? "You: " : "") + match.snippet, highlight: text)
                            .contentShape(Rectangle())
                            .onTapGesture { openChat(match.session) }
                    }
                    ForEach(groups, id: \.id) { group in
                        HomeRow(avatar: .group(groupInitials(group.name), Mint.tint(for: group.id)), name: group.name, group: true,
                                time: listTime(group.updatedAt), preview: groupSummary(group), highlight: text)
                            .contentShape(Rectangle())
                            .onTapGesture { openGroup(group.id) }
                    }
                }
                if !places.isEmpty {
                    sectionLabel("Places")
                    ForEach(places) { place in
                        HomeRow(avatar: .place(Mint.tint(for: place.id)), name: place.name, time: "\(place.radius) m",
                                preview: place.address ?? "", highlight: text)
                            .contentShape(Rectangle())
                            .onTapGesture { go(.places) }
                    }
                }
                if !text.isEmpty, matches.isEmpty, groups.isEmpty, places.isEmpty {
                    Text("Nothing found for \"\(text)\"").font(.body(14)).foregroundStyle(Mint.muted)
                        .frame(maxWidth: .infinity).padding(.top, 40)
                }
            }
        }
        .scrollDismissesKeyboard(.interactively)
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 2)
    }
}

/// One row of the list: avatar, name and time, then the last line and a pin.
struct HomeRow: View {
    enum Avatar {
        case beep
        case group(String, Color)
        case place(Color)
    }

    let avatar: Avatar
    let name: String
    var group = false
    let time: String
    let preview: String
    var pinned = false
    var held = false
    var highlight = ""

    var body: some View {
        HStack(spacing: 12) {
            picture
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    HStack(spacing: 4) {
                        if group {
                            Image(systemName: "person.2.fill").font(.system(size: 11)).foregroundStyle(Mint.muted)
                        }
                        Text(marked(name)).font(.body(15, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    Text(time).font(.body(11.5)).foregroundStyle(Mint.dim).monospacedDigit()
                }
                HStack(spacing: 6) {
                    Text(marked(preview.isEmpty ? " " : preview)).font(.body(13)).foregroundStyle(Mint.muted).lineLimit(1)
                    Spacer(minLength: 0)
                    if pinned {
                        Image(systemName: "pin.fill").font(.system(size: 12)).foregroundStyle(Mint.dim)
                            .accessibilityLabel("Pinned")
                    }
                }
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 9)
        .background(held ? Mint.user : .clear)
    }

    @ViewBuilder private var picture: some View {
        if held {
            Image(systemName: "checkmark").font(.system(size: 18, weight: .bold)).foregroundStyle(.white)
                .frame(width: 48, height: 48)
                .background(Circle().fill(Mint.ok))
                .overlay(Circle().stroke(Mint.ledge, lineWidth: 1.5))
        } else {
            switch avatar {
            case .beep: ChatAvatar(text: "b", fill: Mint.brand, light: true)
            case .group(let initials, let tint): ChatAvatar(text: initials, fill: tint)
            case .place(let tint):
                Image(systemName: "mappin").font(.system(size: 18, weight: .semibold)).foregroundStyle(Mint.ink)
                    .frame(width: 48, height: 48)
                    .background(Circle().fill(tint))
                    .overlay(Circle().stroke(Mint.ledge, lineWidth: 1.5))
            }
        }
    }

    /// The search words with a yellow mark behind them.
    private func marked(_ text: String) -> AttributedString {
        var out = AttributedString(text)
        guard !highlight.isEmpty else { return out }
        var start = out.startIndex
        while start < out.endIndex,
            let range = out[start...].range(of: highlight, options: [.caseInsensitive, .diacriticInsensitive]) {
            out[range].backgroundColor = Mint.tints[2]
            start = range.upperBound
        }
        return out
    }
}

/// A round avatar with a ledge outline: Beep's "b" or a group's initials.
struct ChatAvatar: View {
    let text: String
    let fill: Color
    var size: CGFloat = 48
    var light = false

    var body: some View {
        Text(text)
            .font(.body(size * (text.count > 1 ? 0.31 : 0.4), .bold))
            .foregroundStyle(light ? .white : Mint.ink)
            .frame(width: size, height: size)
            .background(Circle().fill(fill))
            .overlay(Circle().stroke(Mint.ledge, lineWidth: 1.5))
    }
}

/// "TT" for Tokyo trip, "F" for Flat.
func groupInitials(_ name: String) -> String {
    let words = name.split(whereSeparator: { $0.isWhitespace }).prefix(2)
    let letters = words.compactMap(\.first).map { String($0).uppercased() }.joined()
    return letters.isEmpty ? "?" : letters
}

/// The list's time: "09:52" today, then "Yesterday", a weekday this week, else "3 Oct".
func listTime(_ millis: Int64) -> String {
    let date = Date(timeIntervalSince1970: Double(millis) / 1000)
    let calendar = Calendar.current
    if calendar.isDateInToday(date) { return date.formatted(date: .omitted, time: .shortened) }
    if calendar.isDateInYesterday(date) { return "Yesterday" }
    if let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: .now)).day,
        days < 7 {
        return date.formatted(.dateTime.weekday(.abbreviated))
    }
    if calendar.isDate(date, equalTo: .now, toGranularity: .year) {
        return date.formatted(.dateTime.day().month(.abbreviated))
    }
    return date.formatted(.dateTime.day().month(.abbreviated).year())
}

/// A row of the list: a chat with Beep or a group.
enum HomeItem: Identifiable {
    case chat(Session)
    case group(Group)

    var id: String {
        switch self {
        case .chat(let session): session.id
        case .group(let group): "group-" + group.id
        }
    }

    var session: Session? {
        if case .chat(let session) = self { return session }
        return nil
    }

    var group: Group? {
        if case .group(let group) = self { return group }
        return nil
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

/// The New sheet behind the + button.
struct StartSheet: View {
    let newChat: () -> Void
    let newGroup: () -> Void
    let joinCode: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            option(fill: Mint.accent, title: "New chat with Beep", note: "A fresh conversation, same memories", action: newChat) {
                ChatAvatar(text: "b", fill: Mint.surface, size: 40)
            }
            option(title: "New group", note: "Chat with people, and Beep helps when it can", action: newGroup) {
                TintIcon(system: "person.2", tint: Mint.tints[2])
            }
            option(title: "Join with a code", note: "Enter or scan the code someone sent you", action: joinCode) {
                TintIcon(system: "qrcode", tint: Mint.tints[1])
            }
        }
        .padding(.horizontal, 18).padding(.top, 28).padding(.bottom, 12)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Mint.page)
        .presentationDetents([.height(300)])
        .presentationDragIndicator(.visible)
        .presentationCornerRadius(28)
    }

    private func option<Icon: View>(fill: Color = Mint.surface, title: String, note: String, action: @escaping () -> Void,
                                    @ViewBuilder icon: () -> Icon) -> some View {
        Button(action: action) {
            HStack(spacing: 14) {
                icon()
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                    Text(note).font(.body(12.5)).foregroundStyle(Mint.ink.opacity(0.7)).multilineTextAlignment(.leading)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .chunky(fill, radius: 16)
        }
        .buttonStyle(.plain)
    }
}
