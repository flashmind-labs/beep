import SwiftUI

/// The springs the Android app uses, so both apps move alike.
extension Animation {
    /// `popIn`: damping 0.55, stiffness 420.
    static let pop = Animation.interpolatingSpring(stiffness: 420, damping: 2 * 0.55 * sqrt(420))
    /// A pressed button sinking into its shadow: damping 0.55, high stiffness.
    static let sink = Animation.interpolatingSpring(stiffness: 10_000, damping: 2 * 0.55 * 100)
    /// A done mark stamped onto a tool chip: damping 0.45, stiffness 500.
    static let stamp = Animation.interpolatingSpring(stiffness: 500, damping: 2 * 0.45 * sqrt(500))
    /// Android's medium bouncy spring, for things that grow or shrink.
    static let bouncy = Animation.interpolatingSpring(stiffness: 1500, damping: 2 * 0.5 * sqrt(1500))
}

/// Springs a piece in from slightly below and smaller, once, when it first shows.
/// `play: false` is for content that was already on screen.
private struct PopIn: ViewModifier {
    let anchor: UnitPoint
    let delay: Double
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Taken from `play` the first time only, so marking a piece as seen
    /// while it springs in does not cut the spring short.
    @State private var play: Bool
    @State private var shown = false

    init(play: Bool, anchor: UnitPoint, delay: Double) {
        self.anchor = anchor
        self.delay = delay
        _play = State(initialValue: play)
    }

    func body(content: Content) -> some View {
        let on = shown || !play || reduceMotion
        content
            .opacity(on ? 1 : 0)
            .scaleEffect(on ? 1 : 0.98, anchor: anchor)
            .offset(y: on ? 0 : 4)
            .onAppear {
                guard play, !shown else { return }
                withAnimation(.pop.delay(delay)) { shown = true }
            }
    }
}

extension View {
    func popIn(play: Bool = true, anchor: UnitPoint = .bottom, delay: Double = 0) -> some View {
        modifier(PopIn(play: play, anchor: anchor, delay: delay))
    }

    /// Fades and grows in from the top, as Android's `fadeIn() + expandVertically()`.
    func unfold() -> some View {
        transition(.asymmetric(
            insertion: .opacity.combined(with: .scale(scale: 0.96, anchor: .top)).combined(with: .move(edge: .top)),
            removal: .opacity))
    }
}

struct PulsingDots: View {
    var body: some View {
        HStack(spacing: 4) {
            ForEach(0..<3) { _ in Circle().fill(Mint.accent).frame(width: 6, height: 6) }
        }.frame(height: 12)
    }
}

struct StatusLine: View {
    var body: some View { Text("Thinking…").font(.body(13)).foregroundStyle(Mint.muted) }
}

struct ReplySkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach([1, 0.86, 0.55], id: \.self) { width in
                GeometryReader { geo in
                    RoundedRectangle(cornerRadius: 4).fill(Mint.code).frame(width: geo.size.width * width)
                }.frame(height: 12)
            }
        }
    }
}

/// Shows streamed text a little behind the stream, so it flows in instead of
/// jumping a chunk at a time. Text that is not streaming shows at once.
struct Typewriter<Content: View>: View {
    let text: String
    let streaming: Bool
    @ViewBuilder let content: (String) -> Content
    @State private var shown: Int?

    var body: some View {
        let count = text.count
        let visible = streaming ? min(shown ?? count, count) : count
        content(visible < count ? String(text.prefix(visible)) : text)
            .task(id: count) {
                guard streaming else { shown = nil; return }
                let start = min(shown ?? count, count)
                guard start < count else { return }
                // Catch up over 150 ms, as Android's `animateIntAsState`.
                let steps = 9
                for step in 1...steps {
                    try? await Task.sleep(for: .milliseconds(150 / steps))
                    if Task.isCancelled { return }
                    shown = start + (count - start) * step / steps
                }
            }
            .onAppear { if streaming, shown == nil { shown = count } }
    }
}
