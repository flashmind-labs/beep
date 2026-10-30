import ActivityKit
import AppIntents

/// The Live Activity a live voice session shows in the Dynamic Island and on the Lock Screen.
/// In both the app and the widget target: the app starts and updates it, the widget draws it.
struct LiveVoiceAttributes: ActivityAttributes {
    enum Phase: String, Codable, Hashable {
        case listening, thinking, speaking
    }

    struct ContentState: Codable, Hashable {
        var phase: Phase
        /// What was said so far, the tool step, or Beep's line, by phase.
        var line: String
    }
}

/// The End button on the Live Activity: leaves live voice as the close button does.
struct EndLiveIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "End live voice"
    static let isDiscoverable = false

    init() {}

    @MainActor
    func perform() async throws -> some IntentResult {
        #if !BEEP_WIDGET
        LiveLauncher.end()
        #endif
        return .result()
    }
}
