import AppIntents

/// Where live voice was started from, which sets where the launch glow comes in.
enum LiveOrigin: String, AppEnum {
    case actionButton, sideButton, control, app

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Start"
    static let caseDisplayRepresentations: [LiveOrigin: DisplayRepresentation] = [
        .actionButton: "Action Button",
        .sideButton: "Side button",
        .control: "Control",
        .app: "Beep",
    ]
}

/// Opens Beep in live voice. The Action Button, Back Tap and Siri run it through the App
/// Shortcut; the Control Center and Lock Screen controls and the Home Screen widget run it
/// with `from: .control`. This file is in the widget target too, where `perform` does nothing:
/// `openAppWhenRun` brings up the app, which runs the intent itself.
struct TalkToBeepIntent: AppIntent {
    static let title: LocalizedStringResource = "Talk to Beep"
    static let description = IntentDescription("Opens Beep and starts talking with it, hands free.")
    static let openAppWhenRun = true

    @Parameter(title: "From", default: .actionButton)
    var from: LiveOrigin

    init() {}

    init(from: LiveOrigin) {
        self.from = from
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        #if !BEEP_WIDGET
        LiveLauncher.launch(from: from)
        #endif
        return .result()
    }
}

#if !BEEP_WIDGET
/// Pressing and holding the side button, where iOS offers it to apps (iPhone in Japan, iOS
/// 26.2 and later). Needs the `com.apple.developer.side-button-access.allow` entitlement.
@available(iOS 26.2, *)
@AppIntent(schema: .assistant.activate)
struct ActivateLiveIntent {
    static let supportedModes: IntentModes = .foreground

    @MainActor
    func perform() async throws -> some IntentResult {
        LiveLauncher.launch(from: .sideButton)
        return .result()
    }
}

/// What the Action Button, Back Tap and Siri can pick without building a shortcut.
struct BeepShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(intent: TalkToBeepIntent(),
                    phrases: ["Talk to \(.applicationName)", "Start live voice in \(.applicationName)"],
                    shortTitle: "Talk to Beep",
                    systemImageName: "waveform")
    }
}
#endif
