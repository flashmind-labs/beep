import SwiftUI

/// The first message of each day, mapped to that day. A reply without a time keeps its prompt's
/// day, and a prompt sent in this session counts as today.
func dayBreaks(_ messages: [Message], now: Date = .now) -> [String: Date] {
    let calendar = Calendar.current
    var breaks: [String: Date] = [:]
    var current: Date?
    for message in messages {
        let day: Date?
        if message.sentAt > 0 {
            day = calendar.startOfDay(for: Date(timeIntervalSince1970: Double(message.sentAt) / 1000))
        } else if message.role == .assistant {
            day = current
        } else {
            day = calendar.startOfDay(for: now)
        }
        guard let day else { continue }
        if day != current { breaks[message.id] = day }
        current = day
    }
    return breaks
}

/// "Today", "Yesterday", "3 Oct", or "3 Oct 2025" for another year.
func dayLabel(_ day: Date, now: Date = .now) -> String {
    let calendar = Calendar.current
    if calendar.isDate(day, inSameDayAs: now) { return "Today" }
    if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) {
        return "Yesterday"
    }
    let sameYear = calendar.component(.year, from: day) == calendar.component(.year, from: now)
    return day.formatted(sameYear ? .dateTime.day().month(.abbreviated) : .dateTime.day().month(.abbreviated).year())
}

struct DayMarker: View {
    let day: Date

    var body: some View {
        Text(dayLabel(day)).font(.body(11)).foregroundStyle(Mint.dim).frame(maxWidth: .infinity)
    }
}

extension View {
    /// Keeps `follow` true while the list sits at its end, and turns it off once the user
    /// scrolls away from it. Our own scrolling while a reply streams in does not count.
    /// `scrolls` says whether the content is taller than the screen. Before iOS 18 there is
    /// no scroll geometry to read, so the list always follows.
    @ViewBuilder func followLatest(_ follow: Binding<Bool>, scrolls: Binding<Bool>) -> some View {
        if #available(iOS 18, *) {
            modifier(FollowLatest(follow: follow, scrolls: scrolls))
        } else {
            self
        }
    }
}

@available(iOS 18, *)
private struct FollowLatest: ViewModifier {
    @Binding var follow: Bool
    @Binding var scrolls: Bool
    @State private var phase = ScrollPhase.idle

    private struct Geometry: Equatable {
        var atEnd: Bool
        var scrolls: Bool
    }

    func body(content: Content) -> some View {
        content
            .onScrollPhaseChange { _, new in phase = new }
            .onScrollGeometryChange(for: Geometry.self) { geometry in
                let bottom = geometry.contentOffset.y + geometry.containerSize.height - geometry.contentInsets.bottom
                return Geometry(atEnd: bottom >= geometry.contentSize.height - 40,
                                scrolls: geometry.contentSize.height > geometry.containerSize.height + 1)
            } action: { _, new in
                if scrolls != new.scrolls { scrolls = new.scrolls }
                if new.atEnd {
                    if !follow { follow = true }
                } else if phase == .interacting || phase == .decelerating, follow {
                    follow = false
                }
            }
    }
}

/// A tick per prompt on the chat's right edge once it scrolls. Pressing the strip shows the
/// nearest prompt, dragging moves between them, and lifting the finger jumps there. A tick sits
/// at its message's place in the chat by index, as on Android.
struct PromptMap: View {
    let messages: [Message]
    let jump: (String) -> Void
    @State private var held: Int?

    private static let inset: CGFloat = 14

    var body: some View {
        let prompts = messages.indices.filter { messages[$0].role == .user }
        if prompts.count >= 2 {
            GeometryReader { proxy in
                let track = proxy.size.height - Self.inset * 2
                let at = { (slot: Int) in Self.inset + track * CGFloat(prompts[slot]) / CGFloat(messages.count) }
                let nearest = { (y: CGFloat) in prompts.indices.min { abs(at($0) - y) < abs(at($1) - y) } ?? 0 }
                ZStack(alignment: .topTrailing) {
                    ZStack(alignment: .topTrailing) {
                        Rectangle().fill(held == nil ? Color.clear : Mint.soft.opacity(0.5))
                        ForEach(prompts.indices, id: \.self) { slot in
                            let lit = slot == held
                            RoundedRectangle(cornerRadius: 2)
                                .fill(lit ? Mint.brand : Mint.dim)
                                .frame(width: lit ? 16 : 10, height: 3)
                                .padding(.trailing, 4)
                                .offset(y: at(slot) - 1.5)
                                .animation(.snappy, value: lit)
                        }
                    }
                    .frame(width: 24)
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            let next = nearest(drag.location.y)
                            if next != held { held = next }
                        }
                        .onEnded { _ in
                            if let held { jump(messages[prompts[held]].id) }
                            held = nil
                        })
                    .sensoryFeedback(.selection, trigger: held) { _, now in now != nil }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Prompts in this chat")
                    if let held {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Prompt \(held + 1) of \(prompts.count)").font(.body(11, .semibold)).foregroundStyle(Mint.muted)
                            Text(messages[prompts[held]].text).font(.body(13)).foregroundStyle(Mint.ink).lineLimit(2)
                        }
                        .padding(.horizontal, 12).padding(.vertical, 8)
                        .frame(maxWidth: 250, alignment: .leading)
                        .chunky(radius: 12)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.trailing, 28)
                        // Centred on the tick, but kept inside the list.
                        .offset(y: min(max(at(held) - 24, 0), max(proxy.size.height - 56, 0)))
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
            }
            .transition(.opacity)
        }
    }
}
