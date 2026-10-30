import AppIntents
import SwiftUI
import WidgetKit

/// The Talk to Beep control, for Control Center and the Lock Screen: opens Beep in live voice,
/// with the same launch as the Action Button.
@available(iOS 18.0, *)
struct TalkToBeepControl: ControlWidget {
    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: "TalkToBeep") {
            ControlWidgetButton(action: TalkToBeepIntent(from: .control)) {
                Label("Talk to Beep", systemImage: "waveform")
            }
        }
        .displayName("Talk to Beep")
        .description("Start live voice with Beep.")
    }
}
