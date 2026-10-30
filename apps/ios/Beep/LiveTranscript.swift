import SwiftUI

/// Stretches of a chat said aloud in live mode on the Mac, shown as one transcript block each
/// instead of bubbles, as in the mock. The turns are stored as ordinary messages; only the look
/// changes. A stretch keeps going while each spoken prompt comes within ten minutes of the last.
struct SpokenStretches {
    /// Each stretch's messages, by the id of its first message, which draws the block.
    private(set) var byFirst: [String: [Message]] = [:]
    /// The other messages of every stretch, which draw nothing of their own.
    private(set) var hidden: Set<String> = []

    /// The longest pause between spoken prompts that still counts as one conversation.
    private static let gap: Int64 = 10 * 60 * 1000

    init(_ messages: [Message]) {
        var byFirst: [String: [Message]] = [:]
        var hidden: Set<String> = []
        var current: [Message] = []
        var lastPrompt: Int64 = 0

        func close() {
            if let first = current.first {
                byFirst[first.id] = current
                hidden.formUnion(current.dropFirst().map(\.id))
            }
            current = []
        }

        for message in messages {
            switch message.role {
            case .user:
                guard message.spoken, message.sentAt > 0 else {
                    close()
                    continue
                }
                if !current.isEmpty, message.sentAt - lastPrompt > Self.gap { close() }
                current.append(message)
                lastPrompt = message.sentAt
            case .assistant:
                // A card that still waits for an answer keeps its own place, so it can be answered.
                if !current.isEmpty, !message.approvals.contains(where: { $0.answer == nil && !$0.withdrawn }) {
                    current.append(message)
                } else {
                    close()
                }
            case .scheduled:
                close()
            }
        }
        close()
        self.byFirst = byFirst
        self.hidden = hidden
    }
}

/// One live conversation: who said what, with the steps Beep took between. The phone shows it
/// read only; live mode itself is on the Mac.
struct LiveTranscript: View {
    let messages: [Message]
    @State private var showAll = false

    /// How many lines show before "Show all".
    private static let collapsed = 4

    private enum Line {
        case said(you: Bool, at: Int64, text: String)
        case step(String)
    }

    private var lines: [Line] {
        messages.flatMap { message -> [Line] in
            switch message.role {
            case .user:
                return [.said(you: true, at: message.sentAt, text: message.text)]
            case .assistant:
                let steps = message.tools.filter { !quietTools.contains($0.name) }.map { Line.step(doneLabel($0.label)) }
                let text = message.text.trimmingCharacters(in: .whitespacesAndNewlines)
                return steps + (text.isEmpty ? [] : [.said(you: false, at: message.repliedAt, text: text)])
            case .scheduled:
                return []
            }
        }
    }

    private var start: Int64 { messages.first?.sentAt ?? 0 }

    private var end: Int64 {
        messages.map { max($0.sentAt, $0.repliedAt) }.max() ?? start
    }

    var body: some View {
        let all = lines
        let cut = !showAll && all.count > Self.collapsed
        let shown = cut ? Array(all.prefix(Self.collapsed)) : all
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "mic").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.accent)
                Text("Live conversation").font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                Text("\(clock(start)) · \(length)").font(.body(13)).foregroundStyle(Mint.muted)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            Rectangle().fill(Mint.ledge).frame(height: Mint.outline)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(shown.indices, id: \.self) { index in row(shown[index]) }
            }
            .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, all.count > Self.collapsed ? 6 : 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .overlay(alignment: .bottom) {
                if cut {
                    LinearGradient(colors: [Mint.surface.opacity(0), Mint.surface], startPoint: .top, endPoint: .bottom)
                        .frame(height: 32)
                        .allowsHitTesting(false)
                }
            }
            if all.count > Self.collapsed {
                Button(showAll ? "Show less" : "Show all \(all.count) lines") {
                    withAnimation(.smooth) { showAll.toggle() }
                }
                .font(.body(13)).foregroundStyle(Mint.muted)
                .padding(.horizontal, 14).padding(.top, 6).padding(.bottom, 12)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Live conversation")
    }

    @ViewBuilder private func row(_ line: Line) -> some View {
        switch line {
        case .said(let you, let at, let text):
            VStack(alignment: .leading, spacing: 1) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(you ? "You" : "Beep").font(.body(12, .semibold)).foregroundStyle(you ? Mint.muted : Mint.accent)
                    if at > 0 { Text(clock(at)).font(.body(11)).foregroundStyle(Mint.dim).monospacedDigit() }
                }
                if you {
                    Text(text).font(.body(15)).foregroundStyle(Mint.ink).lineSpacing(4).textSelection(.enabled)
                } else {
                    MarkdownText(text: text)
                }
            }
        case .step(let label):
            HStack(spacing: 8) {
                Circle().fill(Mint.dim).frame(width: 4, height: 4)
                Text(label).font(.body(13)).foregroundStyle(Mint.dim).lineLimit(1)
            }
            .padding(.leading, 2)
        }
    }

    /// "5 min", from the first prompt to the last thing said.
    private var length: String {
        let minutes = max(1, Int((end - start) / 60_000))
        return "\(minutes) min"
    }

    private func clock(_ millis: Int64) -> String {
        Date(timeIntervalSince1970: Double(millis) / 1000).formatted(date: .omitted, time: .shortened)
    }
}
