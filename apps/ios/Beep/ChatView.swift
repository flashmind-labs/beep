import SwiftUI

struct ChatView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var draft = ""
    @FocusState private var typing: Bool

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if model.chat.messages.isEmpty {
                            EmptyChat { model.send($0) }
                        }
                        ForEach(model.chat.messages) { message in
                            MessageView(message: message, live: message.id == "assistant-\(model.chat.runId)")
                        }
                        if model.chat.busy, !model.chat.thinking.isEmpty {
                            ThinkingPreview(text: model.chat.thinking)
                        }
                        if model.chat.busy, waitingForFirstEvent {
                            ProgressView().padding(.leading, 4)
                        }
                        if !model.chat.error.isEmpty, !model.chat.busy {
                            HStack(spacing: 10) {
                                Text(model.chat.error).font(.body(13)).foregroundStyle(Mint.error)
                                Button("Retry") { model.retry() }.buttonStyle(PillButton())
                            }
                        } else if !model.chat.error.isEmpty {
                            Text(model.chat.error).font(.body(13)).foregroundStyle(Mint.muted)
                        }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                }
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: model.chat.messages) { scroller.scrollTo("end", anchor: .bottom) }
                .onChange(of: model.chat.id) { scroller.scrollTo("end", anchor: .bottom) }
            }
            composer
        }
        .background(Mint.page)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Mint.page, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button { path.append(.chats) } label: { Image(systemName: "line.3.horizontal") }
                    .accessibilityLabel("Chats")
            }
            ToolbarItem(placement: .principal) {
                VStack(spacing: 1) {
                    Text("Beep").font(.display(20)).foregroundStyle(Mint.ink)
                    HStack(spacing: 5) {
                        Circle().fill(model.connected ? Mint.ok : Mint.error).frame(width: 6, height: 6)
                        Text(model.connected ? "Connected" : "Reconnecting")
                            .font(.body(11, .medium)).foregroundStyle(Mint.muted)
                    }
                    .onTapGesture { if !model.connected { model.connect() } }
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { model.newChat() } label: { Image(systemName: "square.and.pencil") }
                    .accessibilityLabel("New chat")
            }
        }
    }

    private var waitingForFirstEvent: Bool {
        guard let last = model.chat.messages.last else { return true }
        return last.role == .user
    }

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField("Message", text: $draft, axis: .vertical)
                .font(.body(16))
                .lineLimit(1...6)
                .focused($typing)
                .padding(.horizontal, 16)
                .padding(.vertical, 11)
                .background(RoundedRectangle(cornerRadius: 22).fill(Mint.surface))
                .overlay(RoundedRectangle(cornerRadius: 22).stroke(Mint.soft, lineWidth: 1.5))
            if model.chat.busy {
                Button { model.stop() } label: {
                    RoundedRectangle(cornerRadius: 3).fill(Mint.ink).frame(width: 12, height: 12)
                        .frame(width: 44, height: 44)
                        .chunky(Mint.surface, radius: 22, drop: 2)
                }
                .accessibilityLabel("Stop")
            } else {
                let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                Button {
                    model.send(draft)
                    draft = ""
                } label: {
                    Image(systemName: "arrow.up").font(.system(size: 18, weight: .bold))
                        .foregroundStyle(empty ? Mint.dim : Mint.ink)
                        .frame(width: 44, height: 44)
                        .chunky(empty ? Mint.soft : Mint.accent, radius: 22, drop: empty ? 0 : 2)
                }
                .disabled(empty)
                .accessibilityLabel("Send")
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 8)
        .padding(.bottom, 6)
        .background(Mint.page)
    }
}

private struct EmptyChat: View {
    @Environment(AppModel.self) private var model
    let send: (String) -> Void

    private static let templates = [
        PromptSuggestion(title: "Remember something", prompt: "Remember that I prefer short, practical answers."),
        PromptSuggestion(title: "Plan my Saturday", prompt: "What's the weather for Saturday and anything on my calendar?"),
        PromptSuggestion(title: "What's nearby", prompt: "What's a good place for coffee near me?"),
    ]
    private static let icons = ["brain", "clock", "magnifyingglass"]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Hey. What can I help with?")
                .font(.display(32)).foregroundStyle(Mint.ink)
                .padding(.top, 20)
            Text("Find an answer. Untangle a thought.\nKeep something worth remembering.")
                .font(.body(15)).foregroundStyle(Mint.muted).lineSpacing(4)
                .padding(.bottom, 12)
            let items = model.starters.isEmpty ? Self.templates : Array(model.starters.prefix(3))
            ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                Button { send(item.prompt) } label: {
                    HStack(spacing: 12) {
                        TintIcon(system: Self.icons[index % 3], tint: Mint.tints[index % 3], size: 36)
                        Text(item.title).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                            .multilineTextAlignment(.leading)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 14).padding(.vertical, 12)
                    .chunky()
                }
                .buttonStyle(.plain)
            }
            if !model.starters.isEmpty {
                Button {
                    Task { await model.loadStarters(refresh: true) }
                } label: {
                    Label("New ideas", systemImage: "arrow.clockwise").font(.body(13))
                }
                .foregroundStyle(Mint.muted)
                .frame(maxWidth: .infinity)
                .padding(.top, 4)
            }
        }
    }
}

private struct ThinkingPreview: View {
    let text: String

    var body: some View {
        Text(String(text.suffix(240)))
            .font(.body(13)).foregroundStyle(Mint.muted)
            .lineLimit(3)
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 10).fill(Mint.surface))
    }
}

struct MessageView: View {
    @Environment(AppModel.self) private var model
    let message: Message
    let live: Bool
    @State private var showThoughts = false
    @State private var showAllTools = false

    var body: some View {
        switch message.role {
        case .user:
            HStack {
                Spacer(minLength: 60)
                Text(message.text)
                    .font(.body(15)).foregroundStyle(Mint.ink)
                    .textSelection(.enabled)
                    .padding(.horizontal, 14).padding(.vertical, 10)
                    .background(bubble.fill(Mint.user))
                    .overlay(bubble.stroke(Mint.ledge, lineWidth: 1.5))
            }
        case .scheduled:
            Label(message.text, systemImage: "clock")
                .font(.body(13, .medium)).foregroundStyle(Mint.muted)
                .frame(maxWidth: .infinity)
        case .assistant:
            VStack(alignment: .leading, spacing: 8) {
                if !message.reasoning.isEmpty {
                    Button { withAnimation(.snappy) { showThoughts.toggle() } } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "chevron.down").rotationEffect(.degrees(showThoughts ? 0 : -90))
                            Text("Thoughts")
                        }
                        .font(.body(12, .medium)).foregroundStyle(Mint.muted)
                    }
                    if showThoughts {
                        MarkdownText(text: message.reasoning, size: 13, color: Mint.muted)
                            .padding(10)
                            .background(RoundedRectangle(cornerRadius: 10).fill(Mint.surface))
                    }
                }
                tools
                if !message.text.isEmpty {
                    MarkdownText(text: message.text)
                }
                ForEach(message.approvals, id: \.queryId) { prompt in
                    ApprovalCard(prompt: prompt)
                }
            }
        }
    }

    private var bubble: UnevenRoundedRectangle {
        UnevenRoundedRectangle(topLeadingRadius: 18, bottomLeadingRadius: 18, bottomTrailingRadius: 4, topTrailingRadius: 18)
    }

    /// While a run streams only the last three calls show; older ones fold into one line.
    @ViewBuilder private var tools: some View {
        let calls = message.tools
        let folded = showAllTools ? 0 : max(0, calls.count - (live ? 3 : 0))
        if folded > 0 {
            Button { withAnimation(.snappy) { showAllTools = true } } label: {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.right")
                    let failed = calls.prefix(folded).filter { $0.done && !$0.success }.count
                    Text("\(folded) \(folded == 1 ? "tool" : "tools")" + (failed > 0 ? ", \(failed) failed" : ""))
                }
                .font(.body(12, .medium)).foregroundStyle(Mint.muted)
            }
        }
        ForEach(calls.dropFirst(folded)) { call in
            ToolChip(call: call)
        }
    }
}

struct ToolChip: View {
    let call: ToolCall

    var body: some View {
        HStack(spacing: 8) {
            Text(toolInitial(call.name))
                .font(.body(11, .semibold))
                .frame(width: 26, height: 26)
                .background(Circle().fill(Mint.surface))
            Text(call.label).font(.body(13, .medium)).lineLimit(1)
            Group {
                if !call.done {
                    ProgressView().controlSize(.mini)
                } else if call.success {
                    Image(systemName: "checkmark").foregroundStyle(Mint.ok)
                } else {
                    Image(systemName: "xmark").foregroundStyle(Mint.error)
                }
            }
            .font(.system(size: 12, weight: .bold))
            .frame(width: 18, height: 18)
        }
        .foregroundStyle(Mint.ink)
        .padding(.leading, 5).padding(.trailing, 12).padding(.vertical, 5)
        .background(Capsule().fill(call.done ? Mint.tints[1] : Mint.tints[2]))
        .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1.5))
    }
}

struct ApprovalCard: View {
    @Environment(AppModel.self) private var model
    let prompt: ApprovalPrompt

    var body: some View {
        if let answer = prompt.answer {
            Label(answerText(answer), systemImage: "shield")
                .font(.body(12, .medium)).foregroundStyle(Mint.muted)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    Text(String(prompt.approval.app.prefix(1)).uppercased())
                        .font(.body(14, .semibold))
                        .frame(width: 32, height: 32)
                        .background(RoundedRectangle(cornerRadius: 9).fill(Mint.surface))
                        .overlay(RoundedRectangle(cornerRadius: 9).stroke(Mint.ledge, lineWidth: 1.5))
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(prompt.approval.action) with \(prompt.approval.app)?").font(.body(15, .semibold))
                        Text("\(prompt.approval.app) asks before it does this").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Mint.tints[2])
                Rectangle().fill(Mint.ledge).frame(height: 1.5)
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 4) {
                    ForEach(Array(prompt.approval.fields.enumerated()), id: \.offset) { _, field in
                        GridRow {
                            Text(field.label).foregroundStyle(Mint.muted)
                            Text(field.value).lineLimit(6)
                        }
                    }
                }
                .font(.body(13))
                .padding(.horizontal, 14).padding(.top, 12)
                HStack(spacing: 8) {
                    Button("Allow") { model.answer(prompt, .allow) }
                        .font(.body(14, .semibold)).foregroundStyle(.white)
                        .padding(.horizontal, 20).frame(minHeight: 44)
                        .chunky(Mint.ok, radius: 12)
                    Button("Always") { model.answer(prompt, .always) }.buttonStyle(PillButton())
                    Spacer()
                    Button("Deny") { model.answer(prompt, .deny) }
                        .font(.body(14, .medium)).foregroundStyle(Mint.error)
                }
                .padding(14)
            }
            .foregroundStyle(Mint.ink)
            .chunky(radius: 16)
        }
    }

    private func answerText(_ answer: ApprovalDecision) -> String {
        switch answer {
        case .allow: "Allowed once"
        case .always: "Always allowed for \(prompt.approval.app)"
        case .deny: "Denied"
        }
    }
}

/// Replies as markdown: paragraphs, bullets and headings, with inline styling.
struct MarkdownText: View {
    let text: String
    var size: CGFloat = 15
    var color: Color = Mint.ink

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .bullet(let line):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•")
                        inline(line)
                    }
                case .heading(let line):
                    inline(line).fontWeight(.bold)
                case .code(let code):
                    Text(code)
                        .font(.system(size: size - 2, design: .monospaced))
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 10).fill(Mint.tints[1]))
                case .paragraph(let line):
                    inline(line)
                }
            }
        }
        .font(.body(size))
        .foregroundStyle(color)
        .lineSpacing(3)
        .textSelection(.enabled)
    }

    private enum Block {
        case paragraph(String), bullet(String), heading(String), code(String)
    }

    private var blocks: [Block] {
        var out: [Block] = []
        var code: [String]?
        for raw in text.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("```") {
                if let lines = code {
                    out.append(.code(lines.joined(separator: "\n")))
                    code = nil
                } else {
                    code = []
                }
                continue
            }
            if code != nil {
                code?.append(raw)
                continue
            }
            if line.isEmpty { continue }
            if line.hasPrefix("- ") || line.hasPrefix("* ") {
                out.append(.bullet(String(line.dropFirst(2))))
            } else if line.hasPrefix("#") {
                out.append(.heading(line.drop(while: { $0 == "#" }).trimmingCharacters(in: .whitespaces)))
            } else {
                out.append(.paragraph(line))
            }
        }
        if let lines = code { out.append(.code(lines.joined(separator: "\n"))) }
        return out
    }

    private func inline(_ line: String) -> Text {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        if let styled = try? AttributedString(markdown: line, options: options) { return Text(styled) }
        return Text(line)
    }
}
