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

/// The app's Mint Quiet colors, as `Mint` in the app.
private enum Colors {
    static let page = Color(red: 0xF4 / 255, green: 0xF8 / 255, blue: 0xF5 / 255)
    static let surface = Color.white
    static let ink = Color(red: 0x10 / 255, green: 0x20 / 255, blue: 0x1A / 255)
    static let muted = Color(red: 0x5C / 255, green: 0x6E / 255, blue: 0x64 / 255)
    static let dim = Color(red: 0x94 / 255, green: 0xA4 / 255, blue: 0x9B / 255)
    static let ledge = Color(red: 0x1F / 255, green: 0x3A / 255, blue: 0x2E / 255)
    static let accent = Color(red: 0xF0 / 255, green: 0x7A / 255, blue: 0x6A / 255)
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
                    Colors.page
                }
            }
    }

    @ViewBuilder private var content: some View {
        switch family {
        case .accessoryCircular:
            Text("b").font(.custom("BricolageGrotesque-96ptExtraBold", size: 26))
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
                    .font(.custom("Rubik-Light", size: 15))
                    .foregroundStyle(Colors.dim)
                    .padding(.horizontal, 14)
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .chunky(Capsule(), fill: Colors.surface)
            }
        default:
            VStack(alignment: .leading, spacing: 0) {
                tile(44)
                Spacer(minLength: 0)
                Text("Beep").font(.custom("BricolageGrotesque-96ptExtraBold", size: 20)).foregroundStyle(Colors.ink)
                Text("Ask anything").font(.custom("Rubik-Light", size: 13)).foregroundStyle(Colors.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
    }

    /// The coral "b" tile, the app's mark.
    private func tile(_ size: CGFloat) -> some View {
        Text("b")
            .font(.custom("BricolageGrotesque-96ptExtraBold", size: size * 0.5))
            .foregroundStyle(Colors.ink)
            .frame(width: size, height: size)
            .chunky(RoundedRectangle(cornerRadius: size * 0.3), fill: Colors.accent)
    }
}

private extension View {
    /// The app's outline with a hard shadow, smaller to suit the widget.
    func chunky<S: Shape>(_ shape: S, fill: Color) -> some View {
        background(shape.fill(fill))
            .overlay(shape.stroke(Colors.ledge, lineWidth: 1.5))
            .background(shape.fill(Colors.ledge).offset(x: 2, y: 2))
    }
}

#Preview(as: .systemMedium) {
    AskBeepWidget()
} timeline: {
    Still.Entry(date: .now)
}
