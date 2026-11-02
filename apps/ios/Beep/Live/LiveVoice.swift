import ActivityKit
import AVFoundation
import MediaPlayer
import Observation
import os
import SwiftUI
import UIKit

private let liveLog = Logger(subsystem: "com.flashmind.beep", category: "live")

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

    /// Why live voice cannot go on; the dock stays to say so and offers a way on.
    struct Problem: Equatable {
        let text: String
        /// Fixed in Settings, such as the microphone turned off for Beep.
        var settings = false
    }

    /// Why live voice ended, for the line over the chat's composer and the Live Activity.
    enum Ending: Equatable {
        case left, quiet, call, cancelled
        case failed(String)

        var note: String {
            switch self {
            case .left: "Left live mode."
            case .quiet: "Live mode ended after 2 minutes quiet."
            case .call: "Live mode ended for a call."
            case .cancelled: "Cancelled the reply."
            case .failed(let reason): reason.hasSuffix(".") ? reason : reason + "."
            }
        }
    }

    /// A live start continues the chat on screen when it was active this recently; otherwise
    /// it starts a new one. A start from inside the app always continues it.
    static let freshChat: TimeInterval = 5 * 60

    /// Whether live voice is on: the dock stands in for the chat's composer and the edge glows.
    private(set) var active = false
    private(set) var origin: LiveOrigin = .app
    /// Counts launches: each one plays the glow's launch again and closes what covers the chat.
    private(set) var launches = 0
    private(set) var view = LiveView(phase: .connecting, words: "", line: nil, prev: nil, step: nil, speaking: false,
                                     voiceUnavailable: false)
    private(set) var toast: String?
    private(set) var soon: Soon?
    /// The mic's level while listening, Beep's voice while it speaks, 0 to 1.
    private(set) var level: Float = 0
    /// Another app has the audio, such as a call; live voice carries on after.
    private(set) var paused = false
    private(set) var problem: Problem?
    /// The last message sent from live voice, shown in the dock while Beep thinks.
    private(set) var said: String?
    /// The chat live voice talks in; nil for a new one, made with the first message.
    private(set) var chatId: String?
    /// Why live voice last ended, shown over the chat's composer until dismissed.
    private(set) var ended: String?

    /// Whether a pause sends what was said, or only "send" does.
    var sendOnPause = UserDefaults.standard.object(forKey: "liveSendOnPause") as? Bool ?? true {
        didSet {
            UserDefaults.standard.set(sendOnPause, forKey: "liveSendOnPause")
            session?.setSendOnPause(on: sendOnPause)
        }
    }

    /// How fast Beep's lines play, from 1 to 2.
    var voiceSpeed = UserDefaults.standard.object(forKey: "liveVoiceSpeed") as? Double ?? 1.25 {
        didSet {
            UserDefaults.standard.set(voiceSpeed, forKey: "liveVoiceSpeed")
            audio?.rate = Float(voiceSpeed)
        }
    }

    /// The approval the reply waits on, answered with the chat's card when the chat holds it,
    /// or else with one in the dock.
    var asking: ApprovalPrompt? {
        guard active, problem == nil, let approval else { return nil }
        let card = model.chat.messages.lazy.flatMap(\.approvals).first { $0.queryId == approval.prompt.queryId }
        let prompt = card ?? approval.prompt
        return prompt.answer == nil && !prompt.withdrawn ? prompt : nil
    }

    /// The mic is on and the transcription is not open yet; what is said meanwhile is kept.
    var connecting: Bool { problem == nil && view.phase == .connecting }

    /// Beep is writing or reading out a reply, which Cancel stops.
    var replying: Bool { problem == nil && (view.phase == .thinking || view.phase == .speaking) }

    @ObservationIgnored private unowned let model: AppModel
    @ObservationIgnored private var session: LiveSession?
    @ObservationIgnored private var audio: LiveAudio?
    @ObservationIgnored private var events: AsyncStream<LiveEvent>.Continuation?
    @ObservationIgnored private var run: Task<Void, Never>?
    /// Counts `begin`s. A run task from an earlier one, such as one still connecting when
    /// Try again started another, must not take over the session or handle its events.
    @ObservationIgnored private var attempt = 0
    @ObservationIgnored private var toastTask: Task<Void, Never>?
    @ObservationIgnored private var interruptions: NSObjectProtocol?
    @ObservationIgnored private var activity: Activity<LiveVoiceAttributes>?
    /// The headset's play and pause commands, while live.
    @ObservationIgnored private var remote: [(MPRemoteCommand, Any)] = []
    /// Why the session was asked to leave, for when it says it left.
    @ObservationIgnored private var leaving: Ending?
    /// The approval the session waits on, from its `Approval` event, and its reply's number.
    private var approval: (reply: UInt64, prompt: ApprovalPrompt)?
    /// Lines the server could not voice, each shown for a moment in turn instead of played.
    @ObservationIgnored private var silentLines: [Task<Void, Never>] = []

    init(model: AppModel) {
        self.model = model
    }

    // MARK: Dock

    /// Opens live voice in the chat on screen, or in a new one when the chat on screen is not
    /// one to go on with. While it is open, a press of the Action Button or the side button
    /// acts like a tap on the orb, or tries again after a problem; other launches, such as the
    /// widget or the Live Activity bringing the app up, change nothing.
    func start(from origin: LiveOrigin, holding: Bool = false) {
        guard model.onboarded else { return }
        guard !active else {
            guard origin == .actionButton || origin == .sideButton else { return }
            if problem != nil { retry() } else { tap() }
            return
        }
        self.origin = origin
        launches += 1
        active = true
        ended = nil
        let continuing = continues(from: origin)
        chatId = continuing ? model.chat.id : nil
        if !continuing {
            // The new chat shows from the start, so the dock sits under it rather than an old one.
            model.openGroup = nil
            if !model.chat.messages.isEmpty { model.newChat() }
        }
        // The dock takes the composer's place, so the keyboard goes with it.
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        begin(holding: holding)
    }

    /// The close button and the Live Activity's End: leaves, keeping what was said and not
    /// sent as a draft. Mid-reply it cancels the reply too, as Cancel does.
    func close() {
        guard let session else {
            end(note: nil)
            active = false
            return
        }
        if replying { return cancelReply() }
        leaving = .left
        session.stop()
    }

    /// Cancel, while Beep thinks or speaks: leaves, and the session has the reply stopped on
    /// the server too (`CancelReply`); the question comes back as the draft, as on the Mac.
    func cancelReply() {
        guard let session else { return }
        leaving = .cancelled
        session.stop()
    }

    /// Try again, after a problem that Settings cannot fix.
    func retry() {
        guard active, problem != nil else { return }
        begin(holding: false)
    }

    func openSettings() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        UIApplication.shared.open(url)
    }

    /// The line over the composer saying why live voice ended goes away.
    func dismissEnded() { ended = nil }

    /// A tap on the orb: leaves when listening, otherwise stops the reply and listens.
    func tap() { session?.tap() }
    func holdStart() { headphoneTalk = false; session?.holdStart() }
    func holdEnd() { headphoneTalk = false; session?.holdEnd() }
    func holdCancel() { headphoneTalk = false; session?.holdCancel() }

    /// The talking now was started by a headphone press rather than the orb, so the next
    /// press sends it.
    private(set) var headphoneTalk = false

    // MARK: The chat's run

    /// An event of the run answering what live voice sent as reply `reply`.
    func reply(_ reply: UInt64, _ event: RunEvent) { session?.replyEvent(reply: reply, event: event) }
    /// That run could not start, or its stream broke for good.
    func replyFailed(_ reply: UInt64, _ failure: ReplyFailure) { session?.replyFailed(reply: reply, failure: failure) }
    /// That run's stream broke and is being fetched again.
    func reconnecting(_ reply: UInt64) { session?.reconnecting(reply: reply) }

    /// Approval `queryId` was answered, in the chat or the dock. Returns the reply
    /// number to read the resumed run out as, when live voice waits on it.
    func answered(_ queryId: String) -> UInt64? {
        guard let approval, approval.prompt.queryId == queryId, let session else { return nil }
        self.approval = nil
        session.approvalAnswered(reply: approval.reply)
        return approval.reply
    }

    // MARK: Session

    /// Whether a start from `origin` goes on in the chat on screen, rather than a new one.
    private func continues(from origin: LiveOrigin) -> Bool {
        guard model.openGroup == nil, !model.chat.messages.isEmpty else { return false }
        if origin == .app { return true }
        guard let last = model.openChatActivity else { return false }
        return Date.now.timeIntervalSince(last) < Self.freshChat
    }

    /// Starts the mic, then the session, which takes what the mic heard while it connected.
    private func begin(holding: Bool) {
        view = LiveView(phase: .connecting, words: "", line: nil, prev: nil, step: nil, speaking: false,
                        voiceUnavailable: false)
        paused = false
        toast = nil
        soon = nil
        level = 0
        problem = nil
        said = nil
        leaving = nil
        approval = nil

        startActivity()

        attempt += 1
        let current = attempt
        let (stream, continuation) = AsyncStream.makeStream(of: LiveEvent.self)
        events = continuation
        run?.cancel()
        run = Task { [weak self] in
            let allowed = await AVAudioApplication.requestRecordPermission()
            guard let self, self.current(current) else { return }
            guard allowed else {
                return self.fail(Problem(text: "Beep can’t hear you. Allow the microphone for Beep in Settings.", settings: true))
            }
            let pipe = MicPipe()
            do {
                try self.startAudio(pipe)
            } catch {
                liveLog.error("audio did not start: \(describe(error), privacy: .public)")
                return self.fail(Problem(text: describe(error)))
            }
            do {
                let session = try await self.model.startLive(Listener(continuation), holding: holding)
                guard self.current(current) else { return session.stop() }
                self.session = session
                pipe.connect(session)
                liveLog.info("session started")
            } catch {
                guard self.current(current) else { return }
                liveLog.error("session did not start: \(describe(error), privacy: .public)")
                return self.fail(Problem(text: describe(error)))
            }
            for await event in stream {
                guard self.current(current) else { break }
                self.handle(event)
            }
        }
    }

    /// Whether `begin` number `attempt` is still the one live voice runs, and has not ended.
    private func current(_ attempt: Int) -> Bool {
        active && events != nil && self.attempt == attempt
    }

    private func startAudio(_ pipe: MicPipe) throws {
        let audio = LiveAudio()
        audio.rate = Float(voiceSpeed)
        audio.lost = { [weak self] reason in self?.lostAudio(reason) }
        try audio.start(mic: { [weak self] pcm in
            guard let heard = pipe.push(pcm) else { return }
            Task { @MainActor in self?.hear(heard, speaking: false) }
        }, voice: { [weak self] level in
            Task { @MainActor in self?.hear(level, speaking: true) }
        })
        self.audio = audio
        interruptions = NotificationCenter.default.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let type = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init)
            let options = AVAudioSession.InterruptionOptions(
                rawValue: note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0)
            let reason = (note.userInfo?[AVAudioSessionInterruptionReasonKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionReason.init)
            guard let type else { return }
            // A late notice of an interruption from while the app was suspended, such as before
            // a launch from the Action Button: the audio is not taken now, and no end follows,
            // so pausing for it would leave the mic off for good.
            if type == .began, reason == .appWasSuspended { return }
            let began = type == .began
            let resume = options.contains(.shouldResume)
            Task { @MainActor in self?.interrupted(began: began, resume: resume) }
        }
        startRemote()
    }

    /// Another app took the audio, or gave it back. A call pauses live voice; when it ends
    /// without the system saying to resume, live voice ends for it.
    private func interrupted(began: Bool, resume: Bool) {
        guard let audio else { return }
        liveLog.info("audio interrupted: began \(began, privacy: .public), resume \(resume, privacy: .public)")
        if began {
            paused = true
            audio.pause()
            updateActivity()
        } else if resume, paused {
            paused = false
            do {
                try audio.resume()
            } catch {
                lostAudio(describe(error))
            }
            updateActivity()
        } else {
            leaveForCall()
        }
    }

    private func leaveForCall() {
        guard let session else {
            end(note: Ending.call.note)
            active = false
            return
        }
        leaving = .call
        session.interrupted()
    }

    /// The audio could not come back after a route or media change.
    private func lostAudio(_ reason: String) {
        if let session { session.fail(reason: reason) } else { fail(Problem(text: reason)) }
    }

    private func hear(_ value: Float, speaking: Bool) {
        // The orb and the glow follow the voice playing while Beep speaks, the mic otherwise.
        guard speaking == view.speaking else { return }
        level = level * 0.5 + value * 0.5
    }

    private func handle(_ event: LiveEvent) {
        switch event {
        case .view(let view):
            if view.phase != self.view.phase {
                liveLog.info("phase \(String(describing: view.phase), privacy: .public)")
            }
            let replied = self.view.phase == .thinking || self.view.phase == .speaking
            self.view = view
            if view.phase != .listening { soon = nil }
            if view.phase == .listening, replied, unseen { audio?.chime(.listening) }
            updateActivity()
        case .toast(let text):
            show(text)
        case .soon(let afterMs):
            soon = afterMs.map { Soon(from: .now, seconds: Double($0) / 1000) }
        case .send(let reply, let text, let newChat):
            said = text
            soon = nil
            approval = nil
            if newChat { chatId = nil }
            chatId = model.sendSpoken(text, in: chatId, reply: reply)
        case .cancelReply(_, let runId):
            model.cancelSpokenRun(runId, in: chatId)
        case .approval(let reply, _, let queryId, let approval):
            self.approval = (reply, ApprovalPrompt(queryId: queryId, approval: approval))
        case .play(let id, let text, let mp3, _):
            guard let audio, let session else { return }
            guard !mp3.isEmpty else { return showSilently(text, id: id) }
            do {
                try audio.play(mp3) { session.played(id: id) }
            } catch {
                session.played(id: id)
            }
        case .stopAudio:
            audio?.stopPlaying()
            dropSilentLines()
        case .left(let draft, let reason, let failure):
            liveLog.info("left: \(String(describing: reason), privacy: .public)")
            let ending: Ending = switch reason {
            case .failed: .failed(failure ?? "Voice is not available right now")
            case .quiet: paused ? .call : .quiet
            case .interrupted: .call
            case .asked: leaving ?? .left
            }
            left(draft: draft, ending)
        }
    }

    /// The server could not voice `text`: the dock shows it as Beep's line for about as
    /// long as saying it would take, then it counts as played.
    private func showSilently(_ text: String, id: UInt64) {
        let seconds = min(8, max(1.5, Double(text.count) / 15))
        let before = silentLines.last
        silentLines.append(Task { [weak self] in
            await before?.value
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled, let self else { return }
            self.silentLines.removeFirst()
            self.session?.played(id: id)
        })
    }

    private func dropSilentLines() {
        for line in silentLines { line.cancel() }
        silentLines = []
    }

    /// The session ended: keeps what was said as the draft and says why it ended.
    private func left(draft: String?, _ ending: Ending) {
        var kept = draft ?? ""
        // Cancel gives back the question whose reply it stopped.
        if ending == .cancelled, kept.isEmpty { kept = said ?? "" }
        if !kept.isEmpty { model.pendingDraft = kept }
        let note = kept.isEmpty ? ending.note : ending.note + " Your words are kept here."
        ended = note
        end(note: note)
        if case .failed(let reason) = ending {
            problem = Problem(text: reason)
        } else {
            active = false
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

    /// Leaves the dock up with why live voice stopped and a way on.
    private func fail(_ problem: Problem) {
        end(note: problem.text)
        self.problem = problem
    }

    /// Whether nobody is looking at the screen: it is locked or Beep is in the background.
    private var unseen: Bool { UIApplication.shared.applicationState != .active }

    /// Stops the audio and lets go of the session. `note` is why it ended, for the Live
    /// Activity's last state; with one, a chime marks the end when nobody is looking.
    private func end(note: String?) {
        stopRemote()
        audio?.stop(after: note != nil && unseen ? .left : nil)
        audio = nil
        session = nil
        events?.finish()
        events = nil
        soon = nil
        level = 0
        paused = false
        approval = nil
        dropSilentLines()
        if let interruptions { NotificationCenter.default.removeObserver(interruptions) }
        interruptions = nil
        endActivity(note: note)
    }

    // MARK: Headset

    /// AirPods and other headsets: their play and pause act like a tap on the orb.
    /// AirPods and other headphones: a press talks and the next one sends, as holding the orb
    /// does; a double press (the next track command) leaves live voice.
    private func startRemote() {
        let center = MPRemoteCommandCenter.shared()
        let press: [MPRemoteCommand] = [center.togglePlayPauseCommand, center.playCommand, center.pauseCommand]
        for command in press {
            command.isEnabled = true
            let target = command.addTarget { [weak self] _ in
                Task { @MainActor in self?.headphonePress() }
                return .success
            }
            remote.append((command, target))
        }
        center.nextTrackCommand.isEnabled = true
        let leave = center.nextTrackCommand.addTarget { [weak self] _ in
            Task { @MainActor in self?.close() }
            return .success
        }
        remote.append((center.nextTrackCommand, leave))
        // The system sends those commands to the app that says it is playing.
        MPNowPlayingInfoCenter.default().nowPlayingInfo = [MPMediaItemPropertyTitle: "Live with Beep"]
    }

    /// A headphone press: starts talking, interrupting Beep if it speaks, or sends what was
    /// said since the last press.
    private func headphonePress() {
        if view.phase == .holding {
            holdEnd()
        } else {
            holdStart()
            headphoneTalk = true
        }
    }

    private func stopRemote() {
        for (command, target) in remote {
            command.removeTarget(target)
            command.isEnabled = false
        }
        remote = []
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
    }

    // MARK: Live Activity

    private var activityState: LiveVoiceAttributes.ContentState {
        if paused { return LiveVoiceAttributes.ContentState(phase: .thinking, line: "Paused") }
        return switch view.phase {
        case .connecting:
            LiveVoiceAttributes.ContentState(phase: .listening, line: "Connecting")
        case .waiting:
            LiveVoiceAttributes.ContentState(phase: .thinking, line: asking.map { "\($0.approval.action)? Open Beep to answer" } ?? "")
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

    /// Ends the Live Activity on why live voice ended, left for the system to take away.
    private func endActivity(note: String?) {
        guard let activity else { return }
        self.activity = nil
        let last = LiveVoiceAttributes.ContentState(phase: .ended, line: note ?? Ending.left.note)
        Task { await activity.end(ActivityContent(state: last, staleDate: nil), dismissalPolicy: .default) }
    }
}

/// Carries the mic's chunks from the audio thread to the session, holding them while the
/// session connects so what was said first is not lost.
private final class MicPipe: @unchecked Sendable {
    /// About ten seconds of 100 ms chunks.
    private static let limit = 100
    private let lock = NSLock()
    private var session: LiveSession?
    private var held: [Data] = []

    /// Hands `pcm` to the session and returns its level, or holds it and returns nil.
    func push(_ pcm: Data) -> Float? {
        lock.withLock {
            guard let session else {
                held.append(pcm)
                if held.count > Self.limit { held.removeFirst() }
                return nil
            }
            return session.pushAudio(pcm: pcm)
        }
    }

    /// The session is ready: it gets what was held, in order, then each chunk as it comes.
    func connect(_ session: LiveSession) {
        lock.withLock {
            for pcm in held { _ = session.pushAudio(pcm: pcm) }
            held = []
            self.session = session
        }
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
