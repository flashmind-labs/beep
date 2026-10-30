import AVFoundation

/// The audio side of live voice: the mic in as 16 kHz mono PCM16, with the system's echo
/// cancellation, and Beep's spoken lines out through the speaker. Used from the main actor;
/// the taps run on the audio thread and only call the closures given to `start`.
final class LiveAudio {
    struct Failure: LocalizedError {
        let errorDescription: String?
        init(_ text: String) { errorDescription = text }
    }

    private let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    private var playFormat: AVAudioFormat?
    private var running = false

    /// Starts the mic and the player. `mic` gets each chunk of 16 kHz PCM16 and `voice` the
    /// level of what plays, both on the audio thread.
    func start(mic: @escaping @Sendable (Data) -> Void, voice: @escaping @Sendable (Float) -> Void) throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetooth])
        try session.setActive(true)

        let input = engine.inputNode
        try input.setVoiceProcessingEnabled(true)
        let heard = input.outputFormat(forBus: 0)
        guard heard.sampleRate > 0, heard.channelCount > 0 else { throw Failure("No microphone") }
        guard let wire = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true),
              let converter = AVAudioConverter(from: heard, to: wire),
              let playFormat = AVAudioFormat(standardFormatWithSampleRate: 24_000, channels: 1)
        else { throw Failure("This microphone's format is not supported") }
        self.playFormat = playFormat

        // About 100 ms a chunk.
        let frames = AVAudioFrameCount(heard.sampleRate / 10)
        input.installTap(onBus: 0, bufferSize: frames, format: heard) { buffer, _ in
            let capacity = AVAudioFrameCount(Double(buffer.frameLength) * wire.sampleRate / heard.sampleRate) + 32
            guard let out = AVAudioPCMBuffer(pcmFormat: wire, frameCapacity: capacity) else { return }
            var fed = false
            var error: NSError?
            converter.convert(to: out, error: &error) { _, status in
                if fed {
                    status.pointee = .noDataNow
                    return nil
                }
                fed = true
                status.pointee = .haveData
                return buffer
            }
            guard error == nil, out.frameLength > 0, let samples = out.int16ChannelData else { return }
            mic(Data(bytes: samples[0], count: Int(out.frameLength) * MemoryLayout<Int16>.size))
        }

        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: playFormat)
        player.installTap(onBus: 0, bufferSize: 1024, format: playFormat) { buffer, _ in
            voice(Self.level(buffer))
        }
        engine.prepare()
        try engine.start()
        player.play()
        running = true
    }

    /// Plays one MP3 line after any still playing; `done` runs on an audio thread once it was
    /// heard, or when the player stops.
    func play(_ mp3: Data, done: @escaping @Sendable () -> Void) throws {
        guard running, let playFormat else { throw Failure("Audio is off") }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("live-\(UUID().uuidString).mp3")
        try mp3.write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        let file = try AVAudioFile(forReading: url)
        guard let read = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length))
        else { throw Failure("Could not read the audio") }
        try file.read(into: read)
        let buffer = try Self.convert(read, to: playFormat)
        player.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { _ in done() }
        if !player.isPlaying { player.play() }
    }

    /// Drops what plays and what is queued.
    func stopPlaying() {
        guard running else { return }
        player.stop()
        player.play()
    }

    func stop() {
        guard running else { return }
        running = false
        player.stop()
        engine.inputNode.removeTap(onBus: 0)
        player.removeTap(onBus: 0)
        engine.stop()
        // Lets other apps' audio come back up.
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

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
