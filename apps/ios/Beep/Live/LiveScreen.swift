import SwiftUI

/// The live voice screen, over the whole app: a glow around the screen's edge that follows
/// the voice, the orb, the words and Beep's lines. It opens with a wash of color from the
/// button that started it, which pulls back into the edge glow.
struct LiveScreen: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var opened = Date.now

    private var live: LiveVoice { model.live }
    private var colors: LiveColors { scheme == .dark ? .dark : .light }

    var body: some View {
        GeometryReader { geometry in
            TimelineView(.animation(paused: settled)) { context in
                let clock = LaunchClock(time: context.date.timeIntervalSince(opened), still: reduceMotion)
                scene(clock, size: geometry.size)
            }
        }
        .ignoresSafeArea()
        .onChange(of: live.launches, initial: true) { opened = .now }
        .accessibilityAddTraits(.isModal)
    }

    /// The launch is over, so only the glow and the orb still move.
    private var settled: Bool { Date.now.timeIntervalSince(opened) > LaunchClock.length }

    private func scene(_ clock: LaunchClock, size: CGSize) -> some View {
        let origin = live.origin.point
        let level = CGFloat(live.level)
        return ZStack {
            colors.page.opacity(clock.page)
            EdgeGlow(level: level, still: reduceMotion, dark: scheme == .dark)
                .opacity(clock.edge)
            if !reduceMotion {
                Wash(origin: origin, size: size, clock: clock, dark: scheme == .dark)
                Spark(clock: clock)
                    .position(x: origin.x * size.width, y: origin.y * size.height)
            }
            VStack(spacing: 0) {
                top.opacity(clock.content).offset(y: clock.rise)
                Spacer(minLength: 0)
                middle(clock, size: size, origin: origin, level: level)
                Spacer(minLength: 0)
                bottom.opacity(clock.content).offset(y: clock.rise)
            }
            .padding(.top, 54)
            .padding(.bottom, 30)
            if let toast = live.toast {
                VStack {
                    Text(toast).font(.body(13, .medium)).foregroundStyle(colors.page)
                        .padding(.horizontal, 12).padding(.vertical, 5)
                        .background(Capsule().fill(colors.ink))
                        .padding(.top, 112)
                        .transition(.scale(scale: 0.8).combined(with: .opacity))
                        .accessibilityAddTraits(.updatesFrequently)
                    Spacer()
                }
            }
        }
        .animation(.spring(response: 0.25, dampingFraction: 0.6), value: live.toast)
    }

    // MARK: Parts

    private var top: some View {
        HStack(spacing: 8) {
            Text("b").font(.custom("BricolageGrotesque-96ptExtraBold", size: 13)).foregroundStyle(Color(hex: 0x10201A))
                .frame(width: 24, height: 24)
                .background(RoundedRectangle(cornerRadius: 7).fill(Color(hex: 0xF07A6A)))
            Text(live.chatId.map(model.title(of:)) ?? "New chat").lineLimit(1)
            HStack(spacing: 5) {
                Circle().frame(width: 6, height: 6)
                Text("LIVE").kerning(0.6)
            }
            .font(.body(10.5, .semibold)).foregroundStyle(LiveColors.listen)
            .padding(.horizontal, 8).padding(.vertical, 2)
            .background(Capsule().fill(colors.chip))
            .padding(.leading, 6)
            Spacer(minLength: 0)
            Button { live.close() } label: {
                Image(systemName: "xmark").font(.system(size: 15, weight: .semibold)).foregroundStyle(colors.ink)
                    .frame(width: 40, height: 40).background(Circle().fill(colors.chip))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Leave live mode")
        }
        .font(.body(13)).foregroundStyle(colors.muted)
        .padding(.horizontal, 18)
    }

    private func middle(_ clock: LaunchClock, size: CGSize, origin: UnitPoint, level: CGFloat) -> some View {
        // The orb flies in from the button, from where it sits in the screen's middle.
        let from = CGSize(width: (origin.x - 0.5) * size.width, height: (origin.y - 0.42) * size.height)
        return VStack(spacing: 10) {
            OrbControl(live: live, level: level, still: reduceMotion)
                .scaleEffect(clock.orbScale)
                .offset(x: from.width * clock.orbAway, y: from.height * clock.orbAway)
                .opacity(clock.orbOpacity)
            Group {
                Text(status.word).font(.body(12, .semibold)).kerning(1).textCase(.uppercase)
                    .foregroundStyle(status.color).padding(.top, 8)
                caption.frame(minHeight: 96, alignment: .top)
                if let soon = live.soon, live.view.phase == .listening {
                    SoonBar(soon: soon, track: colors.line)
                }
            }
            .opacity(clock.content).offset(y: clock.rise)
        }
        .padding(.horizontal, 26)
    }

    private var status: (word: String, color: Color) {
        if live.problem != nil { return ("Voice unavailable", colors.muted) }
        switch live.view.phase {
        case .listening: return ("Listening", LiveColors.listen)
        case .holding: return ("Holding", LiveColors.listen)
        case .thinking: return ("Thinking", colors.muted)
        case .speaking: return ("Speaking", LiveColors.speak)
        }
    }

    @ViewBuilder private var caption: some View {
        let view = live.view
        VStack(spacing: 6) {
            if let problem = live.problem {
                Text(problem).font(.body(24)).foregroundStyle(colors.dim)
            } else {
                switch view.phase {
                case .listening, .holding:
                    if let prev = view.line, view.words.isEmpty { Text(prev).font(.body(15)).foregroundStyle(colors.dim) }
                    if view.words.isEmpty {
                        Text(view.line == nil ? "Go ahead, I’m listening" : "Go ahead").font(.body(24)).foregroundStyle(colors.dim)
                    } else {
                        Text(view.words).font(.body(24, .medium)).foregroundStyle(colors.ink)
                    }
                case .thinking:
                    if let said = live.said { Text(said).font(.body(15)).foregroundStyle(colors.dim) }
                    if let step = view.step { StepRow(text: step, color: colors.muted) } else {
                        Text("…").font(.body(24)).foregroundStyle(colors.dim)
                    }
                case .speaking:
                    if let prev = view.prev { Text(prev).font(.body(15)).foregroundStyle(colors.dim) }
                    if let line = view.line { Text(line).font(.body(24, .medium)).foregroundStyle(colors.ink) }
                    if let step = view.step { StepRow(text: step, color: colors.muted).padding(.top, 2) }
                }
            }
        }
        .multilineTextAlignment(.center)
        .lineSpacing(4)
        .frame(maxWidth: .infinity)
        .animation(.easeOut(duration: 0.15), value: view)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.updatesFrequently)
    }

    private var bottom: some View {
        VStack(spacing: 12) {
            if let hint { Text(hint).font(.body(13)).foregroundStyle(colors.muted).multilineTextAlignment(.center) }
            if live.problem == nil, live.view.phase == .listening, live.view.words.isEmpty, live.view.line == nil {
                Commands(chip: colors.chip, ink: colors.ink, muted: colors.muted)
            }
        }
        .padding(.horizontal, 22)
    }

    private var hint: String? {
        if live.problem != nil { return nil }
        switch live.view.phase {
        case .listening:
            if live.soon != nil { return "Sending… keep talking to add more" }
            return live.sendOnPause ? "Pause to send · Tap the orb to stop · Hold it to talk"
                : "Say “send” when you’re done · Tap the orb to stop"
        case .holding: return "Let go to send · Slide away to cancel"
        case .thinking: return "Tap the orb to skip the answer"
        case .speaking: return "Tap the orb or say “stop” to interrupt"
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

/// The launch's timing, from the moment the screen opened: a bright wash for about a quarter
/// second, then it opens up from the middle and fades into the edge glow while the orb flies
/// in from the button. With Reduce Motion the scene only fades in.
struct LaunchClock {
    static let length: TimeInterval = 0.9
    let time: TimeInterval
    let still: Bool

    private func part(_ start: Double, _ duration: Double) -> Double {
        min(1, max(0, (time - start) / duration))
    }

    private static func easeOut(_ p: Double) -> Double { 1 - pow(1 - p, 3) }

    /// Overshoots a little before settling, like the mock's cubic-bezier(.3, 1.35, .5, 1).
    private static func easeOutBack(_ p: Double) -> Double {
        let c1 = 1.70158, c3 = c1 + 1
        return 1 + c3 * pow(p - 1, 3) + c1 * pow(p - 1, 2)
    }

    /// The page behind the scene: the app shows through the wash at first.
    var page: Double { still ? part(0, 0.2) : Self.easeOut(part(0.1, 0.35)) }
    var edge: Double { still ? part(0, 0.2) : part(0.175, 0.525) }
    var content: Double { still ? part(0, 0.2) : Self.easeOut(part(0.38, 0.3)) }
    var rise: CGFloat { still ? 0 : CGFloat(6 * (1 - content)) }

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

    private var orb: Double { part(0.14, 0.62) }
    var orbScale: CGFloat { still ? 1 : CGFloat(0.15 + 0.85 * Self.easeOutBack(orb)) }
    var orbAway: CGFloat { still ? 0 : CGFloat(1 - Self.easeOutBack(orb)) }
    var orbOpacity: Double { still ? part(0, 0.2) : min(1, orb / 0.4) }
}

/// The colors of the live screen, which follows the system's light or dark appearance.
struct LiveColors {
    var page, ink, muted, dim, chip, line: Color

    static let light = LiveColors(page: Color(hex: 0xF4F8F5), ink: Color(hex: 0x10201A), muted: Color(hex: 0x5C6E64),
                                  dim: Color(hex: 0x94A49B), chip: .white, line: Color(hex: 0xCFE0D6))
    static let dark = LiveColors(page: Color(hex: 0x0A0E0C), ink: Color(hex: 0xE4EEE8), muted: Color(hex: 0x92A59A),
                                 dim: Color(hex: 0x5E6F66), chip: Color(hex: 0x1A231E), line: Color(hex: 0x243028))
    static let listen = Color(hex: 0x3B9B7A)
    static let speak = Color(hex: 0xF07A6A)
    /// Around the edge: coral, peach, butter, mint, green, sky, rose.
    static let glow: [Color] = ([0xF07A6A, 0xFFB59E, 0xFFE08A, 0xA6EBCF, 0x3B9B7A, 0x6FC3D9, 0xF59A8C, 0xF07A6A] as [UInt32])
        .map { Color(hex: $0) }
}

/// A soft band of Beep's colors around the screen's edge, turning slowly, wider and brighter
/// as the voice gets louder, with a thin bright line on the very edge.
struct EdgeGlow: View {
    var level: CGFloat
    var still: Bool
    var dark: Bool

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
        .allowsHitTesting(false)
        .accessibilityHidden(true)
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
            .allowsHitTesting(false)
            .accessibilityHidden(true)
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
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// The orb, which also takes the screen's taps: a tap stops listening or skips the answer,
/// holding talks until let go, and sliding away while holding cancels.
private struct OrbControl: View {
    let live: LiveVoice
    var level: CGFloat
    var still: Bool
    @State private var pressed = false
    @State private var holding = false
    @State private var away = false
    @State private var holdTimer: Task<Void, Never>?

    var body: some View {
        LiveOrb(mood: mood, level: still ? 0 : level, still: still)
            .scaleEffect(pressed ? 0.96 : 1)
            .opacity(away ? 0.5 : 1)
            .contentShape(Circle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    if !pressed {
                        pressed = true
                        holdTimer = Task {
                            try? await Task.sleep(for: .milliseconds(350))
                            guard !Task.isCancelled, pressed else { return }
                            holding = true
                            live.holdStart()
                        }
                    }
                    if holding {
                        away = hypot(drag.translation.width, drag.translation.height) > 90
                    }
                }
                .onEnded { _ in
                    holdTimer?.cancel()
                    if holding {
                        if away { live.holdCancel() } else { live.holdEnd() }
                    } else {
                        live.tap()
                    }
                    pressed = false
                    holding = false
                    away = false
                })
            .accessibilityElement()
            .accessibilityLabel(live.view.phase == .speaking ? "Stop speaking" : "Tap to stop, hold to talk")
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { live.tap() }
            .disabled(live.problem != nil)
    }

    private var mood: LiveOrb.Mood {
        switch live.view.phase {
        case .listening, .holding: .listen
        case .thinking: .think
        case .speaking: .speak
        }
    }
}

/// Beep's orb: mint while it listens, darker and breathing while it thinks, coral while it
/// speaks. Its rings and glow grow with the voice.
struct LiveOrb: View {
    enum Mood { case listen, think, speak }

    var mood: Mood
    var level: CGFloat
    var still: Bool
    var size: CGFloat = 112

    private struct Tones {
        var light, mid, deep, glow: Color
    }

    private var tones: Tones {
        switch mood {
        case .listen: Tones(light: Color(hex: 0xA6EBCF), mid: Color(hex: 0x3B9B7A), deep: Color(hex: 0x0F3B2C), glow: Color(hex: 0x3B9B7A))
        case .think: Tones(light: Color(hex: 0x7FBFA6), mid: Color(hex: 0x2E6F59), deep: Color(hex: 0x0D2A20), glow: Color(hex: 0x3B9B7A))
        case .speak: Tones(light: Color(hex: 0xFFD0C7), mid: Color(hex: 0xF07A6A), deep: Color(hex: 0x5A231C), glow: Color(hex: 0xF07A6A))
        }
    }

    var body: some View {
        TimelineView(.animation(paused: still || mood != .think)) { context in
            let breath = mood == .think && !still
                ? 1 + 0.04 * sin(context.date.timeIntervalSinceReferenceDate * 2 * .pi / 2.4) : 1
            let palette = tones
            ZStack {
                Circle().stroke(palette.glow.opacity(0.06 + level * 0.2), lineWidth: 2)
                    .frame(width: size, height: size).scaleEffect(1.18 + level * 0.65)
                Circle().stroke(palette.glow.opacity(0.14 + level * 0.35), lineWidth: 2)
                    .frame(width: size, height: size).scaleEffect(1.05 + level * 0.4)
                Circle()
                    .fill(RadialGradient(stops: [
                        .init(color: palette.light, location: 0),
                        .init(color: palette.mid, location: 0.46),
                        .init(color: palette.deep, location: 1),
                    ], center: UnitPoint(x: 0.36, y: 0.3), startRadius: 0, endRadius: size * 0.75))
                    .overlay(Circle().stroke(.black.opacity(0.15), lineWidth: 6).blur(radius: 6).offset(y: -3).clipShape(Circle()))
                    .frame(width: size, height: size)
                    .shadow(color: palette.glow.opacity(0.3 + level * 0.4), radius: 11 + level * 25)
                    .scaleEffect(1 + level * 0.2)
            }
            .scaleEffect(breath)
            .frame(width: size * 1.5, height: size * 1.5)
            .animation(still ? nil : .linear(duration: 0.08), value: level)
            .animation(.easeInOut(duration: 0.3), value: mood)
        }
    }
}

/// How long until a pause sends.
private struct SoonBar: View {
    var soon: LiveVoice.Soon
    var track: Color

    var body: some View {
        TimelineView(.animation) { context in
            let done = min(1, context.date.timeIntervalSince(soon.from) / max(soon.seconds, 0.1))
            Capsule().fill(track).frame(width: 150, height: 3)
                .overlay(alignment: .leading) {
                    Capsule().fill(LiveColors.listen).frame(width: 150 * done, height: 3)
                }
        }
        .accessibilityElement()
        .accessibilityLabel("Sending after the pause")
    }
}

private struct StepRow: View {
    var text: String
    var color: Color

    var body: some View {
        HStack(spacing: 8) {
            ProgressView().controlSize(.small).tint(color)
            Text(text).font(.body(14)).foregroundStyle(color)
        }
    }
}

/// The spoken commands, while nothing has been said yet.
private struct Commands: View {
    var chip, ink, muted: Color
    private struct Command {
        var said: String
        var does = ""
    }

    private let all = [
        Command(said: "send", does: "sends"), Command(said: "cancel", does: "drops it"), Command(said: "scratch that"),
        Command(said: "stop"), Command(said: "repeat that"), Command(said: "new chat"), Command(said: "bye", does: "leaves"),
    ]

    var body: some View {
        FlowRow(spacing: 6) {
            ForEach(all, id: \.said) { command in
                (Text("“\(command.said)”").foregroundStyle(ink).fontWeight(.medium)
                    + Text(command.does.isEmpty ? "" : " " + command.does).foregroundStyle(muted))
                    .font(.body(12))
                    .padding(.horizontal, 10).padding(.vertical, 4)
                    .background(Capsule().fill(chip))
            }
        }
    }
}

/// Lays its children out in centered rows that wrap.
private struct FlowRow: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = rows(width: proposal.width ?? .infinity, subviews: subviews)
        let height = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(rows.count - 1, 0))
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in rows(width: bounds.width, subviews: subviews) {
            var x = bounds.midX - row.width / 2
            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func rows(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = []
        var row = Row()
        for (index, subview) in subviews.enumerated() {
            let size = subview.sizeThatFits(.unspecified)
            let needed = row.items.isEmpty ? size.width : row.width + spacing + size.width
            if !row.items.isEmpty, needed > width {
                rows.append(row)
                row = Row()
            }
            row.width = row.items.isEmpty ? size.width : row.width + spacing + size.width
            row.height = max(row.height, size.height)
            row.items.append(index)
        }
        if !row.items.isEmpty { rows.append(row) }
        return rows
    }
}
