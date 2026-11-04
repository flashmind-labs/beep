import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

/// The Home Screen's Talk to Beep button: runs the same intent as the Action Button, so the app
/// opens straight into live voice. The widget cannot read the app's chats, so it shows no reply.
struct TalkToBeepWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "TalkToBeep", provider: Still()) { _ in TalkToBeepView() }
            .configurationDisplayName("Talk to Beep")
            .description("Start live voice with Beep.")
            .supportedFamilies([.systemSmall, .systemMedium])
    }
}

private struct TalkToBeepView: View {
    @Environment(\.widgetFamily) private var family

    var body: some View {
        Button(intent: TalkToBeepIntent(from: .control)) { content }
            .buttonStyle(.plain)
            .widgetURL(URL(string: "beep://live"))
            .containerBackground(for: .widget) { Mint.page }
    }

    @ViewBuilder private var content: some View {
        if family == .systemMedium {
            HStack(spacing: 14) {
                SmallOrb(phase: .listening, size: 52)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Talk to Beep").font(.body(20)).foregroundStyle(Mint.ink)
                    Text("Live voice, hands free").font(.body(13)).foregroundStyle(Mint.muted)
                }
                Spacer(minLength: 0)
                Label("Talk", systemImage: "waveform")
                    .font(.system(size: 14, weight: .semibold)).foregroundStyle(Mint.onAccent)
                    .padding(.leading, 10).padding(.trailing, 14).padding(.vertical, 9)
                    .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.accent))

            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                SmallOrb(phase: .listening, size: 58)
                Spacer(minLength: 0)
                Text("Talk to Beep").font(.body(19)).foregroundStyle(Mint.ink)
                Text("Tap to talk").font(.body(12)).foregroundStyle(Mint.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
    }
}

/// A live voice session while it runs: its state with a small orb in the Dynamic Island and on
/// the Lock Screen. A tap goes back to the session; End leaves live voice.
struct LiveVoiceActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: LiveVoiceAttributes.self) { context in
            HStack(spacing: 12) {
                SmallOrb(phase: context.state.phase, size: 40)
                Summary(state: context.state)
                if context.state.phase != .ended { EndButton() }
            }
            .padding(.horizontal, 16).padding(.vertical, 14)
            .activityBackgroundTint(Mint.activity.opacity(0.95))
            .activitySystemActionForegroundColor(.white)
            .widgetURL(URL(string: "beep://live"))
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    SmallOrb(phase: context.state.phase, size: 44)
                }
                DynamicIslandExpandedRegion(.center) {
                    Summary(state: context.state)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    HStack {
                        Image(systemName: "waveform").foregroundStyle(context.state.phase.tint)
                        Spacer()
                        if context.state.phase != .ended { EndButton() }
                    }
                }
            } compactLeading: {
                SmallOrb(phase: context.state.phase, size: 22)
            } compactTrailing: {
                Text(context.state.phase.word).font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(context.state.phase.tint)
            } minimal: {
                SmallOrb(phase: context.state.phase, size: 22)
            }
            .widgetURL(URL(string: "beep://live"))
        }
    }
}

private struct Summary: View {
    var state: LiveVoiceAttributes.ContentState

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Beep · \(state.phase.word)").font(.system(size: 15, weight: .semibold)).foregroundStyle(.white)
            if !state.line.isEmpty {
                Text(state.line).font(.system(size: 13)).foregroundStyle(Color(white: 0.8)).lineLimit(2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct EndButton: View {
    var body: some View {
        Button(intent: EndLiveIntent()) {
            Text("End").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.ink)
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.accent))
        }
        .buttonStyle(.plain)
    }
}

/// The app's orb, small: mint listening, darker thinking, coral speaking.
private struct SmallOrb: View {
    var phase: LiveVoiceAttributes.Phase
    var size: CGFloat

    var body: some View {
        Circle().fill(phase == .speaking ? Mint.accent : phase == .listening ? Mint.ok : Mint.muted)
            .frame(width: size, height: size)
    }
}

extension LiveVoiceAttributes.Phase {
    var word: String {
        switch self {
        case .listening: "Listening"
        case .thinking: "Thinking"
        case .speaking: "Speaking"
        case .ended: "Ended"
        }
    }

    var tint: Color {
        switch self {
        case .listening: Mint.ok
        case .thinking, .ended: Mint.muted
        case .speaking: Mint.brand
        }
    }
}

#Preview(as: .systemMedium) {
    TalkToBeepWidget()
} timeline: {
    Still.Entry(date: .now)
}
