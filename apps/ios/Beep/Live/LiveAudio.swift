import AVFoundation
import os

private let audioLog = Logger(subsystem: "com.flashmind.beep", category: "live audio")

/// The audio side of live voice: the mic in as 16 kHz mono PCM16, with the system's echo
/// cancellation, and Beep's spoken lines out through the speaker. Used from the main actor;
/// the taps run on the audio thread and only call the closures given to `start`.
///
/// When the route changes (headphones in or out, a Bluetooth headset), the engine's
/// configuration changes, it refreshes the engine's taps and connections. A media services
/// reset replaces the engine. Lines playing then count as played, so the session does not wait.
final class LiveAudio {
    struct Failure: LocalizedError {
        let errorDescription: String?
        init(_ text: String) { errorDescription = text }
    }

    /// A short sound for when nobody is looking at the screen.
    enum Chime {
        /// Beep listens again after a reply.
        case listening
        /// Live voice ended.
        case left
    }

    private enum State {
        case off, running, paused
    }

    /// Why the audio could not come back after a change; called on the main queue.
    var lost: ((String) -> Void)?

    private var engine = AVAudioEngine()
    private var player = AVAudioPlayerNode()
    /// Plays Beep's voice faster at the same pitch.
    private var speed = LiveAudio.makeSpeed()
    private var playFormat: AVAudioFormat?
    private var state = State.off
    private var mic: (@Sendable (Data) -> Void)?
    private var voice: (@Sendable (Float) -> Void)?
    private var observers: [NSObjectProtocol] = []
    private var rebuildSoon: DispatchWorkItem?
    /// Set while this class takes an engine down or builds one, whose own configuration
    /// changes are not a reason to build it again.
    private var changing = false
    /// What to call once each line still queued has played, by line.
    private let lock = NSLock()
    private var playing: [UUID: @Sendable () -> Void] = [:]
    /// One still playing its last chime. Its finish turns voice processing off and lets go of
    /// the shared audio session, so a new start finishes it first rather than have that land
    /// under the new mic. Used from the main actor only.
    nonisolated(unsafe) private static var stopping: LiveAudio?

    /// Starts the mic and the player. `mic` gets each chunk of 16 kHz PCM16 and `voice` the
    /// level of what plays, both on the audio thread.
    func start(mic: @escaping @Sendable (Data) -> Void, voice: @escaping @Sendable (Float) -> Void) throws {
        Self.stopping?.finish()
        self.mic = mic
        self.voice = voice
        do {
            try Self.configureSession()
            try build()
        } catch {
            // Nothing else will stop it: undo what got as far as starting.
            tearDown()
            self.mic = nil
            self.voice = nil
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
            throw error
        }
        observe()
    }

    /// Plays one MP3 line after any still playing; `done` runs on an audio thread once it was
    /// heard, when the player stops, or when the engine is built again.
    func play(_ mp3: Data, done: @escaping @Sendable () -> Void) throws {
        // A configuration change stops the engine until it is built again, and starting the
        // player on a stopped engine raises an exception.
        guard state == .running, engine.isRunning, let playFormat else { throw Failure("Audio is off") }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("live-\(UUID().uuidString).mp3")
        try mp3.write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        let file = try AVAudioFile(forReading: url)
        guard let read = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length))
        else { throw Failure("Could not read the audio") }
        try file.read(into: read)
        let buffer = try Self.convert(read, to: playFormat)
        let line = UUID()
        lock.withLock { playing[line] = done }
        player.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { [weak self] _ in self?.played(line) }
        if !player.isPlaying { player.play() }
    }

    /// Plays `chime` after anything queued; `done` runs on an audio thread once it was heard,
    /// or at once when the audio is off.
    func chime(_ chime: Chime, done: (@Sendable () -> Void)? = nil) {
        guard state == .running, engine.isRunning, let playFormat, let buffer = Self.tone(chime, format: playFormat) else {
            done?()
            return
        }
        player.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { _ in done?() }
        if !player.isPlaying { player.play() }
    }

    /// Drops what plays and what is queued.
    func stopPlaying() {
        guard state == .running, player.engine != nil else { return }
        player.stop()
        if engine.isRunning { player.play() }
    }

    /// Another app took the audio, such as a call: holds still until `resume`.
    func pause() {
        guard state == .running else { return }
        state = .paused
        engine.pause()
    }

    /// Takes the audio back after `pause`, with the engine built again for whatever route
    /// there is now.
    func resume() throws {
        guard state == .paused else { return }
        try AVAudioSession.sharedInstance().setActive(true)
        try rebuild()
    }

    /// Stops everything, after `chime` when one is given. Safe to call more than once.
    func stop(after chime: Chime? = nil) {
        guard let chime, state == .running, engine.isRunning else { return finish() }
        Self.stopping = self
        // The caller lets go of this at once, so the closures hold it until the chime ends,
        // on an audio thread, or a second passes; the engine stops on the main thread.
        self.chime(chime) { DispatchQueue.main.async { self.finish() } }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.finish() }
    }

    private func finish() {
        if Self.stopping === self { Self.stopping = nil }
        guard state != .off else { return }
        state = .off
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers = []
        rebuildSoon?.cancel()
        rebuildSoon = nil
        tearDown()
        finishPlaying()
        mic = nil
        voice = nil
        audioLog.info("audio stopped")
        // Lets other apps' audio come back up.
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    // MARK: Engine

    private static func configureSession() throws {
        let session = AVAudioSession.sharedInstance()
        var options: AVAudioSession.CategoryOptions = [.defaultToSpeaker]
        if #available(iOS 26.0, *) {
            options.insert(.allowBluetoothHFP)
        } else {
            options.insert(.allowBluetooth)
        }
        try session.setCategory(.playAndRecord, mode: .voiceChat, options: options)
        try session.setActive(true)
    }

    /// Sets up the engine: voice processing on the mic, the tap that converts what it hears,
    /// and the player with its level tap.
    private func build() throws {
        guard let mic, let voice else { throw Failure("Audio is off") }
        changing = true
        defer { changing = false }
        let input = engine.inputNode
        if !input.isVoiceProcessingEnabled { try input.setVoiceProcessingEnabled(true) }
        // On by default; said here since a quiet mic after Beep spoke is the first thing to check.
        input.isVoiceProcessingAGCEnabled = true
        let heard = input.outputFormat(forBus: 0)
        guard heard.sampleRate > 0, heard.channelCount > 0 else { throw Failure("No microphone") }
        guard let wire = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true),
              let playFormat = AVAudioFormat(standardFormatWithSampleRate: 24_000, channels: 1)
        else { throw Failure("This microphone's format is not supported") }
        self.playFormat = playFormat
        audioLog.info("mic \(heard.sampleRate, privacy: .public) Hz, \(heard.channelCount, privacy: .public) channels")

        // About 100 ms a chunk. No format: the tap takes the input's own, which a format given
        // here must match exactly or the engine raises an exception.
        let pcm = MicConverter(wire: wire)
        let frames = AVAudioFrameCount(heard.sampleRate / 10)
        input.installTap(onBus: 0, bufferSize: frames, format: nil) { buffer, _ in
            guard let data = pcm.convert(buffer) else { return }
            mic(data)
        }

        if player.engine == nil { engine.attach(player) }
        if speed.engine == nil { engine.attach(speed) }
        engine.connect(player, to: speed, format: playFormat)
        engine.connect(speed, to: engine.mainMixerNode, format: playFormat)
        player.installTap(onBus: 0, bufferSize: 1024, format: playFormat) { buffer, _ in
            voice(Self.level(buffer))
        }
        engine.prepare()
        try engine.start()
        player.play()
        state = .running
    }

    /// Stops the engine and takes its taps off. A player that never got attached, as after a
    /// failed build, is left alone: it raises an exception without an engine. Voice processing
    /// stays enabled during recovery and is released when the audio ends.
    private func tearDown(releasing: Bool = true) {
        changing = true
        defer { changing = false }
        if player.engine != nil {
            player.stop()
            player.removeTap(onBus: 0)
        }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        if releasing {
            // Two voice-processing engines at once leave the newer one's mic quiet.
            // Turn it off before another starts; failure only risks a quiet mic.
            try? engine.inputNode.setVoiceProcessingEnabled(false)
        }
    }

    /// Refreshes taps and connections for the current hardware format. Keeping voice
    /// processing avoids another hardware change caused by turning it off and on again.
    private func rebuild(reset: Bool = false) throws {
        guard state != .off else { return }
        tearDown(releasing: reset)
        finishPlaying()
        if reset {
            engine = AVAudioEngine()
            player = AVAudioPlayerNode()
            speed = Self.makeSpeed()
        }
        try Self.configureSession()
        try build()
    }

    /// Rebuilds once a burst of changes settles: a new headset brings a route change and a
    /// configuration change together.
    private func rebuildAfterChange(reset: Bool = false) {
        rebuildSoon?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.state == .running else { return }
            audioLog.info("refreshing the engine")
            do {
                try self.rebuild(reset: reset)
            } catch {
                audioLog.error("audio could not come back: \(describe(error), privacy: .public)")
                self.lost?(describe(error))
            }
        }
        rebuildSoon = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2, execute: work)
    }

    private func observe() {
        let center = NotificationCenter.default
        observers = [
            center.addObserver(forName: .AVAudioEngineConfigurationChange, object: nil, queue: .main) { [weak self] note in
                guard let self, !self.changing, note.object as? AVAudioEngine === self.engine else { return }
                audioLog.info("engine configuration changed: running \(self.engine.isRunning, privacy: .public)")
                self.rebuildAfterChange()
            },
            center.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] note in
                let reason = (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt)
                    .flatMap(AVAudioSession.RouteChangeReason.init)
                // Only a device coming or going; a category change is this class's own doing.
                guard reason == .newDeviceAvailable || reason == .oldDeviceUnavailable else { return }
                audioLog.info("audio route changed: reason \(reason?.rawValue ?? 0, privacy: .public)")
                self?.rebuildAfterChange()
            },
            center.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
                self?.rebuildAfterChange(reset: true)
            },
        ]
    }

    private func played(_ line: UUID) {
        let done = lock.withLock { playing.removeValue(forKey: line) }
        done?()
    }

    private func finishPlaying() {
        let all = lock.withLock {
            let all = playing.values
            playing = [:]
            return Array(all)
        }
        for done in all { done() }
    }

    /// How much faster than recorded Beep's lines play.
    private static let voiceRate: Float = 1.5

    private static func makeSpeed() -> AVAudioUnitTimePitch {
        let speed = AVAudioUnitTimePitch()
        speed.rate = voiceRate
        return speed
    }

    // MARK: Buffers

    private static func convert(_ buffer: AVAudioPCMBuffer, to format: AVAudioFormat) throws -> AVAudioPCMBuffer {
        if buffer.format == format { return buffer }
        guard let converter = AVAudioConverter(from: buffer.format, to: format) else {
            throw Failure("Could not convert the audio")
        }
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * format.sampleRate / buffer.format.sampleRate) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else {
            throw Failure("Could not convert the audio")
        }
        var fed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if fed {
                status.pointee = .endOfStream
                return nil
            }
            fed = true
            status.pointee = .haveData
            return buffer
        }
        if let error { throw error }
        return out
    }

    /// Two soft notes, rising for listening and falling for leaving.
    private static func tone(_ chime: Chime, format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let notes: [Double] = chime == .listening ? [660, 880] : [880, 587]
        let rate = format.sampleRate
        let note = Int(rate * 0.09)
        let total = note * notes.count
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(total)),
              let out = buffer.floatChannelData?[0]
        else { return nil }
        buffer.frameLength = AVAudioFrameCount(total)
        for (index, pitch) in notes.enumerated() {
            for i in 0..<note {
                // A quick rise and a longer fall keep each note from clicking.
                let envelope = min(1, Double(i) / (rate * 0.005), Double(note - i) / (rate * 0.04))
                out[index * note + i] = Float(sin(2 * .pi * pitch * Double(i) / rate) * envelope * 0.2)
            }
        }
        return buffer
    }

    /// RMS of a float buffer, scaled to about 0 to 1 for speech.
    private static func level(_ buffer: AVAudioPCMBuffer) -> Float {
        guard let samples = buffer.floatChannelData, buffer.frameLength > 0 else { return 0 }
        let count = Int(buffer.frameLength)
        var sum: Float = 0
        for i in 0..<count {
            sum += samples[0][i] * samples[0][i]
        }
        return min(1, (sum / Float(count)).squareRoot() * 4)
    }
}

/// Turns the mic's buffers into 16 kHz mono PCM16 on the tap's thread, which is the only one
/// that uses it. A buffer in another format than the last gets a new converter, so a format
/// that changes under the tap does not silence the mic.
private final class MicConverter: @unchecked Sendable {
    private let wire: AVAudioFormat
    private var converter: AVAudioConverter?
    private var chunks = 0
    private var failures = 0

    init(wire: AVAudioFormat) {
        self.wire = wire
    }

    func convert(_ buffer: AVAudioPCMBuffer) -> Data? {
        guard buffer.frameLength > 0, buffer.format.sampleRate > 0 else { return nil }
        if converter?.inputFormat != buffer.format {
            converter = AVAudioConverter(from: buffer.format, to: wire)
            audioLog.info("mic converter for \(buffer.format.sampleRate, privacy: .public) Hz")
        }
        guard let converter else { return failed("no converter") }
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * wire.sampleRate / buffer.format.sampleRate) + 32
        guard let out = AVAudioPCMBuffer(pcmFormat: wire, frameCapacity: capacity) else { return failed("no buffer") }
        var fed = false
        var error: NSError?
        let status = converter.convert(to: out, error: &error) { _, input in
            if fed {
                input.pointee = .noDataNow
                return nil
            }
            fed = true
            input.pointee = .haveData
            return buffer
        }
        guard status != .error, error == nil else { return failed(error?.localizedDescription ?? "conversion failed") }
        guard out.frameLength > 0, let samples = out.int16ChannelData else { return nil }
        chunks += 1
        if chunks == 1 { audioLog.info("first mic chunk") }
        return Data(bytes: samples[0], count: Int(out.frameLength) * MemoryLayout<Int16>.size)
    }

    /// Logs the first few failures, so a mic that sends nothing says why.
    private func failed(_ why: String) -> Data? {
        failures += 1
        if failures <= 3 { audioLog.error("mic chunk dropped: \(why, privacy: .public)") }
        return nil
    }
}
