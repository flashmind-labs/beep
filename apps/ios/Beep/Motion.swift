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
            .scaleEffect(on ? 1 : 0.86, anchor: anchor)
            .offset(y: on ? 0 : 10)
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

/// Three dots that hop in turn while Beep works.
struct PulsingDots: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let time = context.date.timeIntervalSinceReferenceDate
            HStack(spacing: 4) {
                ForEach(0..<3) { index in
                    let lift = reduceMotion ? 1 : Self.lift(time, index)
                    Circle().fill(Mint.accent.opacity(0.45 + 0.55 * lift))
                        .frame(width: 6, height: 6)
                        .offset(y: -4 * lift)
                }
            }
            .frame(height: 12)
        }
    }

    /// 380 ms up and 380 ms down, each dot 120 ms behind the one before.
    private static func lift(_ time: TimeInterval, _ index: Int) -> Double {
        let phase = (time - Double(index) * 0.12).truncatingRemainder(dividingBy: 0.76)
        let t = (phase < 0 ? phase + 0.76 : phase) / 0.38
        let x = t <= 1 ? t : 2 - t
        return x < 0.5 ? 4 * x * x * x : 1 - pow(-2 * x + 2, 3) / 2
    }
}

/// The progress line under a pending reply, with a highlight sweeping across its letters.
struct StatusLine: View {
    var body: some View {
        Shimmer(text: "Thinking…")
    }
}

/// Text with a bright band that moves across it, letter by letter.
private struct Shimmer: View {
    let text: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if reduceMotion {
            Text(text).font(.body(13)).foregroundStyle(Mint.muted)
        } else {
            TimelineView(.animation) { context in
                let letters = Array(text)
                let period = 0.09 * Double(letters.count + 8)
                let progress = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: period) / period
                let sweep = -4 + progress * Double(letters.count + 8)
                letters.enumerated().reduce(Text("")) { line, item in
                    let glow = max(0, min(1, 1 - abs(Double(item.offset) - sweep) / 3))
                    return line + Text(String(item.element)).foregroundColor(Self.mix(glow))
                }
                .font(.body(13))
            }
        }
    }

    /// From the muted color to ink as `glow` goes from 0 to 1.
    private static func mix(_ glow: Double) -> Color {
        // Mint.muted is 0x5C6E64 and Mint.ink 0x10201A.
        let from = (0x5C, 0x6E, 0x64), to = (0x10, 0x20, 0x1A)
        func channel(_ a: Int, _ b: Int) -> Double { (Double(a) + (Double(b) - Double(a)) * glow) / 255 }
        return Color(red: channel(from.0, to.0), green: channel(from.1, to.1), blue: channel(from.2, to.2))
    }
}

/// Three grey lines with a sheen passing over them, where the reply will appear.
struct ReplySkeleton: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let shift = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.3) / 1.3
            VStack(alignment: .leading, spacing: 9) {
                ForEach([1, 0.86, 0.55], id: \.self) { width in
                    GeometryReader { geo in
                        let x = geo.size.width * (shift * 2 - 0.5)
                        Capsule().fill(reduceMotion ? AnyShapeStyle(Mint.code) : AnyShapeStyle(LinearGradient(
                            colors: [Mint.code, Mint.surface, Mint.code],
                            startPoint: UnitPoint(x: (x - geo.size.width * 0.4) / geo.size.width, y: 0),
                            endPoint: UnitPoint(x: (x + geo.size.width * 0.4) / geo.size.width, y: 0))))
                            .frame(width: geo.size.width * width)
                    }
                    .frame(height: 12)
                }
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
