import EventKit
import EventKitUI
import SwiftUI

/// A recommendation from show_cards: a place, a film or show, an event, or any other kind.
/// Every card has the same base; `place`, `media` and `event` add the part for their kind.
struct RecCard: Hashable {
    struct Place: Hashable {
        var address: String?
        var latitude: Double?
        var longitude: Double?
        var distanceMeters: Int?
        var hours: String?
        var openNow: Bool?
        var bookingURL: URL?
    }

    struct Media: Hashable {
        var year: Int?
        var creator: String?
        var watch: String?
        var watchURL: URL?
    }

    struct Event: Hashable {
        var start: Date
        var end: Date?
        /// The event's own offset, so its time shows as the venue lists it.
        var timeZone: TimeZone
        var venue: String?
        var ticketsURL: URL?
    }

    var kind: String
    var title: String
    var url: URL
    var subtitle: String?
    var image: URL?
    var facts: [String]
    var rating: Double?
    var reviews: Int?
    var note: String?
    var place: Place?
    var media: Media?
    var event: Event?

    /// "4.6 ★ (2,310) · Closest, takes bookings", or whichever parts there are.
    var details: String? {
        var parts: [String] = []
        if let rating {
            parts.append(String(format: "%.1f ★", rating) + (reviews.map { " (\($0.formatted()))" } ?? ""))
        }
        if let note { parts.append(note) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// The cards a show_cards result lists. The server checked them; one that still does not read is skipped.
    static func shown(_ json: [String: Any]) -> [RecCard] {
        (json["cards"] as? [[String: Any]] ?? []).compactMap(RecCard.init)
    }

    init?(_ o: [String: Any]) {
        guard let title = text(o, "title"), let url = link(o, "url") else { return nil }
        self.title = title
        self.url = url
        kind = text(o, "kind") ?? "other"
        subtitle = text(o, "subtitle")
        image = text(o, "image").flatMap { $0.hasPrefix("https://") ? URL(string: $0) : nil }
        facts = (o["facts"] as? [Any] ?? []).compactMap { ($0 as? String)?.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        rating = o["rating"] as? Double
        reviews = (o["reviews"] as? Int).flatMap { $0 >= 0 ? $0 : nil }
        note = text(o, "note")
        place = (o["place"] as? [String: Any]).map { p in
            Place(address: text(p, "address"), latitude: p["latitude"] as? Double, longitude: p["longitude"] as? Double,
                  distanceMeters: (p["distance_m"] as? Int).flatMap { $0 >= 0 ? $0 : nil }, hours: text(p, "hours"),
                  openNow: p["open_now"] as? Bool, bookingURL: link(p, "booking_url"))
        }
        media = (o["media"] as? [String: Any]).map { m in
            Media(year: (m["year"] as? Int).flatMap { $0 > 0 ? $0 : nil }, creator: text(m, "creator"),
                  watch: text(m, "watch"), watchURL: link(m, "watch_url"))
        }
        event = (o["event"] as? [String: Any]).flatMap { e in
            guard let start = text(e, "start").flatMap(offsetDate) else { return nil }
            return Event(start: start.date, end: text(e, "end").flatMap(offsetDate)?.date, timeZone: start.zone,
                         venue: text(e, "venue"), ticketsURL: link(e, "tickets_url"))
        }
    }
}

private func text(_ o: [String: Any], _ key: String) -> String? {
    guard let value = (o[key] as? String)?.trimmingCharacters(in: .whitespaces), !value.isEmpty else { return nil }
    return value
}

private func link(_ o: [String: Any], _ key: String) -> URL? {
    text(o, key).flatMap { $0.hasPrefix("https://") || $0.hasPrefix("http://") ? URL(string: $0) : nil }
}

/// An RFC 3339 time with the offset it was written in.
private func offsetDate(_ text: String) -> (date: Date, zone: TimeZone)? {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    let date = formatter.date(from: text) ?? {
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: text)
    }()
    guard let date else { return nil }
    var zone = TimeZone.current
    if text.hasSuffix("Z") {
        zone = TimeZone(secondsFromGMT: 0) ?? zone
    } else if let sign = text.lastIndex(where: { $0 == "+" || $0 == "-" }), text.distance(from: sign, to: text.endIndex) == 6 {
        let digits = text[text.index(after: sign)...].split(separator: ":").compactMap { Int($0) }
        if digits.count == 2 {
            let seconds = (digits[0] * 3600 + digits[1] * 60) * (text[sign] == "-" ? -1 : 1)
            zone = TimeZone(secondsFromGMT: seconds) ?? zone
        }
    }
    return (date, zone)
}

/// "650 m", "1.2 km" or "13 km".
func distanceText(_ meters: Int) -> String {
    if meters < 1000 { return "\((meters + 5) / 10 * 10) m" }
    if meters < 10_000 { return (Double(meters) / 1000).formatted(.number.precision(.fractionLength(1))) + " km" }
    return "\((meters + 500) / 1000) km"
}

/// Cards side by side in a row that scrolls and snaps; a lone card takes the full width.
struct RecRow: View {
    let cards: [RecCard]

    var body: some View {
        if cards.count == 1, let card = cards.first {
            RecCardView(card: card)
        } else {
            ScrollView(.horizontal, showsIndicators: false) {
                // Not lazy: the row takes the tallest card's height, so one off screen is not cut.
                HStack(alignment: .top, spacing: 10) {
                    ForEach(Array(cards.enumerated()), id: \.offset) { _, card in
                        RecCardView(card: card).frame(width: 216)
                    }
                }
                .scrollTargetLayout()
                .padding(.horizontal, 18)
                .padding(.bottom, 6)
            }
            .scrollTargetBehavior(.viewAligned)
            .padding(.horizontal, -18)
        }
    }
}

private struct RecCardView: View {
    @Environment(\.openURL) private var openURL
    let card: RecCard
    @State private var page: URL?
    @State private var addingEvent = false

    private var place: RecCard.Place? { card.kind == "place" ? card.place : nil }
    private var media: RecCard.Media? { card.kind == "media" ? card.media : nil }
    private var event: RecCard.Event? { card.kind == "event" ? card.event : nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                if let place { openURL(mapsLink(place, directions: false)) } else { page = card.url }
            } label: { content }
                .buttonStyle(.plain)
                .accessibilityHint(place != nil ? "Opens in Maps" : "Opens the page")
            let actions = self.actions
            if !actions.isEmpty {
                HStack(spacing: 6) {
                    ForEach(actions, id: \.label) { action in
                        Button(action: action.run) {
                            Text(action.label).font(.body(12.5, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                                .frame(maxWidth: .infinity, minHeight: 32)
                                .background(RoundedRectangle(cornerRadius: 9).fill(Mint.surface))
                                .overlay(RoundedRectangle(cornerRadius: 9).stroke(Mint.ledge, lineWidth: 1.5))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 12).padding(.bottom, 12)
            } else {
                Color.clear.frame(height: 4)
            }
        }
        .background(Mint.surface)
        .clipShape(RoundedRectangle(cornerRadius: 16))
        .chunky(Mint.surface, radius: 16)
        .safariSheet($page)
        .sheet(isPresented: $addingEvent) {
            if let event { EventEditor(title: card.title, event: event, notes: card.url, place: event.venue ?? card.subtitle) }
        }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let media {
                Poster(url: card.image, media: media)
            } else if card.image != nil || event != nil {
                // A card without a picture drops the picture area; an event keeps it for its date.
                RecPicture(url: card.image, event: event)
            }
            VStack(alignment: .leading, spacing: 5) {
                Text(card.title).font(.body(14, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                if let subtitle { Text(subtitle).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1) }
                let facts = card.facts + (place?.distanceMeters.map { [distanceText($0)] } ?? [])
                if !facts.isEmpty {
                    FlowLayout(spacing: 4) {
                        ForEach(facts, id: \.self) { fact in
                            Text(fact).font(.body(11, .medium)).foregroundStyle(Mint.ink)
                                .padding(.horizontal, 7).padding(.vertical, 1)
                                .background(Capsule().fill(Mint.tints[1]))
                        }
                    }
                }
                if let place, let status = place.hours ?? place.openNow.map({ $0 ? "Open now" : "Closed now" }) {
                    Text(status).font(.body(12, .medium)).foregroundStyle(place.openNow == true ? Mint.ok : Mint.muted)
                }
                if let event { Text(eventTime(event)).font(.body(12, .medium)).foregroundStyle(Mint.ink) }
                if let details = card.details { Text(details).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1) }
            }
            .padding(.horizontal, 12).padding(.top, 10).padding(.bottom, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .contentShape(Rectangle())
    }

    private var subtitle: String? {
        if let subtitle = card.subtitle { return subtitle }
        if let media {
            let parts = [media.year.map(String.init), media.creator].compactMap { $0 }
            if !parts.isEmpty { return parts.joined(separator: " · ") }
        }
        return event?.venue ?? place?.address
    }

    private struct Action {
        let label: String
        let run: () -> Void
    }

    private var actions: [Action] {
        var out: [Action] = []
        if let place {
            out.append(Action(label: "Directions") { openURL(mapsLink(place, directions: true)) })
            if let booking = place.bookingURL { out.append(Action(label: "Book") { page = booking }) }
        }
        if let event {
            out.append(Action(label: "Calendar") { addingEvent = true })
            if let tickets = event.ticketsURL { out.append(Action(label: "Tickets") { page = tickets }) }
        }
        if let media, let watch = media.watchURL {
            let name = media.watch.map { $0.hasPrefix("Rent on ") ? String($0.dropFirst(8)) : $0 } ?? "page"
            out.append(Action(label: "Open \(name)") { openURL(watch) })
        }
        return out
    }

    private func mapsLink(_ place: RecCard.Place, directions: Bool) -> URL {
        let query = place.address.map { "\(card.title), \($0)" } ?? card.title
        let at = place.latitude.flatMap { lat in place.longitude.map { "\(lat),\($0)" } }
        var link = URLComponents(string: "maps://")!
        link.queryItems = directions
            ? [.init(name: "daddr", value: at ?? query)]
            : [.init(name: "q", value: query)] + (at.map { [.init(name: "ll", value: $0)] } ?? [])
        return link.url!
    }

    /// "Sat 18 Oct, 21:00" in the event's own time, written the phone's way.
    private func eventTime(_ event: RecCard.Event) -> String {
        var style = Date.FormatStyle.dateTime.weekday(.abbreviated).day().month(.abbreviated)
        style.timeZone = event.timeZone
        var time = Date.FormatStyle(date: .omitted, time: .shortened)
        time.timeZone = event.timeZone
        return event.start.formatted(style) + ", " + event.start.formatted(time)
    }
}

private struct RecPicture: View {
    let url: URL?
    let event: RecCard.Event?

    var body: some View {
        ZStack(alignment: .topLeading) {
            Mint.code
            if let url {
                AsyncImage(url: url) { phase in
                    if let image = phase.image { image.resizable().scaledToFill() } else { Mint.code }
                }
            }
            if let event {
                VStack(spacing: 0) {
                    Text(day(event, "EEE").uppercased()).font(.body(9.5, .semibold)).foregroundStyle(Mint.accent)
                    Text(day(event, "d")).font(.body(17, .bold)).foregroundStyle(Mint.ink)
                }
                .frame(width: 42).padding(.vertical, 4)
                .chunky(Mint.surface, radius: 10, drop: 2)
                .padding(10)
            }
        }
        .frame(height: 108).frame(maxWidth: .infinity).clipped()
    }

    private func day(_ event: RecCard.Event, _ format: String) -> String {
        let formatter = DateFormatter()
        formatter.timeZone = event.timeZone
        formatter.setLocalizedDateFormatFromTemplate(format)
        return formatter.string(from: event.start)
    }
}

private struct Poster: View {
    let url: URL?
    let media: RecCard.Media

    var body: some View {
        HStack(spacing: 0) {
            ZStack {
                Mint.code
                if let url {
                    AsyncImage(url: url) { phase in
                        if let image = phase.image { image.resizable().scaledToFill() } else { Mint.code }
                    }
                }
            }
            .frame(width: 100).clipped()
            if let watch = media.watch {
                VStack(alignment: .leading, spacing: 4) {
                    Spacer()
                    Text("Watch on").font(.body(10.5)).foregroundStyle(Mint.muted)
                    Text(watch).font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Spacer(minLength: 0)
            }
        }
        .frame(height: 150)
        .frame(maxWidth: .infinity)
        .background(Mint.tints[1])
    }
}

/// The system's new event sheet, filled in. iOS 17 shows it without asking for calendar access.
private struct EventEditor: UIViewControllerRepresentable {
    let title: String
    let event: RecCard.Event
    let notes: URL
    let place: String?

    func makeUIViewController(context: Context) -> EKEventEditViewController {
        let store = EKEventStore()
        let new = EKEvent(eventStore: store)
        new.title = title
        new.startDate = event.start
        new.endDate = event.end ?? event.start.addingTimeInterval(2 * 60 * 60)
        new.location = place
        new.url = notes
        let controller = EKEventEditViewController()
        controller.eventStore = store
        controller.event = new
        controller.editViewDelegate = context.coordinator
        return controller
    }

    func updateUIViewController(_ controller: EKEventEditViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, EKEventEditViewDelegate {
        func eventEditViewController(_ controller: EKEventEditViewController, didCompleteWith action: EKEventEditViewAction) {
            controller.dismiss(animated: true)
        }
    }
}

/// Lays its children out in rows, wrapping to the next row when one is full.
struct FlowLayout: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(subviews, width: proposal.width ?? .infinity)
        let width = rows.map(\.width).max() ?? 0
        let height = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(0, rows.count - 1))
        return CGSize(width: width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(subviews, width: bounds.width) {
            var x = bounds.minX
            for index in row.items {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(_ subviews: Subviews, width: CGFloat) -> [Row] {
        var rows: [Row] = []
        var row = Row()
        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            let needed = row.items.isEmpty ? size.width : row.width + spacing + size.width
            if needed > width, !row.items.isEmpty {
                rows.append(row)
                row = Row()
            }
            row.width = row.items.isEmpty ? size.width : row.width + spacing + size.width
            row.height = max(row.height, size.height)
            row.items.append(index)
        }
        if !row.items.isEmpty { rows.append(row) }
        return rows
    }
}
