import AVFoundation
import Speech

/// Dictation into the composer, as the mic button on Android: partial results fill the draft
/// while the user speaks, and it stops on its own once they pause.
@MainActor @Observable final class VoiceInput {
    /// Why dictation could not start.
    struct Problem: Equatable {
        let text: String
        /// Fixed in Settings, such as the microphone or speech recognition turned off for Beep.
        var settings = false
    }

    private(set) var listening = false
    /// Why the last start failed, for the composer to show; cleared by `dismissProblem`.
    private(set) var problem: Problem?
    private let recognizer = SFSpeechRecognizer()
    private var engine: AVAudioEngine?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    /// Which dictation results belong to; `cancel` clears it, so the final result iOS still hands
    /// over afterwards cannot fill a draft that was sent or cleared in the meantime.
    private var session: UUID?

    /// Whether dictation works in this language at all; it may still be offline for a moment.
    var available: Bool { recognizer != nil }

    /// Asks for the microphone and speech recognition the first time, then listens.
    func start(_ text: @escaping @MainActor (String) -> Void) async {
        guard !listening else { return }
        guard let recognizer else {
            problem = Problem(text: "Dictation isn’t available in this language.")
            return
        }
        guard recognizer.isAvailable else {
            problem = Problem(text: "Dictation isn’t available right now. Try again in a moment.")
            return
        }
        let speech = await withCheckedContinuation { done in
            SFSpeechRecognizer.requestAuthorization { done.resume(returning: $0) }
        }
        guard speech == .authorized else {
            problem = Problem(text: "Allow Speech Recognition for Beep in Settings to dictate.", settings: true)
            return
        }
        guard await AVAudioApplication.requestRecordPermission() else {
            problem = Problem(text: "Allow the microphone for Beep in Settings to dictate.", settings: true)
            return
        }
        do {
            let audio = AVAudioSession.sharedInstance()
            try audio.setCategory(.record, mode: .measurement, options: .duckOthers)
            try audio.setActive(true, options: .notifyOthersOnDeactivation)
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
            let current = UUID()
            self.session = current
            task = recognizer.recognitionTask(with: request) { [weak self] result, error in
                let spoken = result?.bestTranscription.formattedString
                let final = result?.isFinal == true || error != nil
                Task { @MainActor in
                    guard let self, self.session == current else { return }
                    if let spoken, !spoken.isEmpty { text(spoken) }
                    if final { self.stop() }
                }
            }
        } catch {
            stop()
            problem = Problem(text: "Couldn’t start dictation: \(describe(error))")
        }
    }

    func dismissProblem() { problem = nil }

    /// Stops listening and drops anything still to come, as on sending or leaving the chat.
    func cancel() {
        session = nil
        stop()
    }

    /// Stops listening; what was heard still comes in.
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
