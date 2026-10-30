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
            .containerBackground(for: .widget) { LiveTones.page }
    }

    @ViewBuilder private var content: some View {
        if family == .systemMedium {
            HStack(spacing: 14) {
                SmallOrb(phase: .listening, size: 52)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Talk to Beep").font(.custom("BricolageGrotesque-96ptExtraBold", size: 20)).foregroundStyle(LiveTones.ink)
                    Text("Live voice, hands free").font(.custom("Rubik-Light", size: 13)).foregroundStyle(LiveTones.muted)
                }
                Spacer(minLength: 0)
                Label("Talk", systemImage: "waveform")
                    .font(.system(size: 14, weight: .semibold)).foregroundStyle(LiveTones.ink)
                    .padding(.leading, 10).padding(.trailing, 14).padding(.vertical, 9)
                    .background(Capsule().fill(LiveTones.coral))
                    .overlay(Capsule().stroke(LiveTones.ledge, lineWidth: 1.5))
                    .background(Capsule().fill(LiveTones.ledge).offset(x: 2, y: 2))
            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                SmallOrb(phase: .listening, size: 58)
                Spacer(minLength: 0)
                Text("Talk to Beep").font(.custom("BricolageGrotesque-96ptExtraBold", size: 19)).foregroundStyle(LiveTones.ink)
                Text("Tap to talk").font(.custom("Rubik-Light", size: 12)).foregroundStyle(LiveTones.muted)
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
                EndButton()
            }
            .padding(.horizontal, 16).padding(.vertical, 14)
            .activityBackgroundTint(Color(red: 16 / 255, green: 24 / 255, blue: 20 / 255).opacity(0.72))
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
                        EndButton()
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
            Text("End").font(.system(size: 13, weight: .semibold)).foregroundStyle(LiveTones.ink)
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(Capsule().fill(LiveTones.coral))
        }
        .buttonStyle(.plain)
    }
}

/// The app's orb, small: mint listening, darker thinking, coral speaking.
private struct SmallOrb: View {
    var phase: LiveVoiceAttributes.Phase
    var size: CGFloat

    var body: some View {
        let tones: [Color] = switch phase {
        case .listening: [Color(red: 0xA6 / 255, green: 0xEB / 255, blue: 0xCF / 255), LiveTones.green,
                          Color(red: 0x0F / 255, green: 0x3B / 255, blue: 0x2C / 255)]
        case .thinking: [Color(red: 0x7F / 255, green: 0xBF / 255, blue: 0xA6 / 255), Color(red: 0x2E / 255, green: 0x6F / 255, blue: 0x59 / 255),
                         Color(red: 0x0D / 255, green: 0x2A / 255, blue: 0x20 / 255)]
        case .speaking: [Color(red: 0xFF / 255, green: 0xD0 / 255, blue: 0xC7 / 255), LiveTones.coral,
                         Color(red: 0x5A / 255, green: 0x23 / 255, blue: 0x1C / 255)]
        }
        Circle()
            .fill(RadialGradient(stops: [
                .init(color: tones[0], location: 0), .init(color: tones[1], location: 0.46), .init(color: tones[2], location: 1),
            ], center: UnitPoint(x: 0.36, y: 0.3), startRadius: 0, endRadius: size * 0.75))
            .frame(width: size, height: size)
    }
}

extension LiveVoiceAttributes.Phase {
    var word: String {
        switch self {
        case .listening: "Listening"
        case .thinking: "Thinking"
        case .speaking: "Speaking"
        }
    }

    var tint: Color {
        switch self {
        case .listening: Color(red: 0xA6 / 255, green: 0xEB / 255, blue: 0xCF / 255)
        case .thinking: Color(red: 0x92 / 255, green: 0xA5 / 255, blue: 0x9A / 255)
        case .speaking: Color(red: 0xFF / 255, green: 0xB5 / 255, blue: 0x9E / 255)
        }
    }
}

/// Mint Quiet, as in the app.
private enum LiveTones {
    static let page = Color(red: 0xF4 / 255, green: 0xF8 / 255, blue: 0xF5 / 255)
    static let ink = Color(red: 0x10 / 255, green: 0x20 / 255, blue: 0x1A / 255)
    static let muted = Color(red: 0x5C / 255, green: 0x6E / 255, blue: 0x64 / 255)
    static let ledge = Color(red: 0x1F / 255, green: 0x3A / 255, blue: 0x2E / 255)
    static let green = Color(red: 0x3B / 255, green: 0x9B / 255, blue: 0x7A / 255)
    static let coral = Color(red: 0xF0 / 255, green: 0x7A / 255, blue: 0x6A / 255)
}

#Preview(as: .systemMedium) {
    TalkToBeepWidget()
} timeline: {
    Still.Entry(date: .now)
}
