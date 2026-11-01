import AVFoundation
import Observation
import os

private let speakLog = Logger(subsystem: "com.flashmind.beep", category: "speak")

/// The composer's speaker button: reads the reply that is loading aloud as it streams, sentence
/// by sentence, through a `ReplyVoice` from the bridge. The model hands it the chat each time the
/// reply grows; the bridge says which MP3 to play next, and this plays them in turn at live
/// voice's speed. It goes on after the reply finished until every sentence played.
@MainActor
@Observable
final class ReplySpeaker {
    /// Whether a reply is being read out, from the tap until the last sentence played.
    private(set) var speaking = false

    @ObservationIgnored private unowned let model: AppModel
    @ObservationIgnored private var voice: ReplyVoice?
    @ObservationIgnored private var events: AsyncStream<ReplyVoiceEvent>.Continuation?
    @ObservationIgnored private var run: Task<Void, Never>?
    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private var playerEnd: PlayerEnd?
    @ObservationIgnored private var interruptions: NSObjectProtocol?
    /// Counts starts and stops, so events and players from an earlier reading are dropped.
    @ObservationIgnored private var attempt = 0
    /// The chat being read, and the message whose reply it is.
    @ObservationIgnored private var chatId = ""
    @ObservationIgnored private var promptId = ""

    init(model: AppModel) {
        self.model = model
    }

    /// The button: starts reading the reply that runs now, or stops.
    func toggle() {
        if speaking { stop() } else { start() }
    }

    /// Starts reading the open chat's running reply. Live voice owns the audio while it is on.
    func start() {
        guard !speaking, !model.live.active, model.chat.busy,
            let prompt = model.chat.messages.last(where: { $0.role != .assistant })
        else { return }
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .spokenAudio, options: .duckOthers)
            try session.setActive(true)
        } catch {
            speakLog.error("audio did not start: \(describe(error), privacy: .public)")
            return
        }
        speaking = true
        chatId = model.chat.id
        promptId = prompt.id
        attempt += 1
        let current = attempt
        interruptions = NotificationCenter.default.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let type = (note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionType.init)
            let reason = (note.userInfo?[AVAudioSessionInterruptionReasonKey] as? UInt)
                .flatMap(AVAudioSession.InterruptionReason.init)
            // A call or another app took the audio; the player will not say it finished.
            guard type == .began, reason != .appWasSuspended else { return }
            Task { @MainActor in self?.stop() }
        }
        let (stream, continuation) = AsyncStream.makeStream(of: ReplyVoiceEvent.self)
        events = continuation
        run = Task { [weak self] in
            guard let self else { return }
            do {
                let voice = try await self.model.speakReply(ReplyListener(continuation))
                guard self.attempt == current else { return }
                self.voice = voice
            } catch {
                guard self.attempt == current else { return }
                speakLog.error("reading did not start: \(describe(error), privacy: .public)")
                return self.stop()
            }
            // What streamed while the reading started.
            self.follow(self.model.chat)
            for await event in stream {
                guard self.attempt == current else { break }
                self.handle(event, attempt: current)
            }
        }
    }

    /// Stops reading and lets go of the audio. Releasing the `ReplyVoice` stops the bridge's side.
    func stop() {
        guard speaking else { return }
        speaking = false
        attempt += 1
        run?.cancel()
        run = nil
        events?.finish()
        events = nil
        voice = nil
        player?.stop()
        player = nil
        playerEnd = nil
        if let interruptions { NotificationCenter.default.removeObserver(interruptions) }
        interruptions = nil
        do {
            try AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        } catch {
            // Something else still plays; the session goes once it stops.
            speakLog.info("audio did not deactivate: \(describe(error), privacy: .public)")
        }
    }

    /// The open chat changed: hands the reply's text so far to the reading, and says the reply
    /// ended once its run did or another message followed it. Another chat on screen stops it.
    func follow(_ chat: Chat) {
        guard speaking, let voice else { return }
        guard chat.id == chatId else { return stop() }
        let at = chat.messages.firstIndex { $0.id == promptId }
        if let at, let reply = chat.messages[(at + 1)...].first(where: { $0.role == .assistant }) {
            voice.update(text: reply.text)
        }
        let latest = chat.messages.last { $0.role != .assistant }?.id == promptId
        if at == nil || !chat.busy || !latest { voice.finish() }
    }

    private func handle(_ event: ReplyVoiceEvent, attempt current: Int) {
        switch event {
        case .play(let id, _, let audio):
            play(audio, id: id, attempt: current)
        case .done:
            stop()
        }
    }

    /// Plays one sentence and tells the reading when it is over. Empty audio is a sentence the
    /// server could not voice, skipped.
    private func play(_ mp3: Data, id: UInt64, attempt current: Int) {
        guard !mp3.isEmpty else {
            voice?.played(id: id)
            return
        }
        do {
            let player = try AVAudioPlayer(data: mp3)
            let end = PlayerEnd { [weak self] in
                Task { @MainActor in self?.played(id, attempt: current) }
            }
            player.delegate = end
            player.enableRate = true
            player.rate = Float(model.live.voiceSpeed)
            guard player.play() else {
                speakLog.error("sentence \(id, privacy: .public) did not play")
                voice?.played(id: id)
                return
            }
            self.player = player
            playerEnd = end
        } catch {
            speakLog.error("sentence \(id, privacy: .public) did not load: \(describe(error), privacy: .public)")
            voice?.played(id: id)
        }
    }

    private func played(_ id: UInt64, attempt current: Int) {
        guard attempt == current else { return }
        player = nil
        playerEnd = nil
        voice?.played(id: id)
    }
}

/// Says when a sentence finished playing, or could not be decoded.
private final class PlayerEnd: NSObject, AVAudioPlayerDelegate {
    private let done: () -> Void

    init(_ done: @escaping () -> Void) {
        self.done = done
    }

    func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) { done() }
    func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: Error?) { done() }
}

/// Passes the reading's events to the main actor, in order.
private final class ReplyListener: ReplyVoiceListener {
    private let events: AsyncStream<ReplyVoiceEvent>.Continuation

    init(_ events: AsyncStream<ReplyVoiceEvent>.Continuation) {
        self.events = events
    }

    func onEvent(event: ReplyVoiceEvent) {
        events.yield(event)
    }
}
