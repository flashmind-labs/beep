import AVFoundation
import Speech

/// Dictation into the composer, as the mic button on Android: partial results fill the draft
/// while the user speaks, and it stops on its own once they pause.
@MainActor @Observable final class VoiceInput {
    private(set) var listening = false
    private let recognizer = SFSpeechRecognizer()
    private var engine: AVAudioEngine?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?

    /// Whether dictation works in this language at all; it may still be offline for a moment.
    var available: Bool { recognizer != nil }

    /// Asks for the microphone and speech recognition the first time, then listens.
    func start(_ text: @escaping @MainActor (String) -> Void) async {
        guard !listening, let recognizer, recognizer.isAvailable else { return }
        let speech = await withCheckedContinuation { done in
            SFSpeechRecognizer.requestAuthorization { done.resume(returning: $0) }
        }
        guard speech == .authorized, await AVAudioApplication.requestRecordPermission() else { return }
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)
            let engine = AVAudioEngine()
            let request = SFSpeechAudioBufferRecognitionRequest()
            request.shouldReportPartialResults = true
            let input = engine.inputNode
            input.installTap(onBus: 0, bufferSize: 1024, format: input.outputFormat(forBus: 0)) { buffer, _ in
                request.append(buffer)
            }
            engine.prepare()
            try engine.start()
            self.engine = engine
            self.request = request
            listening = true
            task = recognizer.recognitionTask(with: request) { [weak self] result, error in
                let spoken = result?.bestTranscription.formattedString
                let final = result?.isFinal == true || error != nil
                Task { @MainActor in
                    if let spoken, !spoken.isEmpty { text(spoken) }
                    if final { self?.stop() }
                }
            }
        } catch {
            stop()
        }
    }

    func stop() {
        engine?.stop()
        engine?.inputNode.removeTap(onBus: 0)
        request?.endAudio()
        task?.finish()
        engine = nil
        request = nil
        task = nil
        listening = false
        // Handing the audio back to other apps; nothing to do if iOS refuses.
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}
