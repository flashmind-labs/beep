import SwiftUI

/// Over the whole app while live voice is on: a glow around the screen's edge that follows
/// the voice. A launch opens with a wash of color from the button that started it, which
/// pulls back into the edge glow. It never takes a touch, so the chat under it works as usual.
struct LiveGlow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ZStack {
            if model.live.active { Glow().transition(.opacity) }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .animation(.easeOut(duration: 0.2), value: model.live.active)
    }
}

private struct Glow: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var opened = Date.now

    private var live: LiveVoice { model.live }

    var body: some View {
        GeometryReader { geometry in
            TimelineView(.animation(paused: settled)) { context in
                let clock = LaunchClock(time: context.date.timeIntervalSince(opened), still: reduceMotion)
                scene(clock, size: geometry.size)
            }
        }
        .ignoresSafeArea()
        .onChange(of: live.launches, initial: true) { opened = .now }
    }

    /// The launch is over, so only the edge glow still moves.
    private var settled: Bool { Date.now.timeIntervalSince(opened) > LaunchClock.length }

    private func scene(_ clock: LaunchClock, size: CGSize) -> some View {
        let origin = live.origin.point
        return ZStack {
            EdgeGlow(level: CGFloat(live.level), still: reduceMotion)
                .opacity(clock.edge)
            if !reduceMotion, clock.time < LaunchClock.length {
                Wash(origin: origin, size: size, clock: clock, dark: scheme == .dark)
                Spark(clock: clock)
                    .position(x: origin.x * size.width, y: origin.y * size.height)
            }
        }
    }
}

extension LiveOrigin {
    /// Where on the screen the launch glow starts: the Action Button high on the left edge,
    /// the side button on the right, the top's middle for a control or the widget, and the
    /// chat bar's button for a start inside the app.
    var point: UnitPoint {
        switch self {
        case .actionButton: UnitPoint(x: 0, y: 0.21)
        case .sideButton: UnitPoint(x: 1, y: 0.27)
        case .control: UnitPoint(x: 0.5, y: 0.04)
        case .app: UnitPoint(x: 0.8, y: 0.08)
        }
    }
}

/// The launch's timing, from the moment live voice opened: a bright wash for about a quarter
/// second, then it opens up from the middle and fades into the edge glow. With Reduce Motion
/// the glow only fades in.
private struct LaunchClock {
    static let length: TimeInterval = 0.9
    let time: TimeInterval
    let still: Bool

    private func part(_ start: Double, _ duration: Double) -> Double {
        min(1, max(0, (time - start) / duration))
    }

    private static func easeOut(_ p: Double) -> Double { 1 - pow(1 - p, 3) }

    var edge: Double { still ? part(0, 0.2) : part(0.175, 0.525) }

    /// How far the wash has spread from the button, 0 to 1.
    var washSpread: Double { Self.easeOut(part(0, 0.25)) }
    var washOpacity: Double {
        if time < 0.07 { return part(0, 0.07) }
        if time < 0.25 { return 1 }
        return 1 - part(0.25, 0.45)
    }
    /// The hole opening in the wash's middle as it pulls back to the edge, 0 to 0.75.
    var washHole: Double { 0.75 * Self.easeOut(part(0.25, 0.45)) }

    var spark: Double {
        let p = part(0, 0.32)
        return p < 0.25 ? p / 0.25 : 1 - (p - 0.25) / 0.75
    }
}

/// A soft band of Beep's colors around the screen's edge, turning slowly, wider and brighter
/// as the voice gets louder, with a thin bright line on the very edge.
private struct EdgeGlow: View {
    var level: CGFloat
    var still: Bool

    var body: some View {
        TimelineView(.animation(paused: still)) { context in
            let turn = still ? 200 : context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 7) / 7 * 360
            let gradient = AngularGradient(colors: LiveColors.glow, center: .center,
                                           startAngle: .degrees(turn), endAngle: .degrees(turn + 360))
            let shape = RoundedRectangle(cornerRadius: 55, style: .continuous)
            ZStack {
                shape.strokeBorder(gradient, lineWidth: still ? 10 : 13 + level * 10)
                    .blur(radius: 9)
                    .opacity(0.8 + 0.2 * level)
                shape.strokeBorder(gradient, lineWidth: still ? 2.5 : 3 + level * 2)
                    .blur(radius: 1)
                    .opacity(0.9)
            }
            .animation(still ? nil : .linear(duration: 0.12), value: level)
        }
    }
}

/// The launch's flood of color from the button, which opens from the middle as it fades.
private struct Wash: View {
    var origin: UnitPoint
    var size: CGSize
    var clock: LaunchClock
    var dark: Bool

    var body: some View {
        let diagonal = (size.width * size.width + size.height * size.height).squareRoot()
        let gradient = RadialGradient(stops: [
            .init(color: Color(hex: 0xFFD6C8).opacity(0.95), location: 0),
            .init(color: Color(hex: 0xF07A6A).opacity(0.7), location: 0.22),
            .init(color: Color(hex: 0xFFE08A).opacity(0.5), location: 0.4),
            .init(color: Color(hex: 0xA6EBCF).opacity(0.55), location: 0.62),
            .init(color: Color(hex: 0x3B9B7A).opacity(0.35), location: 0.82),
            .init(color: Color(hex: 0x6FC3D9).opacity(0.2), location: 1),
        ], center: origin, startRadius: 0, endRadius: diagonal * 0.9)
        let hole = clock.washHole
        Rectangle().fill(gradient)
            .mask {
                Circle()
                    .frame(width: diagonal * 3 * clock.washSpread, height: diagonal * 3 * clock.washSpread)
                    .position(x: origin.x * size.width, y: origin.y * size.height)
            }
            .mask {
                RadialGradient(stops: [
                    .init(color: .clear, location: hole),
                    .init(color: .black, location: min(1, hole + 0.45)),
                ], center: .center, startRadius: 0, endRadius: diagonal * 0.55)
            }
            .blendMode(dark ? .screen : .normal)
            .opacity(clock.washOpacity)
    }
}

/// The flash where the button is pressed.
private struct Spark: View {
    var clock: LaunchClock

    var body: some View {
        Ellipse()
            .fill(RadialGradient(colors: [.white, Color(hex: 0xFFD0C7), Color(hex: 0xF07A6A).opacity(0)],
                                 center: .center, startRadius: 0, endRadius: 50))
            .frame(width: 72, height: 120)
            .blur(radius: 3)
            .scaleEffect(0.4 + 1.2 * CGFloat(min(1, clock.time / 0.32)))
            .opacity(clock.spark)
    }
}
