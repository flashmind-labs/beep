import CoreLocation
import MapKit
import SwiftUI

/// A place the user saved by name, such as Home or Gym, that routines can start on
/// arriving at or leaving. The same fields as the server's `Place`.
struct SavedPlace: Identifiable, Hashable {
    var id: String
    var name: String
    var latitude: Double
    var longitude: Double
    /// Within this many metres counts as being there.
    var radius: Int
    var address: String?
    /// Whether the phone's last location was there.
    var here: Bool
    /// RFC 3339, when the phone was last there.
    var lastHereAt: String?
    /// How many routines start on arriving at or leaving it.
    var routines: Int

    /// "Within 100 m · 1 routine · last here Thu".
    var detail: String {
        let count = routines == 0 ? "no routines" : routines == 1 ? "1 routine" : "\(routines) routines"
        var parts = ["Within \(radius) m", count]
        if here {
            parts.append("here now")
        } else if let last = lastHereAt.flatMap({ try? Date($0, strategy: .iso8601) }) {
            parts.append("last here " + shortDay(last))
        }
        return parts.joined(separator: " · ")
    }
}

/// The saved places calls. The server side (requests 74 to 76) is not in the
/// bridge yet, so the app runs on `PendingPlaces` until it is.
protocol PlacesStore {
    func list() async throws -> [SavedPlace]
    func delete(id: String) async throws
    /// `radius` in metres, 25 to 1000.
    func update(id: String, name: String?, radius: Int?) async throws -> SavedPlace
}

/// No places until the bridge has `list_places`, `delete_place` and `update_place`.
struct PendingPlaces: PlacesStore {
    func list() async throws -> [SavedPlace] { [] }
    func delete(id: String) async throws { throw unavailable }
    func update(id: String, name: String?, radius: Int?) async throws -> SavedPlace { throw unavailable }
    private var unavailable: BeepError { .BadRequest(reason: "This server can't change places yet.") }
}

#if DEBUG
/// Made-up places for trying the screens on the simulator with `-places sample`.
final class SamplePlaces: PlacesStore {
    private var places = [
        SavedPlace(id: "home", name: "Home", latitude: 41.403, longitude: 2.156, radius: 150,
                   address: "12 Example Street", here: true, routines: 1),
        SavedPlace(id: "gym", name: "Gym", latitude: 41.399, longitude: 2.197, radius: 100,
                   address: "40 Sample Road", here: false,
                   lastHereAt: Date.now.addingTimeInterval(-2 * 86400).ISO8601Format(), routines: 1),
        SavedPlace(id: "office", name: "Office", latitude: 41.387, longitude: 2.170, radius: 100,
                   address: "1 Placeholder Square", here: false, routines: 0),
    ]

    func list() async throws -> [SavedPlace] { places }
    func delete(id: String) async throws { places.removeAll { $0.id == id } }
    func update(id: String, name: String?, radius: Int?) async throws -> SavedPlace {
        guard let index = places.firstIndex(where: { $0.id == id }) else { throw BeepError.NotFound }
        if let name { places[index].name = name }
        if let radius { places[index].radius = radius }
        return places[index]
    }
}
#endif

/// A routine's place trigger, written into its schedule as `arrive:<id>`,
/// `leave:<id>`, either with `:daily` for once a day at most.
struct PlaceTrigger: Equatable {
    enum Kind: String { case arrive, leave }

    var kind: Kind
    var placeId: String
    var daily: Bool

    var schedule: String { "\(kind.rawValue):\(placeId)" + (daily ? ":daily" : "") }

    init(kind: Kind, placeId: String, daily: Bool) {
        self.kind = kind
        self.placeId = placeId
        self.daily = daily
    }

    init?(schedule: String) {
        let parts = schedule.split(separator: ":").map(String.init)
        guard parts.count >= 2, let kind = Kind(rawValue: parts[0]) else { return nil }
        self.init(kind: kind, placeId: parts[1], daily: parts.count > 2 && parts[2] == "daily")
    }
}

/// "Thu" within the last week, else "28 Sep".
func shortDay(_ date: Date) -> String {
    let calendar = Calendar.current
    if calendar.isDateInToday(date) { return "today" }
    if calendar.isDateInYesterday(date) { return "yesterday" }
    if let days = calendar.dateComponents([.day], from: date, to: .now).day, days < 7 {
        return date.formatted(.dateTime.weekday(.abbreviated))
    }
    return date.formatted(.dateTime.day().month(.abbreviated))
}

// MARK: Places screen

struct PlacesView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    let always: () -> Void
    let newRoutine: (SavedPlace) -> Void
    @State private var crons: [Cron] = []
    @State private var deleting: SavedPlace?
    @State private var shown: SavedPlace?
    @State private var error = ""

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Places", back: back)
            ScrollView {
                VStack(spacing: 10) {
                    if !error.isEmpty {
                        Text(error).font(.body(12)).foregroundStyle(Mint.error)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    if model.places.isEmpty {
                        Text("No places yet.\nBeep can remind you when you arrive somewhere, or leave.")
                            .font(.body(14)).foregroundStyle(Mint.muted).lineSpacing(5)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, 20).padding(.top, 48).padding(.bottom, 16)
                    } else {
                        if !model.location.always { banner }
                        ForEach(model.places) { place in row(place) }
                    }
                    hint
                }
                .padding(.horizontal, 18).padding(.top, 4).padding(.bottom, 24)
            }
            .refreshable { await load() }
        }
        .background(Mint.page)
        .task { await load() }
        .alert("Delete \(deleting?.name ?? "this place")?", isPresented: Binding(
            get: { deleting != nil }, set: { if !$0 { deleting = nil } })
        ) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { if let place = deleting { delete(place) } }
        } message: {
            Text(deleteMessage)
        }
        .sheet(item: $shown) { place in
            PlaceSheet(place: place) {
                shown = nil
                newRoutine(place)
            }
        }
    }

    private func load() async {
        await model.refreshPlaces()
        crons = (try? await model.crons()) ?? []
    }

    private var deleteMessage: String {
        guard let place = deleting else { return "" }
        let names = crons.filter { PlaceTrigger(schedule: $0.schedule)?.placeId == place.id }.map(\.label)
        guard !names.isEmpty else { return "Beep forgets this place." }
        let list = names.count == 1 ? names[0] : names.dropLast().joined(separator: ", ") + " and " + names.last!
        return "\(list) \(names.count == 1 ? "runs" : "run") when you arrive here, and \(names.count == 1 ? "stops" : "stop"). Beep forgets this place."
    }

    private var banner: some View {
        Button(action: always) {
            HStack(spacing: 10) {
                Image(systemName: "mappin.and.ellipse").font(.system(size: 16, weight: .semibold))
                Text("Beep can't notice arrivals yet. Allow location all the time.")
                    .font(.body(13)).multilineTextAlignment(.leading)
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.muted)
            }
            .foregroundStyle(Mint.ink)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 14).fill(Mint.tints[2]))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(Mint.ledge, lineWidth: 1.5))
        }
        .buttonStyle(.plain)
    }

    private func row(_ place: SavedPlace) -> some View {
        HStack(spacing: 12) {
            Button { shown = place } label: {
                HStack(spacing: 12) {
                    TintIcon(system: "mappin", tint: placeTint(place))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(place.name).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                        if let address = place.address {
                            Text(address).font(.body(12.5)).foregroundStyle(Mint.muted).lineLimit(1)
                        }
                        Text(place.detail).font(.body(11.5)).foregroundStyle(Mint.dim).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Button { deleting = place } label: {
                Image(systemName: "xmark").font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Mint.muted).frame(width: 36, height: 36)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Delete \(place.name)")
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .chunky()
    }

    private var hint: some View {
        (Text("Tell Beep where you are to add one, like ")
            + Text("\"I'm at the gym\"").fontWeight(.semibold)
            + Text(" or ")
            + Text("\"this is home\"").fontWeight(.semibold)
            + Text("."))
            .font(.body(13)).foregroundStyle(Mint.ink).lineSpacing(4)
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 18).fill(Mint.tints[1]))
            .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.ledge, lineWidth: 1.5))
    }

    private func delete(_ place: SavedPlace) {
        Task {
            do {
                try await model.deletePlace(place.id)
                error = ""
            } catch {
                self.error = describe(error)
            }
        }
    }
}

func placeTint(_ place: SavedPlace) -> Color {
    Mint.tints[abs(place.id.unicodeScalars.reduce(0) { $0 + Int($1.value) }) % Mint.tints.count]
}

/// One place: a map with its radius, the radius chips, a routine for arriving, and rename.
struct PlaceSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State var place: SavedPlace
    let newRoutine: () -> Void
    @State private var renaming = false
    @State private var newName = ""
    @State private var error = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            PlaceMap(place: place).frame(height: 180)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    Text(place.name).font(.display(20)).foregroundStyle(Mint.ink)
                    Spacer()
                    Text("Saved").font(.body(11, .medium)).foregroundStyle(Mint.ink)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(Capsule().fill(Mint.tints[1]))
                }
                if let address = place.address {
                    Text(address).font(.body(13)).foregroundStyle(Mint.muted)
                }
                Text("Counts as here within").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                    .padding(.top, 12).padding(.bottom, 4)
                HStack(spacing: 8) {
                    ForEach([50, 100, 250], id: \.self) { radius in
                        FilterChip(label: "\(radius) m", selected: place.radius == radius) { update(radius: radius) }
                    }
                }
                if !error.isEmpty {
                    Text(error).font(.body(12)).foregroundStyle(Mint.error).padding(.top, 6)
                }
                HStack(spacing: 10) {
                    Button(action: newRoutine) {
                        Label("When I get here", systemImage: "clock").font(.body(14, .semibold))
                            .foregroundStyle(Mint.ink)
                            .padding(.horizontal, 14).frame(minHeight: 42)
                            .chunky(Mint.accent, radius: 12)
                    }
                    .buttonStyle(.plain)
                    Button {
                        newName = place.name
                        renaming = true
                    } label: {
                        Label("Rename", systemImage: "pencil").font(.body(14, .semibold))
                            .foregroundStyle(Mint.ink)
                            .padding(.horizontal, 14).frame(minHeight: 42)
                            .chunky(radius: 12)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.top, 14)
            }
            .padding(18)
            Spacer(minLength: 0)
        }
        .background(Mint.page)
        .presentationDetents([.height(440)])
        .presentationCornerRadius(28)
        .alert("Rename \(place.name)", isPresented: $renaming) {
            TextField("Name", text: $newName)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                let name = newName.trimmingCharacters(in: .whitespacesAndNewlines)
                if !name.isEmpty, name != place.name { update(name: name) }
            }
        }
    }

    private func update(name: String? = nil, radius: Int? = nil) {
        Task {
            do {
                try await model.updatePlace(place.id, name: name, radius: radius)
                if let fresh = model.places.first(where: { $0.id == place.id }) { place = fresh }
                error = ""
            } catch {
                self.error = describe(error)
            }
        }
    }
}

private struct PlaceMap: View {
    let place: SavedPlace

    var body: some View {
        let center = CLLocationCoordinate2D(latitude: place.latitude, longitude: place.longitude)
        Map(initialPosition: .region(MKCoordinateRegion(
            center: center, latitudinalMeters: Double(place.radius) * 5, longitudinalMeters: Double(place.radius) * 5))
        ) {
            MapCircle(center: center, radius: Double(place.radius))
                .foregroundStyle(Mint.brand.opacity(0.18))
                .stroke(Mint.brand, lineWidth: 2)
            Marker(place.name, systemImage: "mappin", coordinate: center).tint(Mint.accent)
        }
        .id(place.radius)
        .allowsHitTesting(false)
    }
}

// MARK: Location always

/// Asks for location Always, which geofencing needs to work with the app closed.
struct LocationAlwaysView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let close: () -> Void
    let places: () -> Void
    @AppStorage("asked_location_always") private var asked = false

    var body: some View {
        SwiftUI.Group {
            if model.location.always { allowed } else { ask }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Mint.page)
        .onChange(of: model.location.always) { if model.location.always { Task { await model.refreshPlaces() } } }
    }

    /// iOS shows its prompt once. Asked before, only Settings can turn on Always.
    private var settingsStep: Bool { asked && !model.location.always && model.location.status != .notDetermined }

    private var ask: some View {
        VStack(alignment: .leading, spacing: 16) {
            TintIcon(system: "mappin.and.ellipse", tint: Mint.tints[0], size: 56)
            Text("Notice when you arrive?").font(.display(30)).foregroundStyle(Mint.ink)
            (Text("For routines like \"when I get to the gym\", the phone watches your saved places and tells Beep when you arrive or leave. That needs location set to ")
                + Text("Always").fontWeight(.semibold).foregroundColor(Mint.ink) + Text("."))
                .font(.body(15)).foregroundStyle(Mint.muted).lineSpacing(5)
            VStack(alignment: .leading, spacing: 10) {
                point("mappin", "Works with the app closed, only for places you saved.")
                point("bell", "You only get a notification when a routine has something to say.")
                point("checkmark.shield", "Uses the phone's geofencing, so the battery cost is small.")
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .card()
            if settingsStep {
                VStack(alignment: .leading, spacing: 8) {
                    Text("iOS asks for this in Settings").font(.body(14, .semibold))
                    Text("1. Tap ") + Text("Open settings").fontWeight(.semibold)
                    Text("2. Location")
                    Text("3. Choose ") + Text("Always").fontWeight(.semibold)
                }
                .font(.body(13)).foregroundStyle(Mint.ink)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .card()
            }
            Spacer(minLength: 0)
            Button(settingsStep ? "Open settings" : "Always") {
                if settingsStep {
                    if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                } else {
                    asked = true
                    model.location.requestAlways()
                }
            }
            .buttonStyle(PrimaryButton())
            Button("Not now", action: close).buttonStyle(TextAction(color: Mint.muted)).frame(maxWidth: .infinity)
        }
        .padding(.horizontal, 20).padding(.top, 28).padding(.bottom, 16)
    }

    private func point(_ icon: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: icon).font(.system(size: 15, weight: .semibold)).foregroundStyle(Mint.ink).frame(width: 20)
            Text(text).font(.body(13)).foregroundStyle(Mint.ink).lineSpacing(3)
        }
    }

    private var allowed: some View {
        VStack(spacing: 14) {
            Spacer()
            Image(systemName: "checkmark").font(.system(size: 26, weight: .bold)).foregroundStyle(Mint.ok)
                .frame(width: 64, height: 64)
                .background(RoundedRectangle(cornerRadius: 18).fill(Mint.tints[1]))
                .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.ledge, lineWidth: 2))
            Text("Beep will notice when you arrive.").font(.display(30)).foregroundStyle(Mint.ink)
                .multilineTextAlignment(.center)
            Text("Your places work with the app closed. Turn it off any time in Settings, Beep, Location.")
                .font(.body(14)).foregroundStyle(Mint.muted).multilineTextAlignment(.center)
                .frame(maxWidth: 270)
            Button("See your places", action: places).buttonStyle(PrimaryButton()).padding(.top, 12)
            Spacer()
        }
        .padding(24)
    }
}
