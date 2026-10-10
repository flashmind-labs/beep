import CoreImage.CIFilterBuiltins
import SwiftUI

/// The name text of members, by the order they joined; their avatars use the tints.
private let nameColors = [Color(hex: 0xB8574B), Color(hex: 0x2F7D5B), Color(hex: 0x8A6A12)]

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
    group.members.firstIndex { $0.name.caseInsensitiveCompare(name) == .orderedSame } ?? abs(name.hashValue)
}

private func memberTint(_ index: Int) -> Color { Mint.tints[index % Mint.tints.count] }

private func initial(_ name: String) -> String { String(name.prefix(1)).uppercased() }

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
        Text(letter)
            .font(.system(size: size * 0.4, weight: .bold))
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
    @Binding var drawer: Bool
    let info: () -> Void
    let invite: (Group) -> Void
    @State private var chat: GroupChat?
    @State private var outgoing: [Outgoing] = []
    @State private var draft = ""
    @State private var error = ""
    @FocusState private var typing: Bool

    private var group: Group? { chat?.group ?? model.groups.first { $0.id == groupId } }

    var body: some View {
        VStack(spacing: 0) {
            topBar
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        messages
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(18)
                }
                .overlay { placeholder }
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: chat?.messages.count) { scroller.scrollTo("end", anchor: .bottom) }
                .onChange(of: outgoing.count) { scroller.scrollTo("end", anchor: .bottom) }
                .onChange(of: chat?.group.beepTyping) { scroller.scrollTo("end", anchor: .bottom) }
            }
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 20)
            }
            if let group { MentionPicker(group: group, draft: $draft) }
            composer
        }
        .background(Mint.page)
        .toolbar(.hidden, for: .navigationBar)
        // A notice would say when something changed; until the iPhone gets them, this polls.
        .task(id: groupId) {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(chat?.group.beepTyping == true ? 2 : 4))
            }
        }
    }

    private func load() async {
        do {
            chat = try await model.group(groupId)
        } catch BeepError.NotFound {
            model.newChat()
        } catch {}
    }

    @ViewBuilder private var placeholder: some View {
        if chat == nil {
            ProgressView()
        } else if chat?.messages.isEmpty == true, outgoing.isEmpty {
            Text("No messages yet.\nSay hi. Beep joins in when it can help.")
                .font(.body(15)).foregroundStyle(Mint.muted).lineSpacing(6)
                .multilineTextAlignment(.center).padding(32)
        }
    }

    @ViewBuilder private var messages: some View {
        if let chat {
            let days = dayStarts(chat.messages)
            ForEach(chat.messages, id: \.seq) { message in
                VStack(spacing: 12) {
                    if let day = days[message.seq] {
                        Text(day).font(.body(11)).foregroundStyle(Mint.dim).frame(maxWidth: .infinity)
                    }
                    GroupBubble(group: chat.group, author: message.author, content: message.content, quote: quote)
                }
            }
            ForEach(outgoing) { out in
                GroupBubble(group: chat.group, author: .member(name: "", you: true), content: out.text, quote: quote)
                    .opacity(0.6)
            }
            if chat.group.beepTyping {
                HStack(spacing: 10) {
                    ProgressView().controlSize(.small)
                    Text("Beep is typing").font(.body(13)).foregroundStyle(Mint.muted)
                }
                .padding(.leading, 4)
            }
        }
    }

    /// The first message of each day, with that day's label.
    private func dayStarts(_ messages: [GroupMessage]) -> [Int64: String] {
        var labels: [Int64: String] = [:]
        var current: Date?
        for message in messages {
            let day = Calendar.current.startOfDay(for: date(message.timestamp))
            if day != current { labels[message.seq] = dayLabel(day) }
            current = day
        }
        return labels
    }

    private var topBar: some View {
        HStack(spacing: 10) {
            Button {
                typing = false
                withAnimation(.snappy) { drawer = true }
            } label: { Image(systemName: "line.3.horizontal") }
                .buttonStyle(ChunkyButton())
                .accessibilityLabel("Conversations")
            Button(action: info) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(group?.name ?? "Group").font(.display(24)).foregroundStyle(Mint.ink).lineLimit(1)
                    if let group {
                        HStack(spacing: 6) {
                            HStack(spacing: -6) {
                                ForEach(Array(group.members.sorted { $0.you && !$1.you }.prefix(4)), id: \.name) { member in
                                    GroupAvatar(letter: initial(member.name), color: memberTint(memberIndex(group, member.name)), size: 18)
                                }
                                GroupAvatar(letter: "b", color: Mint.brand, size: 18, beep: true)
                            }
                            Text(groupPeople(group)).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                        }
                    }
                }
            }
            .buttonStyle(.plain)
            .disabled(group == nil)
            Spacer(minLength: 0)
            Button { if let group { invite(group) } } label: {
                Image(systemName: "link").font(.system(size: 18)).foregroundStyle(Mint.muted).frame(width: 40, height: 40)
            }
            .disabled(group == nil)
            .accessibilityLabel("Invite")
        }
        .padding(.leading, 12)
        .padding(.trailing, 8)
        .padding(.vertical, 8)
        .background(Mint.page)
    }

    private var composer: some View {
        let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        return HStack(alignment: .bottom, spacing: 8) {
            TextField("", text: $draft, prompt: Text("Message").foregroundStyle(Mint.dim), axis: .vertical)
                .font(.body(15))
                .foregroundStyle(Mint.ink)
                .tint(Mint.ink)
                .lineLimit(1...6)
                .focused($typing)
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
                .frame(minHeight: 50)
                .chunky()
            Button(action: send) { Image(systemName: "paperplane") }
                .buttonStyle(ChunkyButton(fill: empty ? Mint.surface : Mint.accent, size: 50, radius: 18))
                .opacity(empty ? 0.5 : 1)
                .disabled(empty)
                .accessibilityLabel("Send message")
        }
        .padding(.leading, 14)
        .padding(.trailing, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .background(Mint.page)
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        let out = Outgoing(text: text)
        outgoing.append(out)
        draft = ""
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

    /// Puts the quoted text above whatever is already typed.
    private func quote(_ text: String) {
        let lines = text.trimmingCharacters(in: .whitespacesAndNewlines)
            .split(separator: "\n", omittingEmptySubsequences: false).prefix(4)
        draft = lines.map { "> " + $0 }.joined(separator: "\n") + "\n\n" + draft.trimmingCharacters(in: .whitespacesAndNewlines)
        typing = true
    }
}

/// The same bubbles as a one to one chat: yours on the right, Beep's reply as Beep writes
/// it there, and other members on the left with their name on top.
private struct GroupBubble: View {
    let group: Group
    let author: GroupAuthor
    let content: String
    let quote: (String) -> Void

    var body: some View {
        switch author {
        case .member(_, true):
            MarkdownText(text: withMentions(content))
                .padding(.horizontal, 14).padding(.vertical, 10)
                .chunky(Mint.user, shape: UnevenRoundedRectangle(
                    topLeadingRadius: 18, bottomLeadingRadius: 18, bottomTrailingRadius: 6, topTrailingRadius: 18))
                .contextMenu { menu }
                .padding(.leading, 48)
                .frame(maxWidth: .infinity, alignment: .trailing)
        case .beep:
            VStack(alignment: .leading, spacing: 4) {
                Text("Beep").font(.body(12, .semibold)).foregroundStyle(Mint.brand).padding(.leading, 4)
                MarkdownText(text: withMentions(content))
                    .contextMenu { menu }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        case .member(let name, false):
            VStack(alignment: .leading, spacing: 4) {
                Text(name).font(.body(12, .semibold))
                    .foregroundStyle(nameColors[memberIndex(group, name) % nameColors.count])
                MarkdownText(text: withMentions(content))
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            .chunky(shape: UnevenRoundedRectangle(
                topLeadingRadius: 6, bottomLeadingRadius: 18, bottomTrailingRadius: 18, topTrailingRadius: 18))
            .contextMenu { menu }
            .padding(.trailing, 48)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder private var menu: some View {
        Button { UIPasteboard.general.string = content } label: { Label("Copy", systemImage: "doc.on.doc") }
        Button { quote(content) } label: { Label("Quote", systemImage: "quote.opening") }
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
                                GroupAvatar(letter: choice.beep ? "b" : initial(choice.name),
                                            color: choice.beep ? Mint.brand : memberTint(memberIndex(group, choice.name)),
                                            size: 30, beep: choice.beep)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(choice.beep ? "Beep" : choice.name).font(.body(15, choice.beep ? .semibold : .medium))
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
                .chunky()
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

private struct SheetHeader: View {
    let icon: String
    let tint: Color
    let title: String
    let subtitle: String

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon).font(.system(size: 22)).foregroundStyle(Mint.ink)
                .frame(width: 52, height: 52)
                .background(RoundedRectangle(cornerRadius: 14).fill(tint))
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.display(24)).foregroundStyle(Mint.ink)
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
                .font(mono ? .system(size: 18, weight: .medium, design: .monospaced) : .body(15))
                .foregroundStyle(Mint.ink)
                .tint(Mint.ink)
                .textInputAutocapitalization(mono ? .characters : .words)
                .autocorrectionDisabled(mono)
                .padding(.horizontal, 14)
                .frame(minHeight: 50)
                .background(RoundedRectangle(cornerRadius: 12).fill(Mint.page))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(error == nil ? Mint.soft : Mint.error, lineWidth: 1.5))
                .onChange(of: text) { if text.count > limit { text = String(text.prefix(limit)) } }
            if let note = error ?? hint {
                Text(note).font(.body(12)).foregroundStyle(error == nil ? Mint.muted : Mint.error)
            }
        }
    }
}

/// The sheets' frame: a surface with the content padded, sized to fit.
private struct SheetBody<Content: View>: View {
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
            .background(RoundedRectangle(cornerRadius: 12).fill(Mint.page))
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
            .background(RoundedRectangle(cornerRadius: 12).fill(Mint.page))
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
                Button("Reset code") { load(reset: true) }
                    .buttonStyle(TextAction(color: Mint.error)).padding(.leading, -8)
                    .disabled(link == nil)
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(TextAction())
            }
        }
        .task { load(reset: false) }
    }

    private func load(reset: Bool) {
        Task {
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
            Button {
                typed = UIPasteboard.general.string ?? ""
                if let code { find(code) }
            } label: {
                Label("Paste link", systemImage: "doc.on.clipboard").frame(maxWidth: .infinity)
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
private struct QRCode: View {
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
                    Text("This code no longer works").font(.display(30)).foregroundStyle(Mint.ink)
                        .multilineTextAlignment(.center)
                    Text("Whoever sent it reset the group's code. Ask them for a new one.")
                        .font(.body(15)).foregroundStyle(Mint.muted).multilineTextAlignment(.center)
                    Button("Back to Beep", action: back).buttonStyle(PrimaryButton())
                } else if let preview {
                    details(preview)
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
            GroupAvatar(letter: "b", color: Mint.brand, size: 56, beep: true)
        }
        VStack(spacing: 6) {
            Text("Join \(group.name)?").font(.display(32)).foregroundStyle(Mint.ink)
            Text((group.members + ["Beep"]).dropLast().joined(separator: ", ") + " and Beep are in it.")
                .font(.body(15)).foregroundStyle(Mint.muted)
        }
        .multilineTextAlignment(.center)
        VStack(alignment: .leading, spacing: 10) {
            PrivacyLine(icon: "brain", text: "Beep can use your memories to help the group. It never shows or quotes them to others.")
            PrivacyLine(icon: "shield", text: "Beep reads the messages to see when it can help. When it answers you, it runs with your apps and your account, never someone else's.")
            PrivacyLine(icon: "bell", text: "You get a notification for each new message.")
        }
        .padding(14)
        .chunky()
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
        do {
            let found = try await model.peekGroup(token)
            if found.joined { joined(found.id) } else { preview = found }
        } catch BeepError.NotFound {
            expired = true
        } catch {
            self.error = describe(error)
            expired = true
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
    let invite: (Group) -> Void
    @State private var fetched: Group?
    @State private var reminders: [GroupReminder]?
    @State private var confirmLeave = false
    @State private var error = ""

    private var group: Group? { model.groups.first { $0.id == groupId } ?? fetched }

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Group info", back: back)
            ScrollView {
                if let group {
                    VStack(alignment: .leading, spacing: 10) { content(group) }
                        .padding(.horizontal, 16).padding(.bottom, 24)
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
                : "You stop getting its messages. Someone in it can invite you again.")
        }
    }

    @ViewBuilder private func content(_ group: Group) -> some View {
        section("Members")
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(group.members.enumerated()), id: \.offset) { index, member in
                memberLine(GroupAvatar(letter: initial(member.name), color: memberTint(index), size: 36),
                           member.you ? "\(member.name) (you)" : member.name,
                           member.creator ? "Created the group"
                               : "Joined " + date(member.joinedAt).formatted(.dateTime.day().month(.abbreviated)))
            }
            memberLine(GroupAvatar(letter: "b", color: Mint.brand, size: 36, beep: true),
                       "Beep", "Joins in when it can help, or when someone writes @beep")
            Button { invite(group) } label: {
                Label("Invite with a link", systemImage: "link").frame(maxWidth: .infinity)
            }
            .buttonStyle(PillButton())
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .chunky()
        section("Reminders")
        GroupRemindersCard(reminders: reminders, you: group.members.first { $0.you }?.name, delete: delete)
        if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
        section("Your apps")
        Toggle(isOn: Binding(get: { group.appsShared }, set: { shared in
            fetched?.appsShared = shared
            Task { await model.shareGroupApps(group.id, shared: shared) }
        })) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Share my apps with Beep here").font(.body(15, .medium)).foregroundStyle(Mint.ink)
                Text(group.appsShared
                    ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first."
                    : "Beep uses your apps here only when you ask it yourself.")
                    .font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3)
            }
        }
        .tint(Mint.ok)
        .padding(.horizontal, 14).padding(.vertical, 12)
        .chunky()
        section("Privacy")
        VStack(alignment: .leading, spacing: 10) {
            PrivacyLine(icon: "brain", text: "Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.")
            PrivacyLine(icon: "shield", text: "Beep's thinking and tool calls are hidden in groups. Members only see its replies.")
        }
        .padding(14)
        .chunky()
        Button("Leave group") { confirmLeave = true }
            .buttonStyle(TextAction(color: Mint.error))
            .padding(.top, 4)
    }

    private func section(_ title: String) -> some View {
        Text(title).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            .padding(.leading, 4).padding(.top, 8)
    }

    private func memberLine(_ avatar: GroupAvatar, _ name: String, _ note: String) -> some View {
        HStack(spacing: 12) {
            avatar
            VStack(alignment: .leading, spacing: 1) {
                Text(name).font(.body(15, .medium)).foregroundStyle(Mint.ink)
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
                back()
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
        .chunky()
    }

    private func row(_ reminder: GroupReminder, tint: Color) -> some View {
        let time = parseInstant(reminder.schedule).map { capitalized(describeDay($0)) } ?? describeSchedule(reminder.schedule)
        let by = reminder.setBy == you ? "you" : reminder.setBy
        return HStack(spacing: 12) {
            TintIcon(system: "clock", tint: tint, size: 36)
            VStack(alignment: .leading, spacing: 1) {
                Text(reminder.label).font(.body(15, .medium)).foregroundStyle(Mint.ink)
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
