import SwiftUI
import WidgetKit

@main
struct BeepWidgets: WidgetBundle {
    var body: some Widget {
        AskBeepWidget()
        TalkToBeepWidget()
        LiveVoiceActivity()
        if #available(iOS 18.0, *) { TalkToBeepControl() }
    }
}

/// The Android "Ask Beep..." widget: any tap opens a new chat with the keyboard up.
struct AskBeepWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "AskBeep", provider: Still()) { _ in AskBeepView() }
            .configurationDisplayName("Ask Beep")
            .description("Start a new chat with Beep.")
            .supportedFamilies([.systemSmall, .systemMedium, .accessoryCircular, .accessoryRectangular])
    }
}

/// The widget shows nothing that changes, so it has one entry for good.
struct Still: TimelineProvider {
    struct Entry: TimelineEntry { let date: Date }

    func placeholder(in context: Context) -> Entry { Entry(date: .now) }
    func getSnapshot(in context: Context, completion: @escaping (Entry) -> Void) { completion(Entry(date: .now)) }
    func getTimeline(in context: Context, completion: @escaping (Timeline<Entry>) -> Void) {
        completion(Timeline(entries: [Entry(date: .now)], policy: .never))
    }
}

struct AskBeepView: View {
    @Environment(\.widgetFamily) private var family

    var body: some View {
        content
            .widgetURL(URL(string: "beep://new"))
            .containerBackground(for: .widget) {
                if family == .accessoryCircular || family == .accessoryRectangular {
                    AccessoryWidgetBackground()
                } else {
                    Mint.page
                }
            }
    }

    @ViewBuilder private var content: some View {
        switch family {
        case .accessoryCircular:
            Image(systemName: "bubble.left").font(.system(size: 26, weight: .medium))
                .widgetAccentable()
        case .accessoryRectangular:
            VStack(alignment: .leading, spacing: 0) {
                Text("Beep").font(.headline).widgetAccentable()
                Text("Ask anything").font(.subheadline).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        case .systemMedium:
            HStack(spacing: 12) {
                tile(40)
                Text("Ask Beep...")
                    .font(.body(14))
                    .foregroundStyle(Mint.dim)
                    .padding(.horizontal, 14)
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .card(Mint.surface, radius: Mint.smallRadius)
            }
        default:
            VStack(alignment: .leading, spacing: 0) {
                tile(44)
                Spacer(minLength: 0)
                Text("Beep").font(.body(20)).foregroundStyle(Mint.ink)
                Text("Ask anything").font(.body(13)).foregroundStyle(Mint.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
    }

    /// The app mark.
    private func tile(_ size: CGFloat) -> some View {
        Image(systemName: "bubble.left")
            .font(.system(size: size * 0.5, weight: .medium))
            .foregroundStyle(Mint.ink)
            .frame(width: size, height: size)
            .card(Mint.user, radius: Mint.smallRadius)
    }
}

#Preview(as: .systemMedium) {
    AskBeepWidget()
} timeline: {
    Still.Entry(date: .now)
}
