import SwiftUI

/// Live voice in the chat: a dock where the composer sits, while the chat above stays as it
/// always is. It has the orb, which takes the taps, the state word, the caption (what you say,
/// or the line Beep reads out), the pause bar, the hint, Cancel during a reply
/// and a way out. A toast floats over it.
struct LiveDock: View {
    @Environment(AppModel.self) private var model
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Set just after the dock appears, so it rises in and the orb grows out of it.
    @State private var shown = false

    private var live: LiveVoice { model.live }
    private var landed: Bool { shown || reduceMotion }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if let prompt = strayApproval {
                // Its own element, so VoiceOver reaches Allow, Always and Deny.
                ScrollView { ApprovalCard(prompt: prompt) }
                    .frame(maxHeight: 260)
                    .scrollBounceBehavior(.basedOnSize)
                    .padding(.leading, 2)
                    .padding(.bottom, 6)
            }
            HStack(alignment: .top, spacing: 10) {
                OrbControl(live: live, still: reduceMotion)
                    .scaleEffect(landed ? 1 : 0.2)
                    .opacity(landed ? 1 : 0)
                VStack(alignment: .leading, spacing: 1) {
                    Text(status.word).font(.body(11, .semibold)).kerning(0.9).textCase(.uppercase)
                        .foregroundStyle(status.color)
                    caption
                }
                .frame(maxWidth: .infinity, minHeight: 48, alignment: .leading)
                leave
            }
            if let soon = live.soon, live.view.phase == .listening {
                SoonBar(soon: soon).padding(.leading, 58).padding(.top, 2)
            }
            if hasFooter { footer.padding(.leading, 58) }
        }
        .padding(EdgeInsets(top: 10, leading: 8, bottom: 10, trailing: 10))
        .chunky(Mint.surface, radius: 24)
        .overlay(alignment: .top) {
            if let toast = live.toast {
                Text(toast).font(.body(13, .medium)).foregroundStyle(Mint.page)
                    .padding(.horizontal, 12).padding(.vertical, 5)
                    .background(Capsule().fill(Mint.ink))
                    .offset(y: -40)
                    .transition(.scale(scale: 0.8).combined(with: .opacity))
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
        .animation(.spring(response: 0.25, dampingFraction: 0.6), value: live.toast)
        .padding(.horizontal, 10)
        .padding(.top, 8)
        .padding(.bottom, 10)
        .offset(y: landed ? 0 : 28)
        .opacity(landed ? 1 : 0)
        .blocksChatSwipe()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Live voice")
        .onAppear {
            guard !reduceMotion else { return }
            withAnimation(.spring(response: 0.45, dampingFraction: 0.7).delay(0.12)) { shown = true }
        }
    }

    // MARK: Parts

    private var leave: some View {
        Button { live.close() } label: {
            Image(systemName: "xmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.ink)
                .frame(width: 36, height: 36)
                .background(Circle().fill(Mint.page))
                .overlay(Circle().stroke(Mint.soft, lineWidth: 1.5))
                .frame(width: 44, height: 44)
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .padding(.top, -4).padding(.trailing, -4)
        .accessibilityLabel(live.replying ? "Cancel the reply and leave" : "Leave live mode")
    }

    /// The approval the reply waits on when the chat on screen does not hold its card, such as
    /// after opening another chat; otherwise it is answered in the chat.
    private var strayApproval: ApprovalPrompt? {
        guard live.view.phase == .waiting, let prompt = live.asking else { return nil }
        let inChat = model.chat.messages.contains { $0.approvals.contains { $0.queryId == prompt.queryId } }
        return inChat ? nil : prompt
    }

    private var status: (word: String, color: Color) {
        if live.problem != nil { return ("Voice unavailable", Mint.muted) }
        if live.paused { return ("Paused", Mint.muted) }
        switch live.view.phase {
        case .connecting: return ("Connecting", Mint.muted)
        case .listening: return ("Listening", LiveColors.listen)
        case .holding: return (live.headphoneTalk ? "Talking" : "Holding", LiveColors.listen)
        case .thinking: return ("Thinking", Mint.muted)
        case .speaking: return ("Speaking", LiveColors.speak)
        case .waiting: return ("Waiting for you", LiveColors.listen)
        }
    }

    @ViewBuilder private var caption: some View {
        let view = live.view
        VStack(alignment: .leading, spacing: 1) {
            if let problem = live.problem {
                placeholder(problem.text)
            } else if live.paused {
                placeholder("Paused while another app uses the audio")
            } else {
                switch view.phase {
                case .connecting:
                    placeholder("Getting ready…")
                case .waiting:
                    placeholder(strayApproval == nil ? "Allow or deny in the chat" : "Allow or deny above")
                case .listening, .holding:
                    if view.words.isEmpty {
                        if let prev = view.line { previous(prev) }
                        placeholder(view.line == nil ? "Go ahead, I’m listening" : "Go ahead")
                    } else {
                        // The newest words, when there are more than the dock holds.
                        Text(view.words.count > 140 ? "…" + String(view.words.suffix(140)) : view.words)
                            .font(.body(14, .medium)).foregroundStyle(Mint.ink).lineLimit(3)
                    }
                case .thinking:
                    if let said = live.said { previous(said) }
                    if let step = view.step { StepRow(text: step) } else { placeholder("…") }
                case .speaking:
                    if let prev = view.prev { previous(prev) }
                    if let line = view.line {
                        Text(line).font(.body(14, .medium)).foregroundStyle(Mint.ink).lineLimit(4)
                    }
                    if let step = view.step { StepRow(text: step).padding(.top, 2) }
                }
                if view.voiceUnavailable {
                    Text("Beep’s voice is unavailable").font(.body(12)).foregroundStyle(Mint.muted).padding(.top, 2)
                }
            }
        }
        .lineSpacing(2)
        .animation(.easeOut(duration: 0.15), value: view)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.updatesFrequently)
    }

    private func placeholder(_ text: String) -> some View {
        Text(text).font(.body(14)).foregroundStyle(Mint.dim)
    }

    /// The line before, small and on one line above the caption.
    private func previous(_ text: String) -> some View {
        Text(text).font(.body(12)).foregroundStyle(Mint.dim).lineLimit(1)
    }

    // MARK: Footer

    private var hasFooter: Bool { hint != nil || button != nil }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 6) {
            if hint != nil || button != nil {
                HStack(spacing: 10) {
                    Text(hint ?? "").font(.body(12)).foregroundStyle(Mint.muted)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if let button { DockButton(button: button, live: live) }
                }
            }
        }
    }

    private var button: DockButton.Kind? {
        if let problem = live.problem { return problem.settings ? .settings : .retry }
        return live.replying ? .cancel : nil
    }

    private var hint: String? {
        if live.problem != nil { return nil }
        if live.paused { return "Beep carries on when it’s done" }
        switch live.view.phase {
        case .connecting: return nil
        case .listening:
            if live.soon != nil { return "Sending… keep talking to add more" }
            return live.sendOnPause ? "Pause to send · Tap the orb to leave · Hold it to talk"
                : "Say “send” or “stop” when you’re done · Tap the orb to leave"
        case .holding:
            return live.headphoneTalk ? "Press your AirPods to send · Press twice to leave"
                : "Let go to send · Slide away to cancel"
        case .thinking: return "Tap the orb to stop the answer"
        case .speaking: return "Tap the orb or say “stop” to interrupt"
        case .waiting: return "Beep goes on once you answer"
        }
    }
}

/// The colors live voice adds to the app's own.
enum LiveColors {
    static var listen: Color { Mint.ok }
    static var speak: Color { Mint.brand }

}

/// The orb, which takes the dock's taps: a tap leaves while listening or stops the answer,
/// holding talks until let go, and sliding away while holding cancels.
private struct OrbControl: View {
    let live: LiveVoice
    var still: Bool
    @State private var pressed = false
    @State private var holding = false
    @State private var away = false
    @State private var holdTimer: Task<Void, Never>?

    var body: some View {
        LiveOrb(mood: mood, level: still ? 0 : CGFloat(live.level), still: still)
            .scaleEffect(pressed ? 0.92 : 1)
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
            .accessibilityLabel(label)
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { live.tap() }
            .disabled(live.problem != nil)
    }

    private var label: String {
        switch live.view.phase {
        case .speaking: "Stop speaking"
        case .thinking: "Stop the answer"
        default: "Tap to leave, hold to talk"
        }
    }

    /// Grey while nothing is said or live voice cannot go on, mint once words come.
    private var mood: LiveOrb.Mood {
        if live.problem != nil || live.paused { return .idle }
        switch live.view.phase {
        case .connecting, .waiting: return .idle
        case .listening: return live.view.words.isEmpty ? .idle : .listen
        case .holding: return .listen
        case .thinking: return .think
        case .speaking: return .speak
        }
    }
}

/// Beep's orb: grey while it waits for words, mint while it hears them, darker and breathing
/// while it thinks, coral while it speaks. Its rings and glow grow with the voice. It takes
/// `size * 1.5` so the rings fit.
struct LiveOrb: View {
    enum Mood { case idle, listen, think, speak }

    var mood: Mood
    var level: CGFloat
    var still: Bool
    var size: CGFloat = 32

    private var tone: Color {
        switch mood {
        case .idle: Mint.muted
        case .listen: Mint.ok
        case .think, .speak: Mint.accent
        }
    }

    var body: some View {
        ZStack {
            Circle().fill(tone.opacity(0.15 + level * 0.25))
                .frame(width: size, height: size).scaleEffect(1.1 + level * 0.35)
            Circle().fill(tone).frame(width: size, height: size)
        }
        .frame(width: size * 1.5, height: size * 1.5)
        .animation(still ? nil : .linear(duration: 0.08), value: level)
    }
}

/// How long until a pause sends.
private struct SoonBar: View {
    var soon: LiveVoice.Soon

    var body: some View {
        TimelineView(.animation) { context in
            let done = min(1, max(0, context.date.timeIntervalSince(soon.from) / max(soon.seconds, 0.1)))
            Capsule().fill(Mint.soft).frame(height: 3)
                .overlay(alignment: .leading) {
                    Capsule().fill(LiveColors.listen).scaleEffect(x: done, anchor: .leading)
                }
        }
        .accessibilityElement()
        .accessibilityLabel("Sending after the pause")
    }
}

/// The button next to the hint: Cancel during a reply, or the way on after a problem.
private struct DockButton: View {
    enum Kind { case cancel, retry, settings }

    var button: Kind
    let live: LiveVoice

    var body: some View {
        switch button {
        case .cancel:
            pill("Cancel", primary: false) { live.cancelReply() }
                .accessibilityHint("Stops the reply and leaves live mode")
        case .retry:
            pill("Try again", primary: true) { live.retry() }
        case .settings:
            pill("Open Settings", primary: true) { live.openSettings() }
        }
    }

    private func pill(_ title: String, primary: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(.body(13, .semibold))
                .foregroundStyle(primary ? .white : Mint.ink)
                .padding(.horizontal, 14).frame(minHeight: 32)
                .background(Capsule().fill(primary ? LiveColors.listen : Mint.surface))
                .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1.5))
                .frame(minHeight: 44)
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}

private struct StepRow: View {
    var text: String

    var body: some View {
        HStack(spacing: 6) {
            ProgressView().controlSize(.mini).tint(Mint.muted)
            Text(text).font(.body(13)).foregroundStyle(Mint.muted).lineLimit(2)
        }
    }
}

extension View {
    /// Runs `close` each time live voice starts, so a sheet or cover this view shows does not
    /// hide the chat live voice talks in.
    func closesForLive(_ close: @escaping () -> Void) -> some View {
        modifier(ClosesForLive(close: close))
    }
}

private struct ClosesForLive: ViewModifier {
    @Environment(AppModel.self) private var model
    let close: () -> Void

    func body(content: Content) -> some View {
        content.onChange(of: model.live.launches) { close() }
    }
}
