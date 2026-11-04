import CoreImage.CIFilterBuiltins
import SwiftUI

/// The name text of members, by the order they joined; their avatars use the tints.
private var nameColors: [Color] { Mint.nameColors }

/// The name the user last went by in a group, offered again for the next one.
private var lastMemberName: String {
    get { UserDefaults.standard.string(forKey: "group_member_name") ?? "" }
    set { UserDefaults.standard.set(newValue.trimmingCharacters(in: .whitespaces), forKey: "group_member_name") }
}

/// "Sam, Ali and Beep", with the caller as "You".
func groupPeople(_ group: Group) -> String {
    let names = group.members.sorted { $0.you && !$1.you }.map { $0.you ? "You" : $0.name } + ["Beep"]
    return names.dropLast().joined(separator: ", ") + " and " + names.last!
}

/// The last message as the chat list shows it: "Sam: text", or that Beep is typing.
func groupSummary(_ group: Group) -> String {
    if group.beepTyping { return "Beep is typing…" }
    guard let last = group.last else { return groupPeople(group) }
    return "\(authorName(last.author)): \(last.content)"
}

private func authorName(_ author: GroupAuthor) -> String {
    switch author {
    case .member(let name, let you): you ? "You" : name
    case .beep: "Beep"
    }
}

private func memberIndex(_ group: Group, _ name: String) -> Int {
    group.members.firstIndex { $0.name.caseInsensitiveCompare(name) == .orderedSame } ?? Mint.tintIndex(name)
}

private func memberTint(_ index: Int) -> Color { Mint.tints[index % Mint.tints.count] }

func initial(_ name: String) -> String { String(name.prefix(1)).uppercased() }

/// Bolds each @beep, so a mention stands out in the bubble.
private func withMentions(_ text: String) -> String {
    text.replacingOccurrences(of: "(?i)(?<![\\w*])@beep\\b", with: "**$0**", options: .regularExpression)
}

/// "Today", "Yesterday", "Mon 6 Oct" or, in another year, "6 Oct 2025".
private func dayLabel(_ date: Date) -> String {
    let calendar = Calendar.current
    if calendar.isDateInToday(date) { return "Today" }
    if calendar.isDateInYesterday(date) { return "Yesterday" }
    if calendar.isDate(date, equalTo: .now, toGranularity: .year) {
        return date.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
    }
    return date.formatted(.dateTime.day().month(.abbreviated).year())
}

private func date(_ millis: Int64) -> Date { Date(timeIntervalSince1970: Double(millis) / 1000) }

struct GroupAvatar: View {
    let letter: String
    let color: Color
    var size: CGFloat = 28
    var beep = false

    var body: some View {
        SwiftUI.Group {
            if beep { Image(systemName: "bubble.left") } else { Text(letter) }
        }
            .font(.system(size: size * 0.4, weight: .medium))
            .foregroundStyle(beep ? .white : Mint.ink)
            .frame(width: size, height: size)
            .background(Circle().fill(color))
            .overlay(Circle().stroke(Mint.page, lineWidth: 1.5))
    }
}

/// The open group sheet, shown over whatever is on screen.
enum GroupSheet: Identifiable {
    case new, code
    case invite(id: String, name: String)

    var id: String {
        switch self {
        case .new: "new"
        case .code: "code"
        case .invite(let id, _): "invite-\(id)"
        }
    }
}

// MARK: Chat

/// A message being sent, shown until the server has it.
private struct Outgoing: Identifiable {
    let id = UUID().uuidString.lowercased()
    let text: String
}

/// A group chat: members' messages, Beep's replies and a composer that can @mention.
struct GroupChatView: View {
    @Environment(AppModel.self) private var model
    let groupId: String
    /// Opens the chat list.
    let menu: () -> Void
    /// Leaves the group's screen, as when the group is gone.
    let back: () -> Void
    let info: () -> Void
    let invite: (Group) -> Void
    @State private var chat: GroupChat?
    @State private var pictures: [Int64: [Picture]] = [:]
    /// The last seq the pictures were fetched for, so polling only asks again after new messages.
    @State private var picturesSeq: Int64?
    @State private var outgoing: [Outgoing] = []
    @State private var draft = ""
    /// Replaced on each send, as in the chat's composer.
    @State private var field = 0
    @State private var error = ""
    @State private var replying: GroupMessage?
    @State private var flashing: Int64?
    /// Messages already shown, or there when the chat opened; these do not spring in.
    @State private var settled: Set<Int64>?
    @FocusState private var typing: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var group: Group? { chat?.group ?? model.groups.first { $0.id == groupId } }

    var body: some View {
        VStack(spacing: 0) {
            topBar
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 14) {
                        messages(scroller)
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 18).padding(.top, 8).padding(.bottom, 16)
                }
                .overlay { placeholder }
                .chatScrollAnchor()
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: chat?.messages.count) { toEnd(scroller) }
                .onChange(of: outgoing.count) { toEnd(scroller) }
                .onChange(of: chat?.group.beepTyping) { toEnd(scroller) }
            }
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, Mint.pageInset)
            }
            if let group { MentionPicker(group: group, draft: $draft) }
            if let replying, let group { replyBar(replying, group) }
            composer
        }
        .background(Mint.page)
        .toolbar(.hidden, for: .navigationBar)
        .onAppear {
            model.onScreen = groupId
            if let name = model.pendingMention {
                model.pendingMention = nil
                draft = "@\(name) " + draft
                typing = true
            }
        }
        .onDisappear { if model.onScreen == groupId { model.onScreen = nil } }
        // Notices say when a message arrives (ChatNotifier.handle), but not when Beep is typing, so this still polls.
        .task(id: groupId) {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(chat?.group.beepTyping == true ? 2 : 4))
            }
        }
    }

    private func toEnd(_ scroller: ScrollViewProxy) {
        withAnimation(reduceMotion ? nil : .smooth) { scroller.scrollTo("end", anchor: .bottom) }
    }

    private func load() async {
        do {
            let fresh = try await model.group(groupId)
            if settled == nil { settled = Set(fresh.messages.map(\.seq)) }
            withAnimation(.bouncy) { chat = fresh }
            let last = fresh.messages.last?.seq ?? 0
            if last != picturesSeq {
                pictures = await model.groupPictures(groupId)
                picturesSeq = last
            }
        } catch BeepError.NotFound {
            back()
        } catch {}
    }

    @ViewBuilder private var placeholder: some View {
        if chat == nil {
            ProgressView()
        } else if chat?.messages.isEmpty == true, outgoing.isEmpty {
            Text("No messages yet.\nSay hi. Beep joins in when it can help.")
                .font(.body(14)).foregroundStyle(Mint.muted).lineSpacing(6)
                .multilineTextAlignment(.center).padding(32)
        }
    }

    /// Messages in a row from one author, broken by a new day.
    private struct Run: Identifiable {
        var day: String?
        var author: GroupAuthor
        var messages: [GroupMessage]
        var id: Int64 { messages[0].seq }
    }

    private func runs(_ messages: [GroupMessage]) -> [Run] {
        var out: [Run] = []
        var current: Date?
        for message in messages {
            let day = Calendar.current.startOfDay(for: date(message.timestamp))
            let newDay = day != current
            current = day
            if !newDay, let last = out.last, last.author == message.author {
                out[out.count - 1].messages.append(message)
            } else {
                out.append(Run(day: newDay ? dayLabel(day) : nil, author: message.author, messages: [message]))
            }
        }
        return out
    }

    @ViewBuilder private func messages(_ scroller: ScrollViewProxy) -> some View {
        if let chat {
            let all = runs(chat.messages)
            ForEach(Array(all.enumerated()), id: \.element.id) { index, run in
                // Pending messages join the last run when it is yours.
                let pending = index == all.count - 1 && run.author.isYou ? outgoing : []
                VStack(spacing: 14) {
                    if let day = run.day {
                        Text(day).font(.body(11)).foregroundStyle(Mint.dim).frame(maxWidth: .infinity)
                    }
                    runView(run, pending: pending, group: chat.group, scroller: scroller)
                }
            }
            if !(all.last?.author.isYou ?? false), !outgoing.isEmpty {
                runView(Run(author: .member(name: "", you: true), messages: []), pending: outgoing, group: chat.group,
                        scroller: scroller)
            }
            if chat.group.beepTyping {
                HStack(spacing: 10) {
                    PulsingDots()
                    StatusLine()
                }
                .padding(.leading, 4)
                .popIn(anchor: .bottomLeading)
                .transition(.opacity)
            }
        }
    }

    private func runView(_ run: Run, pending: [Outgoing], group: Group, scroller: ScrollViewProxy) -> some View {
        let total = run.messages.count + pending.count
        return VStack(alignment: run.author.isYou ? .trailing : .leading, spacing: 3) {
            ForEach(Array(run.messages.enumerated()), id: \.element.seq) { index, message in
                GroupBubble(group: group, author: message.author, content: message.content,
                            pictures: pictures[message.seq] ?? [],
                            first: index == 0, last: index == total - 1, flash: flashing == message.seq,
                            quoteAuthor: { source(of: $0)?.author }, quoted: { jump(to: $0, scroller: scroller) }, reply: { reply(to: message) })
                    .id(message.seq)
                    // Your own message was already on screen while it was sending.
                    .popIn(play: !(settled?.contains(message.seq) ?? true) && !message.author.isYou,
                           anchor: run.author.isYou ? .bottomTrailing : .bottomLeading)
                    .onAppear { settled?.insert(message.seq) }
            }
            ForEach(Array(pending.enumerated()), id: \.element.id) { index, out in
                GroupBubble(group: group, author: .member(name: "", you: true), content: out.text,
                            first: run.messages.isEmpty && index == 0, last: run.messages.count + index == total - 1,
                            flash: false, quoteAuthor: { source(of: $0)?.author }, quoted: { _ in }, reply: {})
                    .opacity(0.6)
                    .popIn(anchor: .bottomTrailing)
            }
        }
        .frame(maxWidth: .infinity, alignment: run.author.isYou ? .trailing : .leading)
    }

    /// Scrolls to the message a quote came from and flashes it.
    private func jump(to quote: String, scroller: ScrollViewProxy) {
        guard let target = source(of: quote) else { return }
        withAnimation(.snappy) { scroller.scrollTo(target.seq, anchor: .center) }
        flashing = target.seq
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            if flashing == target.seq { withAnimation(.easeOut) { flashing = nil } }
        }
    }

    private func source(of quote: String) -> GroupMessage? {
        chat?.messages.last { plainLine(quoteBody($0.content).body).contains(quote) }
    }

    private var topBar: some View {
        HStack(spacing: 10) {
            Button {
                typing = false
                menu()
            } label: { Image(systemName: "line.3.horizontal") }
                .buttonStyle(ChunkyButton())
                .accessibilityLabel("Conversations")
            Button(action: info) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(group?.name ?? "Group").font(.display(18)).foregroundStyle(Mint.ink).lineLimit(1)
                    if let group {
                        HStack(spacing: 6) {
                            HStack(spacing: -6) {
                                ForEach(Array(group.members.sorted { $0.you && !$1.you }.prefix(4)), id: \.name) { member in
                                    GroupAvatar(letter: initial(member.name), color: memberTint(memberIndex(group, member.name)), size: 18)
                                }
                                GroupAvatar(letter: "", color: Mint.brand, size: 18, beep: true)
                            }
                            Text(groupPeople(group)).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(group == nil)
            Button { if let group { invite(group) } } label: {
                Image(systemName: "link").font(.system(size: 18)).foregroundStyle(Mint.muted).frame(width: 44, height: 44)
            }
            .disabled(group == nil)
            .accessibilityLabel("Invite")
        }
        .padding(.leading, 12)
        .padding(.trailing, 8)
        .padding(.vertical, 6)
        .background(Mint.page)
    }

    private func replyBar(_ message: GroupMessage, _ group: Group) -> some View {
        let name = authorName(message.author)
        return HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Replying to \(name == "You" ? "yourself" : name)").font(.body(12, .semibold))
                    .foregroundStyle(authorColor(message.author, group))
                Text(plainLine(quoteBody(message.content).body)).font(.body(13)).foregroundStyle(Mint.muted).lineLimit(1)
            }
            Spacer(minLength: 0)
            Button { replying = nil } label: {
                Image(systemName: "xmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.muted)
                    .frame(width: 32, height: 32)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Cancel reply")
        }
        .padding(.leading, 12).padding(.trailing, 6).padding(.vertical, 7)
        .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.surface))
        .overlay(alignment: .leading) { UnevenRoundedRectangle(topLeadingRadius: 10, bottomLeadingRadius: 10).fill(Mint.brand).frame(width: 3) }
        .padding(.horizontal, 14).padding(.top, 4)
    }

    private var composer: some View {
        let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        return HStack(alignment: .bottom, spacing: 8) {
            TextField("", text: $draft, prompt: Text("Message").foregroundStyle(Mint.dim), axis: .vertical)
                .id(field)
                .font(.body(14))
                .foregroundStyle(Mint.ink)
                .tint(Mint.ink)
                .lineLimit(1...6)
                .focused($typing)
                .padding(.horizontal, Mint.pageInset)
                .padding(.vertical, 10)
                .frame(minHeight: 50)
                .chunky()
                .blocksChatSwipe()
            Button(action: send) { Image(systemName: "paperplane") }
                .buttonStyle(ChunkyButton(fill: Mint.accent, size: 50, radius: 18))
                .opacity(empty ? 0.45 : 1)
                .disabled(empty)
                .accessibilityLabel("Send")
        }
        .padding(.leading, 14)
        .padding(.trailing, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .background(Mint.page)
    }

    private func send() {
        var text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if let replying {
            text = "> " + plainLine(quoteBody(replying.content).body).prefix(140) + "\n\n" + text
        }
        let out = Outgoing(text: text)
        outgoing.append(out)
        // A new text field, so iOS cannot put the sent text back (see the chat's `clearDraft`).
        let focused = typing
        draft = ""
        field += 1
        if focused { Task { @MainActor in typing = true } }
        replying = nil
        error = ""
        Task {
            do {
                try await model.sendToGroup(groupId, requestId: out.id, text: text)
                await load()
            } catch {
                self.error = describe(error)
                if draft.isEmpty { draft = text }
            }
            outgoing.removeAll { $0.id == out.id }
        }
    }

    private func reply(to message: GroupMessage) {
        replying = message
        typing = true
    }
}

extension GroupAuthor {
    var isYou: Bool {
        if case .member(_, true) = self { return true }
        return false
    }
}

/// A reply's quote: the "> " lines at the top, then the message.
private func quoteBody(_ content: String) -> (quote: String?, body: String) {
    var lines = content.split(separator: "\n", omittingEmptySubsequences: false)
    var quoted: [String] = []
    while let first = lines.first, first.hasPrefix(">") {
        quoted.append(String(first.dropFirst()).trimmingCharacters(in: .whitespaces))
        lines.removeFirst()
    }
    guard !quoted.isEmpty else { return (nil, content) }
    let body = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    return (quoted.joined(separator: " "), body)
}

private func authorColor(_ author: GroupAuthor, _ group: Group) -> Color {
    switch author {
    case .beep: Mint.brand
    case .member(_, true): Mint.muted
    case .member(let name, false): nameColors[memberIndex(group, name) % nameColors.count]
    }
}

/// Bubbles in a run share a name and tighter corners on the side they hang from:
/// yours on the right, Beep's reply as Beep writes it there, and other members on the left.
private struct GroupBubble: View {
    let group: Group
    let author: GroupAuthor
    let content: String
    var pictures: [Picture] = []
    let first: Bool
    let last: Bool
    let flash: Bool
    /// Who wrote a quoted text, found among the loaded messages.
    let quoteAuthor: (String) -> GroupAuthor?
    let quoted: (String) -> Void
    let reply: () -> Void

    var body: some View {
        let parts = quoteBody(content)
        switch author {
        case .member(_, true):
            ownBody(parts)
            .chunky(Mint.user, shape: UnevenRoundedRectangle(
                topLeadingRadius: 18, bottomLeadingRadius: 18, bottomTrailingRadius: last ? 4 : 6,
                topTrailingRadius: first ? 18 : 6))
            .contextMenu { menu }
            .padding(.leading, 48)
        case .beep:
            VStack(alignment: .leading, spacing: 4) {
                if first { Text("Beep").font(.body(12, .semibold)).foregroundStyle(Mint.brand).padding(.leading, 4) }
                if let quote = parts.quote { chip(quote) }
                MarkdownText(text: withMentions(parts.body))
            }
            .padding(4)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(flash ? Mint.tints[2] : .clear))
            .padding(-4)
            .contextMenu { menu }
            .frame(maxWidth: .infinity, alignment: .leading)
        case .member(let name, false):
            VStack(alignment: .leading, spacing: 2) {
                if first {
                    Text(name).font(.body(12, .semibold))
                        .foregroundStyle(nameColors[memberIndex(group, name) % nameColors.count])
                }
                if let quote = parts.quote { chip(quote) }
                if !pictures.isEmpty { PictureGrid(pictures: pictures).frame(width: 212) }
                if !parts.body.isEmpty { MarkdownText(text: withMentions(parts.body)) }
            }
            .padding(.horizontal, 12).padding(.top, 7).padding(.bottom, 9)
            .chunky(flash ? Mint.tints[2] : Mint.surface, shape: UnevenRoundedRectangle(
                topLeadingRadius: first ? 18 : 6, bottomLeadingRadius: last ? 4 : 6, bottomTrailingRadius: 18,
                topTrailingRadius: 18))
            .contextMenu { menu }
            .padding(.trailing, 48)
        }
    }

    /// Your text under any quote, with any pictures above it as in a one to one chat.
    @ViewBuilder private func ownBody(_ parts: (quote: String?, body: String)) -> some View {
        if pictures.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                if let quote = parts.quote { chip(quote) }
                MarkdownText(text: withMentions(parts.body))
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                PictureGrid(pictures: pictures)
                if let quote = parts.quote { chip(quote).padding(.horizontal, 6) }
                if !parts.body.isEmpty {
                    MarkdownText(text: withMentions(parts.body))
                        .padding(.horizontal, 10).padding(.top, 8).padding(.bottom, 6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(4)
            .frame(width: 220)
        }
    }

    /// The quoted message: who said it and its first line. Tapping jumps to it.
    private func chip(_ quote: String) -> some View {
        let source = quoteAuthor(quote)
        return Button { quoted(quote) } label: {
            VStack(alignment: .leading, spacing: 0) {
                if let source {
                    Text(authorName(source)).font(.body(12, .semibold)).foregroundStyle(authorColor(source, group))
                }
                Text(quote).font(.body(12.5)).foregroundStyle(Mint.muted).lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.leading, 9).padding(.trailing, 9).padding(.vertical, 4)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
            .overlay(alignment: .leading) {
                UnevenRoundedRectangle(topLeadingRadius: 8, bottomLeadingRadius: 8).fill(Mint.brand).frame(width: 3)
            }
        }
        .buttonStyle(.plain)
        .padding(.top, 2).padding(.bottom, 6)
        .accessibilityLabel("Go to the quoted message")
    }

    @ViewBuilder private var menu: some View {
        Button(action: reply) { Label("Reply", systemImage: "arrowshape.turn.up.left") }
        Button { UIPasteboard.general.string = content } label: { Label("Copy", systemImage: "doc.on.doc") }
    }
}

/// Lists Beep and the other members while an @word ends the draft, and completes it on tap.
private struct MentionPicker: View {
    let group: Group
    @Binding var draft: String

    private struct Choice: Identifiable {
        let name: String
        let note: String
        let beep: Bool
        var id: String { name }
    }

    var body: some View {
        if let typed = typedMention {
            let choices = ([Choice(name: "beep", note: "Ask Beep to reply here", beep: true)]
                + group.members.filter { !$0.you }.map { Choice(name: $0.name, note: "Member", beep: false) })
                .filter { $0.name.lowercased().hasPrefix(typed.lowercased()) }
            if !choices.isEmpty {
                VStack(spacing: 0) {
                    ForEach(choices) { choice in
                        Button { pick(choice.name, typed: typed) } label: {
                            HStack(spacing: 12) {
                                GroupAvatar(letter: choice.beep ? "" : initial(choice.name),
                                            color: choice.beep ? Mint.brand : memberTint(memberIndex(group, choice.name)),
                                            size: 30, beep: choice.beep)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(choice.beep ? "Beep" : choice.name).font(.body(14, choice.beep ? .semibold : .medium))
                                        .foregroundStyle(Mint.ink)
                                    Text(choice.note).font(.body(12)).foregroundStyle(Mint.muted)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 14).padding(.vertical, 8)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 6)
                .card()
                .padding(.horizontal, 14)
            }
        }
    }

    /// The word after the last @ when the draft ends in it and it starts a word.
    private var typedMention: String? {
        guard let at = draft.lastIndex(of: "@") else { return nil }
        if at > draft.startIndex, !draft[draft.index(before: at)].isWhitespace { return nil }
        let word = draft[draft.index(after: at)...]
        return word.contains(where: \.isWhitespace) ? nil : String(word)
    }

    private func pick(_ name: String, typed: String) {
        draft = String(draft.dropLast(typed.count + 1)) + "@\(name) "
    }
}

// MARK: Sheets

struct SheetHeader: View {
    let icon: String
    let tint: Color
    let title: String
    let subtitle: String

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon).font(.system(size: 22)).foregroundStyle(Mint.ink)
                .frame(width: 52, height: 52)
                .background(RoundedRectangle(cornerRadius: Mint.radius).fill(tint))
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.display(18)).foregroundStyle(Mint.ink)
                Text(subtitle).font(.body(13)).foregroundStyle(Mint.muted).lineLimit(1)
            }
            Spacer(minLength: 0)
        }
    }
}

private struct NameField: View {
    let label: String
    @Binding var text: String
    var limit = 30
    var error: String?
    var hint: String?
    var mono = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.body(12, .medium)).foregroundStyle(Mint.muted)
            TextField("", text: $text)
                .font(mono ? .system(size: 18, weight: .medium, design: .monospaced) : .body(14))
                .foregroundStyle(Mint.ink)
                .tint(Mint.ink)
                .textInputAutocapitalization(mono ? .characters : .words)
                .autocorrectionDisabled(mono)
                .padding(.horizontal, 14)
                .frame(minHeight: 50)
                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(error == nil ? Mint.soft : Mint.error, lineWidth: 1.5))
                .onChange(of: text) { if text.count > limit { text = String(text.prefix(limit)) } }
            if let note = error ?? hint {
                Text(note).font(.body(12)).foregroundStyle(error == nil ? Mint.muted : Mint.error)
            }
        }
    }
}

/// The sheets' frame: a surface with the content padded, sized to fit.
struct SheetBody<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) { content }
                .padding(20)
        }
        .scrollBounceBehavior(.basedOnSize)
        .background(Mint.surface)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationBackground(Mint.surface)
    }
}

/// Makes a group with the user in it, then hands it over so its invite link can be sent.
struct NewGroupSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let created: (Group) -> Void
    let joinCode: () -> Void
    @State private var name = ""
    @State private var memberName = lastMemberName
    @State private var busy = false
    @State private var error = ""

    var body: some View {
        SheetBody {
            SheetHeader(icon: "person.3", tint: Mint.tints[2], title: "New group",
                        subtitle: "Chat with people, and Beep helps when it can")
            NameField(label: "Group name", text: $name, limit: 60)
            NameField(label: "Your name in the group", text: $memberName)
            Text("You get a link to send to people. Anyone with Beep who opens it can join.")
                .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(4)
            if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
            HStack(spacing: 8) {
                Button("Join with a link", action: joinCode).buttonStyle(TextAction()).padding(.leading, -8)
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(TextAction(color: Mint.muted))
                Button("Create", action: create)
                    .buttonStyle(PillButton(fill: Mint.accent))
                    .disabled(busy || blank(name) || blank(memberName))
                    .opacity(busy || blank(name) || blank(memberName) ? 0.5 : 1)
            }
        }
    }

    private func create() {
        busy = true
        Task {
            do {
                let group = try await model.createGroup(name: trim(name), memberName: trim(memberName))
                lastMemberName = memberName
                created(group)
            } catch {
                self.error = describe(error)
                busy = false
            }
        }
    }
}

/// The group's invite link and code, to copy, send or replace.
struct GroupInviteSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let groupId: String
    let groupName: String
    @State private var link: GroupLink?
    @State private var reset = false
    @State private var copied = false
    @State private var error = ""

    private var code: String? { link.map { formatGroupCode(code: $0.token) } }

    var body: some View {
        SheetBody {
            SheetHeader(icon: "link", tint: Mint.tints[1], title: "Invite people", subtitle: groupName)
            SwiftUI.Group {
                if let link { QRCode(text: link.url) } else { Color.clear }
            }
            .frame(width: 156, height: 156)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
            .frame(maxWidth: .infinity)
            SwiftUI.Group {
                if let code {
                    Text(code).font(.system(size: 26, weight: .semibold, design: .monospaced)).tracking(3)
                        .foregroundStyle(Mint.ink).lineLimit(1).textSelection(.enabled)
                } else {
                    ProgressView()
                }
            }
            .frame(maxWidth: .infinity, minHeight: 56)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.page))
            HStack(spacing: 10) {
                Button {
                    UIPasteboard.general.string = link?.url
                    copied = true
                } label: {
                    Label(copied ? "Copied" : "Copy link", systemImage: copied ? "checkmark" : "doc.on.doc")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(PillButton())
                if let link, let code {
                    ShareLink(item: "Join \(groupName) on Beep: \(link.url)\nOr open Beep, start a new group, tap Join with a link and enter \(code).") {
                        Label("Send link", systemImage: "square.and.arrow.up").frame(maxWidth: .infinity)
                    }
                    .buttonStyle(PillButton(fill: Mint.accent))
                }
            }
            .disabled(link == nil)
            Text((reset ? "New code made. The old one no longer works. " : "")
                + "People open the link or scan the code, or enter the code under New group, Join with a link, and see the whole chat. "
                + "Beep uses what it knows about each of you to help, but never shows one person's memories to the others.")
                .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(4)
            if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
            HStack {
                Button("Reset code") { Task { await load(reset: true) } }
                    .buttonStyle(TextAction(color: Mint.error)).padding(.leading, -8)
                    .disabled(link == nil)
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(TextAction())
            }
        }
        .task { await load(reset: false) }
    }

    private func load(reset: Bool) async {
        do {
            link = try await model.groupInvite(groupId, reset: reset)
            if reset { self.reset = true }
            copied = false
            error = ""
        } catch {
            self.error = describe(error)
        }
    }
}

/// Entering an invite link or code someone sent. Case, spaces and dashes do not matter.
struct JoinCodeSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let found: (String) -> Void
    @State private var typed = ""
    @State private var wrong = false
    @State private var busy = false
    @State private var error = ""
    @State private var scanning = false

    /// A pasted link gives its token; a typed code is read as one.
    private var code: String? {
        let text = trim(typed)
        return groupCode(input: groupToken(url: text) ?? text)
    }

    var body: some View {
        SheetBody {
            SheetHeader(icon: "person.3", tint: Mint.tints[1], title: "Join a group",
                        subtitle: "Paste the link or code someone sent you")
            NameField(label: "Invite link or code", text: $typed, limit: 120,
                      error: wrong ? "No group has this code. Check it, or ask for a new one." : nil,
                      hint: "Ten letters and digits, like K7QM4-X9TRP", mono: true)
                .onChange(of: typed) { wrong = false }
            HStack(spacing: 8) {
                Button {
                    typed = UIPasteboard.general.string ?? ""
                    if let code { find(code) }
                } label: {
                    Label("Paste link", systemImage: "doc.on.clipboard").frame(maxWidth: .infinity)
                }
                Button { scanning = true } label: {
                    Label("Scan code", systemImage: "qrcode.viewfinder").frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(PillButton())
            .disabled(busy)
            if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
            HStack(spacing: 8) {
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(TextAction(color: Mint.muted))
                Button("Find group") { if let code { find(code) } }
                    .buttonStyle(PillButton(fill: Mint.accent))
                    .disabled(busy || code == nil || wrong)
                    .opacity(busy || code == nil || wrong ? 0.5 : 1)
            }
        }
        .qrScanner(isPresented: $scanning) { text in
            typed = text
            if let code { find(code) }
        }
    }

    private func find(_ token: String) {
        busy = true
        error = ""
        Task {
            do {
                _ = try await model.peekGroup(token)
                found(token)
            } catch BeepError.NotFound {
                wrong = true
            } catch {
                self.error = describe(error)
            }
            busy = false
        }
    }
}

/// `text` as a QR code, sharp at any size.
struct QRCode: View {
    let text: String

    var body: some View {
        if let image = Self.render(text) {
            Image(uiImage: image).interpolation(.none).resizable().scaledToFit()
        }
    }

    private static func render(_ text: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage,
            let cg = CIContext().createCGImage(output, from: output.extent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}

// MARK: Join

private struct PrivacyLine: View {
    let icon: String
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).font(.system(size: 15)).foregroundStyle(Mint.brand).frame(width: 18)
            Text(text).font(.body(13)).foregroundStyle(Mint.ink).lineSpacing(3)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// Where an invite leads: the group's name and members, and a name to join as.
struct GroupJoinView: View {
    @Environment(AppModel.self) private var model
    let token: String
    let joined: (String) -> Void
    let back: () -> Void
    @State private var preview: GroupPreview?
    @State private var expired = false
    @State private var name = lastMemberName
    @State private var taken: String?
    @State private var busy = false
    @State private var error = ""

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                if expired {
                    TintIcon(system: "link.badge.plus", tint: Mint.tints[0], size: 64)
                    Text("This code no longer works").font(.display(24)).foregroundStyle(Mint.ink)
                        .multilineTextAlignment(.center)
                    Text("Whoever sent it reset the group's code. Ask them for a new one.")
                        .font(.body(14)).foregroundStyle(Mint.muted).multilineTextAlignment(.center)
                    Button("Back to Beep", action: back).buttonStyle(PrimaryButton())
                } else if let preview {
                    details(preview)
                } else if !error.isEmpty {
                    Text(error).font(.body(12)).foregroundStyle(Mint.error).padding(.top, 120)
                    Button("Try again") { Task { await peek() } }.buttonStyle(PrimaryButton())
                    Button("Not now", action: back).buttonStyle(TextAction())
                } else {
                    ProgressView().padding(.top, 120)
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, minHeight: 600)
        }
        .background(Mint.page)
        .task { await peek() }
    }

    @ViewBuilder private func details(_ group: GroupPreview) -> some View {
        HStack(spacing: -14) {
            ForEach(Array(group.members.prefix(4).enumerated()), id: \.offset) { index, member in
                GroupAvatar(letter: initial(member), color: memberTint(index), size: 56)
            }
            GroupAvatar(letter: "", color: Mint.brand, size: 56, beep: true)
        }
        VStack(spacing: 6) {
            Text("Join \(group.name)?").font(.display(24)).foregroundStyle(Mint.ink)
            Text((group.members + ["Beep"]).dropLast().joined(separator: ", ") + " and Beep are in it.")
                .font(.body(14)).foregroundStyle(Mint.muted)
        }
        .multilineTextAlignment(.center)
        VStack(alignment: .leading, spacing: 10) {
            PrivacyLine(icon: "brain", text: "Beep can use your memories to help the group. It never shows or quotes them to others.")
            PrivacyLine(icon: "shield", text: "Beep reads the messages to see when it can help. When it answers you, it runs with your apps and your account, never someone else's.")
            PrivacyLine(icon: "bell", text: "You get a notification for each new message.")
        }
        .padding(Mint.spacing)
        .card()
        NameField(label: "Your name in the group", text: $name,
                  error: taken.map { "Someone in the group already goes by \($0)" },
                  hint: "The others see this name next to your messages")
            .onChange(of: name) { taken = nil }
        if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
        Button("Join group", action: join)
            .buttonStyle(PrimaryButton())
            .disabled(busy || taken != nil || blank(name))
            .opacity(busy || taken != nil || blank(name) ? 0.5 : 1)
        Button("Not now", action: back).buttonStyle(TextAction())
    }

    private func peek() async {
        error = ""
        do {
            let found = try await model.peekGroup(token)
            if found.joined { joined(found.id) } else { preview = found }
        } catch BeepError.NotFound {
            expired = true
        } catch {
            self.error = describe(error)
        }
    }

    private func join() {
        busy = true
        error = ""
        Task {
            do {
                let group = try await model.joinGroup(token, memberName: trim(name))
                lastMemberName = name
                joined(group.id)
            } catch BeepError.Conflict {
                taken = trim(name)
            } catch BeepError.NotFound {
                expired = true
            } catch {
                self.error = describe(error)
            }
            busy = false
        }
    }
}

// MARK: Info

/// Who is in a group, its reminders, what Beep does with their memories, and leaving it.
struct GroupInfoView: View {
    @Environment(AppModel.self) private var model
    let groupId: String
    let back: () -> Void
    /// After leaving: back to the list.
    let left: () -> Void
    let mention: (String) -> Void
    let invite: (Group) -> Void
    @State private var fetched: Group?
    @State private var reminders: [GroupReminder]?
    @State private var confirmLeave = false
    @State private var member: GroupMember?
    @State private var error = ""

    private var group: Group? { model.groups.first { $0.id == groupId } ?? fetched }

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Group info", back: back) {
                if let group {
                    Button { invite(group) } label: {
                        Image(systemName: "link").font(.system(size: 18)).foregroundStyle(Mint.muted).frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Invite")
                }
            }
            ScrollView {
                if let group {
                    VStack(alignment: .leading, spacing: 10) { content(group) }
                        .padding(.horizontal, Mint.pageInset).padding(.bottom, 24)
                } else {
                    ProgressView().padding(.top, 40)
                }
            }
        }
        .background(Mint.page)
        .task(id: groupId) {
            fetched = try? await model.group(groupId).group
            reminders = (try? await model.groupReminders(groupId)) ?? reminders ?? []
        }
        .alert("Leave \(group?.name ?? "the group")?", isPresented: $confirmLeave) {
            Button("Cancel", role: .cancel) {}
            Button("Leave", role: .destructive, action: leave)
        } message: {
            Text((group?.members.count ?? 0) <= 1
                ? "You are the last member, so the group and its messages are deleted."
                : "You stop getting its messages. To come back, someone has to send you the code again.")
        }
        .sheet(item: $member) { member in
            if let group { memberSheet(member, group) }
        }
    }

    private func memberSheet(_ member: GroupMember, _ group: Group) -> some View {
        let mine = reminders?.filter { $0.setBy == member.name }.count ?? 0
        return VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 14) {
                GroupAvatar(letter: initial(member.name), color: memberTint(memberIndex(group, member.name)), size: 56)
                VStack(alignment: .leading, spacing: 2) {
                    Text(member.name).font(.display(18)).foregroundStyle(Mint.ink)
                    Text(joined(member)).font(.body(13)).foregroundStyle(Mint.muted)
                }
            }
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Label("Reminders set", systemImage: "clock").font(.body(13)).foregroundStyle(Mint.ink)
                    Spacer()
                    Text("\(mine)").font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                }
            }
            .padding(Mint.spacing)
            .card()
            Button {
                self.member = nil
                mention(member.name)
            } label: {
                Text("@ Mention \(member.name)").font(.body(14, .semibold)).frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButton())
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 18).padding(.top, 28)
        .background(Mint.page)
        .presentationDetents([.height(300)])
        .presentationDragIndicator(.visible)
        .presentationCornerRadius(28)
    }

    private func joined(_ member: GroupMember) -> String {
        let day = date(member.joinedAt).formatted(.dateTime.day().month(.abbreviated))
        return member.creator ? "Made the group on \(day)" : "Joined \(day)"
    }

    @ViewBuilder private func content(_ group: Group) -> some View {
        VStack(spacing: 6) {
            ChatAvatar(text: groupInitials(group.name), fill: Mint.tint(for: group.id), size: 84)
            Text(group.name).font(.display(18)).foregroundStyle(Mint.ink).multilineTextAlignment(.center)
            Text("Group · \(group.members.count + 1) members").font(.body(13)).foregroundStyle(Mint.muted)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 6).padding(.bottom, 4)
        section("Members")
        VStack(alignment: .leading, spacing: 12) {
            ForEach(group.members.sorted { $0.you && !$1.you }, id: \.name) { member in
                Button { if !member.you { self.member = member } } label: {
                    HStack(spacing: 12) {
                        GroupAvatar(letter: initial(member.name), color: memberTint(memberIndex(group, member.name)), size: 36)
                        VStack(alignment: .leading, spacing: 1) {
                            HStack(spacing: 6) {
                                Text(member.you ? "You" : member.name).font(.body(14, .medium)).foregroundStyle(Mint.ink)
                                if member.creator {
                                    Text("Made it").font(.body(10, .semibold)).foregroundStyle(Mint.ink)
                                        .padding(.horizontal, 6).padding(.vertical, 2)
                                        .background(Capsule().fill(Mint.tints[1]))
                                }
                            }
                            Text(member.you ? "\(member.name) · \(joined(member))" : joined(member))
                                .font(.body(12)).foregroundStyle(Mint.muted)
                        }
                        Spacer(minLength: 0)
                        if !member.you {
                            Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.dim)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            memberLine(GroupAvatar(letter: "", color: Mint.brand, size: 36, beep: true),
                       "Beep", "Joins in when it can help, or when someone writes @beep")
            Button { invite(group) } label: {
                Label("Invite with a link", systemImage: "link").frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButton())
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .card()
        section("Reminders")
        GroupRemindersCard(reminders: reminders, you: group.members.first { $0.you }?.name, delete: delete)
        if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
        section("Your apps")
        Toggle(isOn: Binding(get: { group.appsShared }, set: { shared in
            fetched?.appsShared = shared
            Task { await model.shareGroupApps(group.id, shared: shared) }
        })) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Share my apps with Beep here").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                Text(group.appsShared
                    ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first."
                    : "Beep uses your apps here only when you ask it yourself.")
                    .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3)
            }
        }
        .tint(Mint.ok)
        .padding(.horizontal, 14).padding(.vertical, 12)
        .card()
        section("Privacy")
        VStack(alignment: .leading, spacing: 10) {
            PrivacyLine(icon: "brain", text: "Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.")
            PrivacyLine(icon: "shield", text: "Beep's thinking and tool calls are hidden in groups. Members only see its replies.")
        }
        .padding(Mint.spacing)
        .card()
        Button { confirmLeave = true } label: {
            Label("Leave group", systemImage: "rectangle.portrait.and.arrow.right")
                .font(.body(14, .semibold)).foregroundStyle(Mint.error)
                .frame(maxWidth: .infinity, minHeight: 48)
                .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.error, lineWidth: 1.5))
        }
        .buttonStyle(.plain)
        .padding(.top, 12)
    }

    private func section(_ title: String) -> some View {
        Text(title).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            .padding(.leading, 4).padding(.top, 8)
    }

    private func memberLine(_ avatar: GroupAvatar, _ name: String, _ note: String) -> some View {
        HStack(spacing: 12) {
            avatar
            VStack(alignment: .leading, spacing: 1) {
                Text(name).font(.body(14, .medium)).foregroundStyle(Mint.ink)
                Text(note).font(.body(12)).foregroundStyle(Mint.muted)
            }
        }
    }

    private func delete(_ reminder: GroupReminder) {
        withAnimation(.snappy) { reminders?.removeAll { $0.id == reminder.id } }
        Task {
            do {
                try await model.deleteGroupReminder(groupId, reminder.id)
                error = ""
            } catch {
                self.error = describe(error)
                reminders = (try? await model.groupReminders(groupId)) ?? reminders
            }
        }
    }

    private func leave() {
        Task {
            do {
                try await model.leaveGroup(groupId)
                left()
            } catch {
                self.error = describe(error)
            }
        }
    }
}

/// The group's reminders, each with when it goes and who set it. Beep posts them into the chat.
private struct GroupRemindersCard: View {
    let reminders: [GroupReminder]?
    let you: String?
    let delete: (GroupReminder) -> Void
    private let ask = "Ask Beep in the chat to add one, like \"remind us at 10 to check out\"."

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let reminders {
                if reminders.isEmpty {
                    Text("No reminders yet. \(ask)").font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(3)
                } else {
                    ForEach(Array(reminders.enumerated()), id: \.element.id) { index, reminder in
                        row(reminder, tint: memberTint(index))
                    }
                    Text(ask).font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3)
                }
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
    }

    private func row(_ reminder: GroupReminder, tint: Color) -> some View {
        let time = parseInstant(reminder.schedule).map { capitalized(describeDay($0)) } ?? describeSchedule(reminder.schedule)
        let by = reminder.setBy == you ? "you" : reminder.setBy
        return HStack(spacing: 12) {
            TintIcon(system: "clock", tint: tint, size: 36)
            VStack(alignment: .leading, spacing: 1) {
                Text(reminder.label).font(.body(14, .medium)).foregroundStyle(Mint.ink)
                Text(reminder.message).font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(2)
                Text("\(time) · set by \(by)").font(.body(12)).foregroundStyle(Mint.muted)
            }
            Spacer(minLength: 0)
            Button { delete(reminder) } label: {
                Image(systemName: "xmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(Mint.muted)
                    .frame(width: 36, height: 36)
            }
            .accessibilityLabel("Delete reminder")
        }
    }
}

private func capitalized(_ text: String) -> String { text.prefix(1).uppercased() + text.dropFirst() }
private func trim(_ text: String) -> String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
private func blank(_ text: String) -> Bool { trim(text).isEmpty }

extension GroupMember: Identifiable {
    public var id: String { name }
}
