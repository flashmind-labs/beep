import SwiftUI

/// The chat list as Android has it: a sheet from the left over the chat, with the chats and
/// groups, the tiles for the other screens, settings and the connection.
struct ChatsDrawer<Content: View>: View {
    @Environment(AppModel.self) private var model
    @Binding var open: Bool
    let openChat: (Session) -> Void
    let openGroup: (String) -> Void
    let newGroup: () -> Void
    let invite: (Group) -> Void
    let go: (Route) -> Void
    @ViewBuilder var content: Content
    @State private var query = ""
    @State private var hits: [SessionMatch] = []
    @State private var drag: CGFloat = 0
    /// Set while a closing swipe is under way. The sheet follows the finger, so the touch would
    /// otherwise end on the row it started on and open it.
    @State private var swiping = false
    @State private var renaming: Session?
    @State private var newTitle = ""
    @State private var deleting: Session?
    @State private var leaving: Group?
    @State private var sharing: Session?
    /// The rows picked for bulk delete, by `HomeItem.id`; selection mode is on while it is set.
    @State private var picked: Set<String>?
    @State private var confirmingBulk = false

    var body: some View {
        GeometryReader { geometry in
            let width = min(480, geometry.size.width * 0.9)
            // How far out the sheet is, from 0 (hidden) to 1 (open), following a closing drag.
            let shown = open ? max(0, 1 + min(0, drag) / width) : 0
            ZStack(alignment: .leading) {
                content
                Color.black.opacity(0.32 * shown)
                    .ignoresSafeArea()
                    .allowsHitTesting(open)
                    .onTapGesture { open = false }
                sheet
                    .frame(width: width)
                    .background(UnevenRoundedRectangle(bottomTrailingRadius: 18, topTrailingRadius: 18)
                        .fill(Mint.page).ignoresSafeArea())
                    .offset(x: (shown - 1) * (width + 20))
                    .simultaneousGesture(DragGesture(minimumDistance: 12)
                        .onChanged { value in
                            drag = value.translation.width
                            if abs(drag) > 10 { swiping = true }
                        }
                        .onEnded { value in
                            if value.predictedEndTranslation.width < -width / 3 { open = false }
                            withAnimation(.snappy) { drag = 0 }
                            Task {
                                try? await Task.sleep(for: .milliseconds(200))
                                swiping = false
                            }
                        })
            }
            .animation(.snappy(duration: 0.28), value: open)
        }
        .onChange(of: open) {
            guard open else {
                picked = nil
                return
            }
            hideKeyboard()
            Task {
                await model.refreshSessions()
                await model.refreshGroups()
            }
        }
        .task(id: query) {
            hits = []
            let text = query.trimmingCharacters(in: .whitespaces)
            guard text.count >= 2 else { return }
            try? await Task.sleep(for: .milliseconds(350))
            guard !Task.isCancelled else { return }
            let found = await model.search(text)
            guard !Task.isCancelled else { return }
            hits = found
        }
        .alert("Rename conversation", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Title", text: $newTitle)
            Button("Cancel", role: .cancel) {}
            Button("Rename") {
                guard let session = renaming else { return }
                let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                if !title.isEmpty { Task { await model.rename(session, to: title) } }
            }
        }
        .alert("Delete conversation?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) {
                guard let session = deleting else { return }
                Task { await model.delete(session) }
            }
        } message: {
            Text("This will permanently delete this conversation and all its messages.")
        }
        .alert(bulkTitle, isPresented: $confirmingBulk) {
            Button("Cancel", role: .cancel) {}
            Button(bulkChats.isEmpty ? "Leave" : "Delete", role: .destructive) {
                let chats = bulkChats, groups = bulkGroups
                picked = nil
                Task { await model.deleteAll(chats: chats, groups: groups) }
            }
        } message: {
            Text([bulkChats.isEmpty ? nil : "The conversations and their messages are deleted for good.",
                  bulkGroups.isEmpty ? nil : "You stop getting the groups' messages."]
                .compactMap { $0 }.joined(separator: " "))
        }
        .sheet(item: $sharing) { session in
            ShareChatSheet(session: session)
        }
        .alert("Leave \(leaving?.name ?? "")?", isPresented: Binding(get: { leaving != nil }, set: { if !$0 { leaving = nil } })) {
            Button("Cancel", role: .cancel) {}
            Button("Leave", role: .destructive) {
                guard let group = leaving else { return }
                if model.openGroup == group.id { model.openGroup = nil }
                Task { try? await model.leaveGroup(group.id) }
            }
        } message: {
            Text((leaving?.members.count ?? 0) <= 1 ? "You are the last member, so the group and its messages are deleted."
                 : "You stop getting its messages. Someone in it can invite you again.")
        }
    }

    private var sheet: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Your chats").font(.display(26)).foregroundStyle(Mint.ink)
                Spacer()
                Button(action: newGroup) { Image(systemName: "person.badge.plus") }
                    .buttonStyle(ChunkyButton(size: 40))
                    .accessibilityLabel("New group")
            }
            .padding(.leading, 20).padding(.trailing, 16).padding(.top, 20).padding(.bottom, 12)
            search
            if let picked { selectionBar(picked) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if !messageHits.isEmpty, !chats.isEmpty { label("Titles") }
                    ForEach(rows) { row in
                        switch row {
                        case .chat(let session): chatRow(session)
                        case .group(let group): groupRow(group)
                        }
                    }
                    if !messageHits.isEmpty { label("In messages") }
                    ForEach(messageHits, id: \.session.id) { hit in
                        hitRow(hit)
                    }
                }
            }
            .scrollDismissesKeyboard(.interactively)
            Rectangle().fill(Mint.soft).frame(height: 1).padding(.horizontal, 20).padding(.vertical, 8)
            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    tile("Memories", "brain", Mint.tints[0], .memories)
                    tile("Routines", "clock", Mint.tints[1], .routines)
                    tile("Skills", "sparkles", Mint.tints[2], .skills)
                }
                HStack(spacing: 8) {
                    tile("Places", "mappin.and.ellipse", Mint.tints[1], .places)
                    tile("Apps", "puzzlepiece.extension", Mint.tints[0], .apps)
                    Color.clear.frame(maxWidth: .infinity, maxHeight: 1)
                }
            }
            .padding(.horizontal, 16).padding(.bottom, 8)
            HStack(spacing: 6) {
                Button { show(.settings) } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "gearshape").font(.system(size: 16))
                        Text("Settings").font(.body(14, .medium))
                    }
                    .foregroundStyle(Mint.muted)
                    .padding(.horizontal, 12).frame(minHeight: 40).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Spacer()
                ConnectionDot(connected: model.connected)
                Text(model.connected ? "Connected" : "Disconnected").font(.body(11)).foregroundStyle(Mint.muted)
                    .padding(.trailing, 12)
            }
            .padding(.horizontal, 10).padding(.bottom, 12)
        }
    }

    private func selectionBar(_ ids: Set<String>) -> some View {
        let all = Set(rows.map(\.id))
        let everything = !all.isEmpty && ids.isSuperset(of: all)
        return HStack(spacing: 4) {
            Text("\(ids.count) selected").font(.body(15, .semibold)).foregroundStyle(Mint.ink)
            Spacer()
            Button(everything ? "None" : "All") { picked = everything ? [] : ids.union(all) }
                .buttonStyle(TextAction())
            Button { confirmingBulk = true } label: {
                Image(systemName: "trash").font(.system(size: 17))
                    .foregroundStyle(ids.isEmpty ? Mint.muted : Mint.accent)
                    .frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(ids.isEmpty)
            .accessibilityLabel("Delete selected")
            Button { picked = nil } label: {
                Image(systemName: "xmark").font(.system(size: 16, weight: .medium)).foregroundStyle(Mint.ink)
                    .frame(width: 44, height: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Done selecting")
        }
        .padding(.leading, 20).padding(.trailing, 8).padding(.bottom, 4)
    }

    private var bulkChats: Set<String> {
        Set(model.sessions.map(\.id).filter { picked?.contains($0) == true })
    }

    private var bulkGroups: Set<String> {
        Set(model.groups.map(\.id).filter { picked?.contains(HomeItem.groupKey($0)) == true })
    }

    /// "Delete 2 conversations and leave 1 group?"
    private var bulkTitle: String {
        func count(_ n: Int, _ noun: String) -> String? { n == 0 ? nil : "\(n) \(noun)\(n == 1 ? "" : "s")" }
        let parts = [count(bulkChats.count, "conversation").map { "delete " + $0 },
                     count(bulkGroups.count, "group").map { "leave " + $0 }].compactMap { $0 }
        let line = parts.joined(separator: " and ")
        return line.prefix(1).uppercased() + line.dropFirst() + "?"
    }

    private func toggle(_ id: String) {
        guard var ids = picked else { return }
        if ids.contains(id) { ids.remove(id) } else { ids.insert(id) }
        picked = ids
    }

    private func check(_ id: String) -> some View {
        let on = picked?.contains(id) == true
        return Image(systemName: on ? "checkmark.circle" : "circle").font(.system(size: 18))
            .foregroundStyle(on ? Mint.accent : Mint.muted)
            .accessibilityLabel(on ? "Selected" : "Not selected")
    }

    private var search: some View {
        HStack(spacing: 10) {
            Image(systemName: "magnifyingglass").font(.system(size: 15)).foregroundStyle(Mint.muted)
            TextField("", text: $query, prompt: Text("Find a conversation").foregroundStyle(Mint.dim))
                .font(.body(13)).foregroundStyle(Mint.ink).tint(Mint.ink)
                .submitLabel(.search)
            if !query.isEmpty {
                Button { query = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(Mint.dim) }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear")
            }
        }
        .padding(.horizontal, 16)
        .frame(minHeight: 52)
        .background(RoundedRectangle(cornerRadius: 18).fill(Mint.surface))
        .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.soft, lineWidth: 1))
        .padding(.horizontal, 20).padding(.vertical, 14)
    }

    // MARK: Rows

    private var text: String { query.trimmingCharacters(in: .whitespaces) }

    private var chats: [Session] {
        model.sessions.filter { text.isEmpty || $0.title.localizedCaseInsensitiveContains(text) }
    }

    /// Chats and groups by when they were last active, the pinned ones first.
    private var rows: [HomeItem] {
        let groups = model.groups.filter { text.isEmpty || $0.name.localizedCaseInsensitiveContains(text) }
        let all = (chats.map(HomeItem.chat) + groups.map(HomeItem.group)).sorted { $0.updatedAt > $1.updatedAt }
        return all.filter(\.pinned) + all.filter { !$0.pinned }
    }

    /// Chats found by what was written in them, past the ones the titles already show.
    private var messageHits: [SessionMatch] {
        guard picked == nil else { return [] }
        let titled = Set(chats.map(\.id))
        let known = Set(model.sessions.map(\.id))
        return hits.filter { !titled.contains($0.session.id) && known.contains($0.session.id) }
    }

    private func chatRow(_ session: Session) -> some View {
        let current = picked.map { $0.contains(session.id) } ?? (model.openGroup == nil && model.chat.id == session.id)
        return Button { picked == nil ? pick { openChat(session) } : toggle(session.id) } label: {
            HStack(spacing: 12) {
                if picked != nil { check(session.id) }
                VStack(alignment: .leading, spacing: 1) {
                    Text(session.title.isEmpty ? "New conversation" : session.title)
                        .font(.body(14)).foregroundStyle(Mint.ink).lineLimit(2).multilineTextAlignment(.leading)
                    Text(chatTime(session.updatedAt)).font(.body(12))
                        .foregroundStyle(current ? Mint.ink.opacity(0.7) : Mint.muted).lineLimit(1)
                }
                Spacer(minLength: 0)
                if session.pinned, picked == nil {
                    Image(systemName: "pin").font(.system(size: 13)).foregroundStyle(Mint.brand).accessibilityLabel("Pinned")
                }
            }
            .rowFrame(highlighted: current)
        }
        .buttonStyle(.plain)
        .contextMenu {
            if picked == nil {
                Button { picked = [session.id] } label: { Label("Select", systemImage: "checkmark.circle") }
                Button { newTitle = session.title; renaming = session } label: { Label("Rename", systemImage: "pencil") }
                Button { Task { await model.togglePin(session) } } label: {
                    Label(session.pinned ? "Unpin" : "Pin", systemImage: session.pinned ? "pin.slash" : "pin")
                }
                Button { sharing = session } label: { Label("Share", systemImage: "square.and.arrow.up") }
                Button(role: .destructive) { deleting = session } label: { Label("Delete", systemImage: "trash") }
            }
        }
    }

    private func groupRow(_ group: Group) -> some View {
        let key = HomeItem.groupKey(group.id)
        let current = picked.map { $0.contains(key) } ?? (model.openGroup == group.id)
        return Button { picked == nil ? pick { openGroup(group.id) } : toggle(key) } label: {
            HStack(spacing: 12) {
                if picked != nil { check(key) }
                VStack(alignment: .leading, spacing: 1) {
                    Text(group.name).font(.body(14)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(groupSummary(group)).font(.body(12))
                        .foregroundStyle(current ? Mint.ink.opacity(0.7) : Mint.muted).lineLimit(1)
                }
                Spacer(minLength: 0)
                if group.pinned, picked == nil {
                    Image(systemName: "pin").font(.system(size: 13)).foregroundStyle(Mint.brand).accessibilityLabel("Pinned")
                }
                Image(systemName: "person.2").font(.system(size: 13)).foregroundStyle(Mint.brand).accessibilityLabel("Group")
            }
            .rowFrame(highlighted: current)
        }
        .buttonStyle(.plain)
        .contextMenu {
            if picked == nil {
                Button { picked = [key] } label: { Label("Select", systemImage: "checkmark.circle") }
                Button { Task { await model.pinGroup(group.id, pinned: !group.pinned) } } label: {
                    Label(group.pinned ? "Unpin" : "Pin", systemImage: group.pinned ? "pin.slash" : "pin")
                }
                Button { invite(group) } label: { Label("Invite", systemImage: "link") }
                Button(role: .destructive) { leaving = group } label: {
                    Label("Leave", systemImage: "rectangle.portrait.and.arrow.right")
                }
            }
        }
    }

    private func hitRow(_ hit: SessionMatch) -> some View {
        Button { pick { openChat(hit.session) } } label: {
            VStack(alignment: .leading, spacing: 2) {
                Text(hit.session.title.isEmpty ? "New conversation" : hit.session.title)
                    .font(.body(14)).foregroundStyle(Mint.ink).lineLimit(1)
                Text(marked((hit.fromUser ? "You: " : "") + hit.snippet)).font(.body(12)).foregroundStyle(Mint.muted)
                    .lineLimit(2).multilineTextAlignment(.leading)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.vertical, 8)
            .contentShape(Rectangle())
            .padding(.horizontal, 12).padding(.vertical, 2)
        }
        .buttonStyle(.plain)
    }

    /// The search words with the brand colour behind them.
    private func marked(_ line: String) -> AttributedString {
        var out = AttributedString(line)
        guard !text.isEmpty else { return out }
        var start = out.startIndex
        while start < out.endIndex,
            let range = out[start...].range(of: text, options: [.caseInsensitive, .diacriticInsensitive]) {
            out[range].backgroundColor = Mint.brand.opacity(0.25)
            out[range].foregroundColor = Mint.ink
            start = range.upperBound
        }
        return out
    }

    private func label(_ text: String) -> some View {
        Text(text).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            .padding(.leading, 28).padding(.trailing, 20).padding(.top, 10).padding(.bottom, 4)
    }

    private func tile(_ title: String, _ icon: String, _ tint: Color, _ route: Route) -> some View {
        Button { show(route) } label: {
            VStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 19)).frame(height: 24)
                Text(title).font(.body(12, .semibold)).lineLimit(1)
            }
            .foregroundStyle(Mint.ink)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
        }
        .buttonStyle(TileButton(fill: tint))
        .padding(.trailing, 3).padding(.bottom, 3)
    }

    private func show(_ route: Route) {
        pick { go(route) }
    }

    /// Runs a row's or tile's action and closes the drawer, unless the touch was a swipe.
    private func pick(_ action: () -> Void) {
        guard !swiping else { return }
        open = false
        action()
    }
}

private extension View {
    /// A drawer row: rounded, filled with the user colour when it is the chat on screen.
    func rowFrame(highlighted: Bool) -> some View {
        frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
            .padding(.horizontal, 16).padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 12).fill(highlighted ? Mint.user : .clear))
            .contentShape(Rectangle())
            .padding(.horizontal, 12).padding(.vertical, 2)
    }
}

/// A drawer tile: a tinted chunky key of any size that sinks onto its shadow.
private struct TileButton: ButtonStyle {
    let fill: Color

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: 12)
        let down: CGFloat = configuration.isPressed ? 3 : 0
        configuration.label
            .background(shape.fill(fill))
            .overlay(shape.stroke(Mint.ledge, lineWidth: 2))
            .offset(x: down, y: down)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge).offset(x: 3, y: 3))
    }
}

/// Swipe left starts a new chat, swipe right goes back to the last one, or opens the chat
/// list when there is none, as on Android. The chat lags the finger and a hint shows what
/// letting go does.
struct SwipeChats<Content: View>: View {
    let canStartNew: Bool
    let hasPrevious: Bool
    let onNew: () -> Void
    let onBack: () -> Void
    @ViewBuilder var content: Content
    @State private var offset: CGFloat = 0
    /// Whether the drag going on is a sideways one; a mostly vertical one is left to the scroll.
    @State private var sideways: Bool?
    /// Where the text box is on screen; a drag starting there selects text instead.
    @State private var textBox: CGRect = .null

    private let trigger: CGFloat = 72
    private let limit: CGFloat = 110

    var body: some View {
        let progress = min(1, abs(offset) / trigger)
        ZStack {
            if offset < 0, canStartNew {
                SwipeHint(icon: "plus", label: "New chat", progress: progress)
                    .frame(maxWidth: .infinity, alignment: .trailing).padding(.trailing, 16)
            }
            if offset > 0 {
                SwipeHint(icon: hasPrevious ? "arrow.left" : "line.3.horizontal",
                          label: hasPrevious ? "Previous" : "All chats", progress: progress)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 16)
            }
            content
                .offset(x: offset)
                .opacity(1 - progress * 0.25)
        }
        .onPreferenceChange(TextBoxFrame.self) { textBox = $0 }
        .simultaneousGesture(DragGesture(minimumDistance: 20, coordinateSpace: .global)
            .onChanged { value in
                if sideways == nil {
                    sideways = !textBox.contains(value.startLocation)
                        && abs(value.translation.width) > abs(value.translation.height) * 1.5
                }
                guard sideways == true else { return }
                // Damped so the chat lags the finger, and capped once past the trigger.
                let low = canStartNew ? -limit : -limit / 4
                offset = min(limit, max(low, value.translation.width * 0.5))
            }
            .onEnded { _ in
                defer { sideways = nil }
                guard sideways == true else { return }
                if offset <= -trigger, canStartNew { onNew() } else if offset >= trigger { onBack() }
                withAnimation(.spring(response: 0.35, dampingFraction: 0.6)) { offset = 0 }
            })
        .sensoryFeedback(.impact(weight: .light), trigger: progress >= 1) { _, ready in ready }
    }
}

private struct TextBoxFrame: PreferenceKey {
    static let defaultValue = CGRect.null
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) { value = value.union(nextValue()) }
}

extension View {
    /// Marks the chat's text box, so the chat swipes do not start in it.
    func chatTextBox() -> some View {
        background(GeometryReader { proxy in
            Color.clear.preference(key: TextBoxFrame.self, value: proxy.frame(in: .global))
        })
    }
}

private struct SwipeHint: View {
    let icon: String
    let label: String
    let progress: CGFloat

    var body: some View {
        let ready = progress >= 1
        VStack(spacing: 6) {
            Image(systemName: icon).font(.system(size: 19, weight: .semibold))
                .foregroundStyle(Mint.ink)
                .frame(width: 44, height: 44)
                .background(Circle().fill(ready ? Mint.accent : Mint.surface))
                .overlay(Circle().stroke(Mint.ledge, lineWidth: 2))
            Text(label).font(.body(11, .semibold)).foregroundStyle(Mint.muted)
        }
        .opacity(progress)
        .scaleEffect(0.8 + progress * 0.2)
    }
}

/// When a chat last changed, for the chat list: "Today, 14:32", "Yesterday, 09:10", "3 Oct, 18:05".
func chatTime(_ millis: Int64) -> String {
    let date = Date(timeIntervalSince1970: Double(millis) / 1000)
    let calendar = Calendar.current
    let time = date.formatted(date: .omitted, time: .shortened)
    let day = if calendar.isDateInToday(date) {
        "Today"
    } else if calendar.isDateInYesterday(date) {
        "Yesterday"
    } else if calendar.isDate(date, equalTo: .now, toGranularity: .year) {
        date.formatted(.dateTime.day().month(.abbreviated))
    } else {
        date.formatted(.dateTime.day().month(.abbreviated).year())
    }
    return "\(day), \(time)"
}

func hideKeyboard() {
    UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
}

/// SwiftUI drops the edge swipe back when the navigation bar is hidden, as every screen here
/// does; this gives it back to any stack with something to go back to.
extension UINavigationController: @retroactive UIGestureRecognizerDelegate {
    override open func viewDidLoad() {
        super.viewDidLoad()
        interactivePopGestureRecognizer?.delegate = self
    }

    public func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        viewControllers.count > 1
    }
}

/// The edge swipe back for a full-screen cover, which UIKit gives none: dragging in from the
/// left edge slides the screen along and lets go of it past a third of the width.
private struct EdgeSwipeBack: ViewModifier {
    let back: () -> Void
    @State private var drag: CGFloat = 0

    func body(content: Content) -> some View {
        GeometryReader { proxy in
            content
                .offset(x: drag)
                .overlay(alignment: .leading) {
                    Color.clear
                        .frame(width: 24)
                        .contentShape(Rectangle())
                        .gesture(DragGesture(minimumDistance: 10, coordinateSpace: .global)
                            .onChanged { drag = max(0, $0.translation.width) }
                            .onEnded { value in
                                let width = proxy.size.width
                                if value.predictedEndTranslation.width > width / 3 {
                                    withAnimation(.snappy(duration: 0.2)) { drag = width }
                                    Task {
                                        try? await Task.sleep(for: .milliseconds(200))
                                        back()
                                    }
                                } else {
                                    withAnimation(.snappy) { drag = 0 }
                                }
                            })
                        .ignoresSafeArea()
                }
        }
    }
}

extension View {
    /// Lets a swipe in from the left edge go back, as on a pushed screen.
    func edgeSwipeBack(_ back: @escaping () -> Void) -> some View {
        modifier(EdgeSwipeBack(back: back))
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

/// A row of the drawer: a chat with Beep or a group.
enum HomeItem: Identifiable {
    case chat(Session)
    case group(Group)

    var id: String {
        switch self {
        case .chat(let session): session.id
        case .group(let group): Self.groupKey(group.id)
        }
    }

    /// A group's key in the list and in the picked set, apart from chat ids.
    static func groupKey(_ id: String) -> String { "group-" + id }

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
