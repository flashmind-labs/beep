import SwiftUI

/// Stretches of a chat said aloud in live mode on the Mac, shown as one transcript block each
/// instead of bubbles, as in the mock. The turns are stored as ordinary messages; only the look
/// changes. A stretch keeps going while each spoken prompt comes within ten minutes of the last.
struct SpokenStretches {
    /// Each stretch's messages, by the id of its first message, which draws the block.
    private(set) var byFirst: [String: [Message]] = [:]
    /// The first message of the stretch each of the other messages is in. Those draw nothing
    /// of their own.
    private(set) var firstOf: [String: String] = [:]

    func isHidden(_ id: String) -> Bool { firstOf[id] != nil }

    /// The longest pause between spoken prompts that still counts as one conversation.
    private static let gap: Int64 = 10 * 60 * 1000

    init(_ messages: [Message]) {
        var byFirst: [String: [Message]] = [:]
        var firstOf: [String: String] = [:]
        var current: [Message] = []
        var lastPrompt: Int64 = 0

        func close() {
            if let first = current.first {
                byFirst[first.id] = current
                for message in current.dropFirst() { firstOf[message.id] = first.id }
            }
            current = []
        }

        for message in messages {
            switch message.role {
            case .user:
                guard message.spoken else {
                    close()
                    continue
                }
                // A prompt sent from here has no time until the chat reloads; it was just said.
                let at = message.sentAt > 0 ? message.sentAt : nowMillis()
                if !current.isEmpty, at - lastPrompt > Self.gap { close() }
                current.append(message)
                lastPrompt = at
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
        self.firstOf = firstOf
    }
}

/// One live conversation: who said what, with the steps Beep took between. The phone shows it
/// read only; live mode itself is on the Mac.
struct LiveTranscript: View {
    let messages: [Message]
    /// Kept by the chat, so the prompt map can open a stretch to reach a prompt inside it.
    @Binding var showAll: Bool

    /// How many lines show before "Show all".
    private static let collapsed = 4

    private enum Line {
        /// `prompt` is the id of a spoken prompt after the stretch's first, for the prompt map
        /// to scroll to; the first one's place is the block's own.
        case said(you: Bool, at: Int64, text: String, prompt: String?)
        case step(String)
    }

    private static func lines(_ messages: [Message]) -> [Line] {
        // A reply without a time of its own shows its prompt's, as on Android and the Mac.
        var at: Int64 = 0
        return messages.enumerated().flatMap { index, message -> [Line] in
            switch message.role {
            case .user:
                at = message.sentAt
                return [.said(you: true, at: at, text: message.text, prompt: index > 0 ? message.id : nil)]
            case .assistant:
                let steps = message.tools.filter { !quietTools.contains($0.name) }.map { Line.step(doneLabel($0.label)) }
                let text = message.text.trimmingCharacters(in: .whitespacesAndNewlines)
                let said = Line.said(you: false, at: message.repliedAt > 0 ? message.repliedAt : at, text: text, prompt: nil)
                return steps + (text.isEmpty ? [] : [said])
            case .scheduled:
                return []
            }
        }
    }

    /// Whether the prompt `id` of `stretch` is past the lines a collapsed block shows.
    static func cuts(_ id: String, in stretch: [Message]) -> Bool {
        let index = lines(stretch).firstIndex {
            if case .said(_, _, _, let prompt) = $0 { return prompt == id }
            return false
        }
        return (index ?? 0) >= collapsed
    }

    /// When each prompt was said; one sent from here has no time until the chat reloads.
    private var prompted: [Int64] {
        messages.filter { $0.role == .user && $0.sentAt > 0 }.map(\.sentAt)
    }

    var body: some View {
        let all = Self.lines(messages)
        let cut = !showAll && all.count > Self.collapsed
        let shown = cut ? Array(all.prefix(Self.collapsed)) : all
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "mic").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.accent)
                Text("Live conversation").font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                if let start = prompted.first {
                    Text("\(clock(start)) · \(length)").font(.body(13)).foregroundStyle(Mint.muted)
                }
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
        case .said(let you, let at, let text, let prompt?):
            said(you: you, at: at, text: text).id(prompt)
        case .said(let you, let at, let text, nil):
            said(you: you, at: at, text: text)
        case .step(let label):
            HStack(spacing: 8) {
                Circle().fill(Mint.dim).frame(width: 4, height: 4)
                Text(label).font(.body(13)).foregroundStyle(Mint.dim).lineLimit(1)
            }
            .padding(.leading, 2)
        }
    }

    private func said(you: Bool, at: Int64, text: String) -> some View {
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
    }

    /// "5 min", from the first prompt to the last, rounded as on the Mac.
    private var length: String {
        let span = Double((prompted.last ?? 0) - (prompted.first ?? 0))
        return "\(max(1, Int((span / 60_000).rounded()))) min"
    }

    private func clock(_ millis: Int64) -> String {
        Date(timeIntervalSince1970: Double(millis) / 1000).formatted(date: .omitted, time: .shortened)
    }
}
