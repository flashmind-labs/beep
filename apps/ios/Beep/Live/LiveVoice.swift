import ActivityKit
import AVFoundation
import Observation
import SwiftUI

/// Hands a launch from an App Intent to the app's live voice. An intent can run before the
/// app delegate made the model, so a launch waits here until `attach`.
@MainActor
enum LiveLauncher {
    private static weak var model: AppModel?
    private static var pending: LiveOrigin?

    static func launch(from origin: LiveOrigin) {
        guard let model else {
            pending = origin
            return
        }
        model.live.start(from: origin)
    }

    /// The Live Activity's End button.
    static func end() {
        model?.live.close()
    }

    static func attach(_ model: AppModel) {
        self.model = model
        if let origin = pending {
            pending = nil
            model.live.start(from: origin)
        }
    }
}

/// Live voice: talks with Beep through a `LiveSession` from the bridge, which turns what the
/// mic hears into turns and reads the replies out. The session says what to send; this sends
/// it through the chat like a typed message and hands the run's events back, so the chat,
/// its approvals and its history work as usual.
@MainActor
@Observable
final class LiveVoice {
    /// A pause that sends after `seconds`, counted from `from`.
    struct Soon: Equatable {
        let from: Date
        let seconds: Double
    }

    /// Whether the live screen shows.
    private(set) var active = false
    private(set) var origin: LiveOrigin = .app
    /// Counts launches, so the screen plays the launch again for a new one.
    private(set) var launches = 0
    private(set) var view = LiveView(phase: .listening, words: "", line: nil, prev: nil, step: nil, speaking: false)
    private(set) var toast: String?
    private(set) var soon: Soon?
    /// The mic's level while listening, Beep's voice while it speaks, 0 to 1.
    private(set) var level: Float = 0
    /// Why live voice cannot go on, such as no microphone; the screen stays to say so.
    private(set) var problem: String?
    /// The last message sent from live voice, shown above while Beep thinks.
    private(set) var said: String?
    /// The chat live voice talks in; nil for a new one, made with the first message.
    private(set) var chatId: String?

    /// Whether a pause sends what was said, or only "send" does.
    var sendOnPause = UserDefaults.standard.object(forKey: "liveSendOnPause") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(sendOnPause, forKey: "liveSendOnPause")
            session?.setSendOnPause(on: sendOnPause)
        }
    }

    @ObservationIgnored private unowned let model: AppModel
    @ObservationIgnored private var session: LiveSession?
    @ObservationIgnored private var audio: LiveAudio?
    @ObservationIgnored private var events: AsyncStream<LiveEvent>.Continuation?
    @ObservationIgnored private var run: Task<Void, Never>?
    @ObservationIgnored private var toastTask: Task<Void, Never>?
    @ObservationIgnored private var interruptions: NSObjectProtocol?
    @ObservationIgnored private var activity: Activity<LiveVoiceAttributes>?

    init(model: AppModel) {
        self.model = model
    }

    // MARK: Screen

    /// Opens live voice, or plays the launch again when it is open already.
    func start(from origin: LiveOrigin, holding: Bool = false) {
        guard model.onboarded else { return }
        self.origin = origin
        launches += 1
        guard !active else { return }
        active = true
        view = LiveView(phase: holding ? .holding : .listening, words: "", line: nil, prev: nil, step: nil, speaking: false)
        toast = nil
        soon = nil
        level = 0
        problem = nil
        said = nil
        chatId = model.openGroup == nil && !model.chat.messages.isEmpty ? model.chat.id : nil

        startActivity()

        let (stream, continuation) = AsyncStream.makeStream(of: LiveEvent.self)
        events = continuation
        run = Task { [weak self] in
            guard await AVAudioApplication.requestRecordPermission() else {
                self?.fail("Beep can’t hear you. Allow the microphone in Settings, Beep.")
                return
            }
            guard let self, self.active else { return }
            do {
                let session = try await self.model.startLive(Listener(continuation), holding: holding)
                guard self.active, self.events != nil else { return session.stop() }
                self.session = session
                try self.startAudio(for: session)
            } catch {
                // With a session, it leaves and says why; without one there is nothing to leave.
                if let session = self.session { session.fail(reason: describe(error)) } else { self.fail(describe(error)) }
            }
            for await event in stream { self.handle(event) }
        }
    }

    /// The close button: ends the session, keeping what was said and not sent as a draft.
    func close() {
        if let session {
            session.stop()
        } else {
            end()
            active = false
        }
    }

    /// A tap on the orb: stops listening when listening, otherwise skips the answer.
    func tap() { session?.tap() }
    func holdStart() { session?.holdStart() }
    func holdEnd() { session?.holdEnd() }
    func holdCancel() { session?.holdCancel() }

    // MARK: The chat's run

    /// An event of the run answering what live voice sent.
    func reply(_ event: RunEvent) { session?.replyEvent(event: event) }
    /// That run could not start.
    func replyFailed() { session?.replyFailed() }

    // MARK: Session

    private func startAudio(for session: LiveSession) throws {
        let audio = LiveAudio()
        try audio.start(mic: { [weak self] pcm in
            let heard = session.pushAudio(pcm: pcm)
            Task { @MainActor in self?.hear(heard, speaking: false) }
        }, voice: { [weak self] level in
            Task { @MainActor in self?.hear(level, speaking: true) }
        })
        self.audio = audio
        interruptions = NotificationCenter.default.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let began = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init) == .began
            // A call took the audio; end as the close button would.
            if began { Task { @MainActor in self?.close() } }
        }
    }

    private func hear(_ value: Float, speaking: Bool) {
        // The orb and the glow follow the voice playing while Beep speaks, the mic otherwise.
        guard speaking == view.speaking else { return }
        level = level * 0.5 + value * 0.5
    }

    private func handle(_ event: LiveEvent) {
        switch event {
        case .view(let view):
            self.view = view
            if view.phase != .listening { soon = nil }
            updateActivity()
        case .toast(let text):
            show(text)
        case .soon(let afterMs):
            soon = afterMs.map { Soon(from: .now, seconds: Double($0) / 1000) }
        case .send(let text, let newChat):
            said = text
            soon = nil
            if newChat { chatId = nil }
            chatId = model.sendSpoken(text, in: chatId)
        case .play(let id, _, let mp3, _):
            guard let audio, let session else { return }
            do {
                try audio.play(mp3) { session.played(id: id) }
            } catch {
                session.played(id: id)
            }
        case .stopAudio:
            audio?.stopPlaying()
        case .left(let draft, _, let failure):
            if let draft, !draft.isEmpty { model.pendingDraft = draft }
            end()
            if let failure {
                problem = failure
            } else {
                active = false
            }
        }
    }

    private func show(_ text: String) {
        toast = text
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1.6))
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    /// Leaves the screen up with why live voice stopped; the close button dismisses it.
    private func fail(_ reason: String) {
        end()
        problem = reason
    }

    /// Stops the audio and lets go of the session.
    private func end() {
        audio?.stop()
        audio = nil
        session = nil
        events?.finish()
        events = nil
        soon = nil
        level = 0
        if let interruptions { NotificationCenter.default.removeObserver(interruptions) }
        interruptions = nil
        endActivity()
    }

    // MARK: Live Activity

    private var activityState: LiveVoiceAttributes.ContentState {
        switch view.phase {
        case .listening, .holding:
            LiveVoiceAttributes.ContentState(phase: .listening, line: String(view.words.suffix(90)))
        case .thinking:
            LiveVoiceAttributes.ContentState(phase: .thinking, line: view.step ?? said ?? "")
        case .speaking:
            LiveVoiceAttributes.ContentState(phase: .speaking, line: view.line ?? "")
        }
    }

    private func startActivity() {
        guard activity == nil, ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        // Without it the session still runs; only the Dynamic Island stays empty.
        activity = try? Activity.request(attributes: LiveVoiceAttributes(),
                                         content: ActivityContent(state: activityState, staleDate: nil))
    }

    private func updateActivity() {
        guard let activity else { return }
        let state = activityState
        guard state != activity.content.state else { return }
        Task { await activity.update(ActivityContent(state: state, staleDate: nil)) }
    }

    private func endActivity() {
        guard let activity else { return }
        self.activity = nil
        Task { await activity.end(nil, dismissalPolicy: .immediate) }
    }
}

/// Passes the session's events to the main actor, in order.
private final class Listener: LiveListener {
    private let events: AsyncStream<LiveEvent>.Continuation

    init(_ events: AsyncStream<LiveEvent>.Continuation) {
        self.events = events
    }

    func onEvent(event: LiveEvent) {
        events.yield(event)
    }
}
