import CoreLocation
import Contacts
import MapKit
import SwiftUI

/// What a finished tool call shows in the chat instead of a bare chip, as on Android.
enum ToolCard: Equatable {
    case place(PlaceSearch)
    case here(LatLon)
    case contacts([Contact])
    case openedApp(OpenedApp)
    case link(URL)
    case alarm(AlarmRequest)
    case replies(Replies)
    case images([ReplyPicture])
    case products([Product])
    /// Recommendations from show_cards: places, films and shows, events or any other kind.
    case recs([RecCard])
    case file(SentFile)
    /// A place Beep saved by name, which routines can start on arriving at or leaving.
    case savedPlace(SavedPlace)
    /// Transit routes a get_route call planned, fetched from the server by the call's id.
    case route(callId: String)

    /// A place to show on a map. `near` centers a search, such as "italian restaurant", around
    /// the user. Directions start at `origin` when given, else at the phone.
    struct PlaceSearch: Equatable {
        var name: String
        var query: String
        var directions: Bool
        var near: LatLon?
        var origin: String?
        var travelMode: String?
    }

    struct Contact: Equatable {
        var name: String
        var phone: String
        var email: String
    }

    struct OpenedApp: Equatable {
        var app: String
        var uri: String
        var fallback: String
    }

    /// Replies the model offered. A choice reads as a question to answer.
    struct Replies: Equatable {
        var replies: [String]
        var choice: Bool
    }

    /// Reads the card a tool's output describes, or nil when it is just text for the model.
    init?(name: String, callId: String, success: Bool, output: String) {
        guard success else { return nil }
        // Its output is text for the model; the card itself waits on the server.
        if name == "get_route" {
            guard !callId.isEmpty else { return nil }
            self = .route(callId: callId)
            return
        }
        guard let data = output.data(using: .utf8),
              let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        switch name {
        case "suggest_replies":
            let replies = (json["replies"] as? [Any] ?? []).compactMap { $0 as? String }.filter { !$0.isBlank }
            guard !replies.isEmpty else { return nil }
            self = .replies(Replies(replies: replies, choice: json["kind"] as? String == "choice"))
        case "open_maps":
            guard let place = Self.place(json) else { return nil }
            self = .place(place)
        case "get_location":
            guard let lat = json["latitude"] as? Double, let lon = json["longitude"] as? Double else { return nil }
            self = .here(LatLon(latitude: lat, longitude: lon))
        case "query_contacts":
            let people = (json["contacts"] as? [[String: Any]] ?? []).map { c in
                Contact(name: c["name"] as? String ?? "", phone: Self.first(c["phones"], "number"),
                        email: Self.first(c["emails"], "address"))
            }
            guard !people.isEmpty else { return nil }
            self = .contacts(people)
        case "open_app":
            guard let app = field(json, "app") else { return nil }
            self = .openedApp(OpenedApp(app: app, uri: json["uri"] as? String ?? "", fallback: json["fallback_url"] as? String ?? ""))
        case "open_browser":
            guard let url = field(json, "url").flatMap(URL.init(string:)), url.scheme != nil else { return nil }
            self = .link(url)
        case "set_alarm":
            guard let alarm = AlarmRequest(json) else { return nil }
            self = .alarm(alarm)
        case "send_image":
            let images: [ReplyPicture] = (json["images"] as? [[String: Any]] ?? []).compactMap { image in
                let url = field(image, "url") ?? (image["file"] as? [String: Any]).flatMap { file in
                    field(file, "id").map { SentFileCache.imageURL(id: $0, name: field(file, "name") ?? "image") }
                }
                return url.map { ReplyPicture(url: $0, alt: field(image, "caption") ?? "") }
            }
            guard !images.isEmpty else { return nil }
            self = .images(images)
        case "show_products":
            let products = Product.shown(toolOutput: output)
            guard !products.isEmpty else { return nil }
            self = .products(products)
        case "show_cards":
            let cards = RecCard.shown(json)
            guard !cards.isEmpty else { return nil }
            self = .recs(cards)
        case "place_save":
            guard let place = SavedPlace(toolOutput: output) else { return nil }
            self = .savedPlace(place)
        case "send_file":
            guard let file = json["file"] as? [String: Any], let id = field(file, "id") else { return nil }
            let name = field(file, "name") ?? "file"
            let mime = field(file, "mime") ?? "application/octet-stream"
            // An image shows as a picture, as one from send_image does.
            if mime.hasPrefix("image/") {
                self = .images([ReplyPicture(url: SentFileCache.imageURL(id: id, name: name), alt: "")])
            } else {
                self = .file(SentFile(id: id, name: name, mime: mime, size: (file["size"] as? Int64) ?? Int64(file["size"] as? Int ?? 0)))
            }
        default:
            return nil
        }
    }

    /// Handles both the `place` object and the older `uri` plus nested card shape.
    private static func place(_ json: [String: Any]) -> PlaceSearch? {
        if let p = json["place"] as? [String: Any] {
            let directions = p["mode"] as? String == "directions"
            guard let query = (directions ? field(p, "destination") : nil) ?? field(p, "query") ?? field(p, "name") else { return nil }
            let near = (p["near"] as? [String: Any]).flatMap { n -> LatLon? in
                guard let lat = n["latitude"] as? Double, let lon = n["longitude"] as? Double else { return nil }
                return LatLon(latitude: lat, longitude: lon)
            }
            return PlaceSearch(name: field(p, "name") ?? query, query: query, directions: directions, near: near,
                               origin: field(p, "origin"), travelMode: field(p, "travel_mode"))
        }
        let uri = json["uri"] as? String ?? ""
        guard let start = uri.range(of: "q=") else { return nil }
        let raw = uri[start.upperBound...].prefix { $0 != "&" }.replacingOccurrences(of: "+", with: " ")
        let query = raw.removingPercentEncoding ?? raw
        guard !query.isEmpty else { return nil }
        let name = ((json["card"] as? [String: Any])?["card_data"] as? [String: Any]).flatMap { field($0, "name") } ?? query
        return PlaceSearch(name: name, query: query, directions: uri.hasPrefix("google.navigation:"))
    }

    private static func first(_ list: Any?, _ key: String) -> String {
        guard let first = (list as? [Any])?.first else { return "" }
        if let object = first as? [String: Any] {
            return (object[key] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? object["value"] as? String ?? ""
        }
        return first as? String ?? ""
    }
}

/// A trimmed string field, nil when it is missing, blank or "null".
private func field(_ json: [String: Any], _ key: String) -> String? {
    guard let value = (json[key] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
          !value.isEmpty, value != "null" else { return nil }
    return value
}

private extension String {
    var isBlank: Bool { trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
}

/// A point on the map. `country` is the ISO 3166 code in capitals, or empty.
struct LatLon: Hashable {
    var latitude: Double
    var longitude: Double
    var address = ""
    var country = ""

    var coordinate: CLLocationCoordinate2D { CLLocationCoordinate2D(latitude: latitude, longitude: longitude) }
}

/// Finds places with Apple's search, so no map key is needed.
@MainActor
enum PlaceLookup {
    private static var found: [String: LatLon] = [:]
    private static var addresses: [String: String] = [:]

    /// Within about 20 km of `near` when given, so a search finds places close by.
    static func lookup(_ query: String, near: LatLon?) async -> LatLon? {
        let key = near.map { String(format: "%@@%.2f,%.2f", query, $0.latitude, $0.longitude) } ?? query
        if let hit = found[key] { return hit }
        let request = MKLocalSearch.Request()
        request.naturalLanguageQuery = query
        if let near {
            request.region = MKCoordinateRegion(center: near.coordinate, latitudinalMeters: 40_000, longitudinalMeters: 40_000)
        }
        guard let item = try? await MKLocalSearch(request: request).start().mapItems.first else { return nil }
        let placemark = item.placemark
        let point = LatLon(latitude: placemark.coordinate.latitude, longitude: placemark.coordinate.longitude,
                           address: line(placemark), country: placemark.isoCountryCode?.uppercased() ?? "")
        found[key] = point
        return point
    }

    static func address(at point: LatLon) async -> String {
        let key = String(format: "%.5f,%.5f", point.latitude, point.longitude)
        if let hit = addresses[key] { return hit }
        let location = CLLocation(latitude: point.latitude, longitude: point.longitude)
        guard let placemark = try? await CLGeocoder().reverseGeocodeLocation(location).first else { return "" }
        let text = line(placemark)
        addresses[key] = text
        return text
    }

    /// The postal address on one line.
    private static func line(_ placemark: CLPlacemark) -> String {
        guard let postal = placemark.postalAddress else { return placemark.name ?? "" }
        return CNPostalAddressFormatter.string(from: postal, style: .mailingAddress)
            .split(separator: "\n").joined(separator: ", ")
    }
}

/// The map part of a place or location card: a pin on the spot and, given `ring`, a circle that many metres around it.
private struct MapSnippet: View {
    let point: LatLon?
    var height: CGFloat = 150
    var ring: Int?
    let open: () -> Void

    var body: some View {
        ZStack {
            Mint.code
            if let point {
                Map(initialPosition: .region(MKCoordinateRegion(center: point.coordinate,
                                                                latitudinalMeters: Double(ring ?? 250) * 5,
                                                                longitudinalMeters: Double(ring ?? 250) * 5))) {
                    if let ring {
                        MapCircle(center: point.coordinate, radius: Double(ring))
                            .foregroundStyle(Mint.accent.opacity(0.16))
                            .stroke(Mint.accent, lineWidth: 2)
                    }
                    Marker("", systemImage: "mappin", coordinate: point.coordinate).tint(Mint.accent)
                }
                .id(ring)
                .allowsHitTesting(false)
            } else {
                Image(systemName: "map").font(.system(size: 26)).foregroundStyle(Mint.dim)
            }
        }
        .frame(height: height)
        .contentShape(Rectangle())
        .onTapGesture(perform: open)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Open in Maps")
    }
}

/// The frame around a chat card.
private extension View {
    func cardFrame() -> some View {
        background(Mint.surface).clipShape(RoundedRectangle(cornerRadius: 18)).card()
    }
}

/// An action on a card: coral when it is the main one.
struct CardButton: View {
    let label: String
    let icon: String
    var primary = false
    var trailing: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) { CardButtonLabel(label: label, icon: icon, primary: primary, trailing: trailing) }
            .buttonStyle(.plain)
    }
}

private struct CardButtonLabel: View {
    let label: String
    let icon: String
    var primary = false
    var trailing: String?

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: icon).font(.system(size: 13, weight: .semibold))
            Text(label).font(.body(13, .semibold))
            if let trailing { Image(systemName: trailing).font(.system(size: 11, weight: .bold)) }
        }
        .foregroundStyle(Mint.ink)
        .padding(.horizontal, 12).frame(minHeight: 38)
        .chunky(primary ? Mint.accent : Mint.surface, radius: 12)
    }
}

/// Maps links. Apple Maps takes the same queries Google Maps does on Android.
private enum MapsLinks {
    static func search(_ query: String, at point: LatLon?, near: LatLon? = nil) -> URL {
        var link = URLComponents(string: "maps://")!
        var items: [URLQueryItem] = [.init(name: "q", value: query)]
        // Maps runs the search itself around the user rather than jumping to one hit.
        if let near { items.append(.init(name: "sll", value: "\(near.latitude),\(near.longitude)")) }
        else if let point { items.append(.init(name: "ll", value: "\(point.latitude),\(point.longitude)")) }
        link.queryItems = items
        return link.url!
    }

    static func directions(_ card: ToolCard.PlaceSearch) -> URL {
        var link = URLComponents(string: "maps://")!
        let flag = switch card.travelMode { case "walking": "w"; case "transit": "r"; default: "d" }
        var items: [URLQueryItem] = [.init(name: "daddr", value: card.query), .init(name: "dirflg", value: flag)]
        if let origin = card.origin { items.append(.init(name: "saddr", value: origin)) }
        link.queryItems = items
        return link.url!
    }

    /// Uber with pickup at the phone and the drop-off at `point`.
    static func uber(_ name: String, _ point: LatLon) -> URL {
        var link = URLComponents(string: "https://m.uber.com/ul/")!
        link.queryItems = [
            .init(name: "action", value: "setPickup"), .init(name: "pickup", value: "my_location"),
            .init(name: "dropoff[nickname]", value: name),
            .init(name: "dropoff[formatted_address]", value: point.address.isEmpty ? name : point.address),
            .init(name: "dropoff[latitude]", value: "\(point.latitude)"),
            .init(name: "dropoff[longitude]", value: "\(point.longitude)"),
        ]
        return link.url!
    }
}

/// Google Maps, offered next to Apple Maps when it is installed.
enum GoogleMaps {
    @MainActor static var installed: Bool { UIApplication.shared.canOpenURL(URL(string: "comgooglemaps://")!) }

    /// A search for `query`, centred on `point` when there is one.
    static func search(_ query: String, at point: LatLon? = nil) -> URL {
        var link = URLComponents(string: "comgooglemaps://")!
        var items: [URLQueryItem] = [.init(name: "q", value: query)]
        if let point { items.append(.init(name: "center", value: "\(point.latitude),\(point.longitude)")) }
        link.queryItems = items
        return link.url!
    }
}

/// A local ride app and the countries it runs in. iOS only says whether an app is installed for
/// schemes listed in Info.plist, so only apps with a known drop-off link are here.
private struct RideApp {
    let name: String
    let scheme: String
    let countries: Set<String>
    let dropoff: (String, LatLon) -> URL?
}

private let rideApps = [
    RideApp(name: "Grab", scheme: "grab://", countries: ["SG", "MY", "ID", "TH", "VN", "PH", "KH", "MM"]) { name, point in
        // Not publicly documented, so a failed open falls back to the app's home.
        var link = URLComponents(string: "grab://open")!
        link.queryItems = [
            .init(name: "screenType", value: "BOOKING"),
            .init(name: "dropOffLatitude", value: "\(point.latitude)"),
            .init(name: "dropOffLongitude", value: "\(point.longitude)"),
            .init(name: "dropOffAddress", value: point.address.isEmpty ? name : point.address),
            .init(name: "dropOffKeywords", value: name),
        ]
        return link.url
    },
    RideApp(name: "Lyft", scheme: "lyft://", countries: ["US", "CA"]) { _, point in
        URL(string: "lyft://ridetype?id=lyft&destination[latitude]=\(point.latitude)&destination[longitude]=\(point.longitude)")
    },
]

private struct RideChoice: Identifiable {
    let name: String
    let note: String
    let links: [URL]
    var id: String { name }
}

/// The ride options for `point`'s country, or empty when no local app there is installed. Where
/// Grab runs, Uber's own rides are gone, so Grab comes first; elsewhere Uber stays first.
@MainActor
private func rideChoices(_ name: String, _ point: LatLon) -> [RideChoice] {
    let local = rideApps.filter { $0.countries.contains(point.country) }
    guard !local.isEmpty else { return [] }
    let installed = { (scheme: String) in URL(string: scheme).map(UIApplication.shared.canOpenURL) ?? false }
    let uber = RideChoice(name: "Uber", note: "Drop-off set to \(name)", links: [MapsLinks.uber(name, point)])
    if let grab = local.first(where: { $0.name == "Grab" }) {
        if installed(grab.scheme) {
            return [RideChoice(name: "Grab", note: "In the Grab app",
                               links: [grab.dropoff(name, point), URL(string: grab.scheme)].compactMap { $0 }),
                    RideChoice(name: "Uber", note: "Uber's own rides", links: uber.links)]
        }
        // Uber books Grab rides there.
        guard installed("uber://") else { return [] }
        return [RideChoice(name: "Grab", note: "Through Uber: tap Grab there", links: [URL(string: "uber://")!]),
                RideChoice(name: "Uber", note: "Uber's own rides", links: uber.links)]
    }
    let others = local.filter { installed($0.scheme) }.prefix(3).map { app in
        RideChoice(name: app.name, note: "Drop-off set to \(name)",
                   links: [app.dropoff(name, point), URL(string: app.scheme)].compactMap { $0 })
    }
    return others.isEmpty ? [] : [uber] + others
}

/// Opens the first link that works.
@MainActor
private func openFirst(_ links: [URL]) {
    guard let first = links.first else { return }
    UIApplication.shared.open(first) { opened in
        if !opened { Task { @MainActor in openFirst(Array(links.dropFirst())) } }
    }
}

private struct PlaceMapCard: View {
    @Environment(\.openURL) private var openURL
    let card: ToolCard.PlaceSearch
    @State private var found: LatLon?

    var body: some View {
        let point = found ?? card.near
        VStack(alignment: .leading, spacing: 0) {
            MapSnippet(point: point) { openURL(MapsLinks.search(card.query, at: point, near: card.near)) }
            VStack(alignment: .leading, spacing: 3) {
                Text(card.name).font(.display(17)).foregroundStyle(Mint.ink).lineLimit(2)
                if let address = found?.address, !address.isEmpty, address != card.name {
                    Text(address).font(.body(13)).foregroundStyle(Mint.muted).lineLimit(2)
                }
                if let origin = card.origin {
                    Text("From \(origin)").font(.body(13)).foregroundStyle(Mint.muted).lineLimit(1)
                }
                FlowLayout(spacing: 10) {
                    CardButton(label: "Directions", icon: "arrow.triangle.turn.up.right.diamond", primary: true) {
                        openURL(MapsLinks.directions(card))
                    }
                    // A ride goes to one looked-up place from the phone, so not for searches or routes from elsewhere.
                    if let ride = found, card.near == nil, card.origin == nil {
                        let choices = rideChoices(card.name, ride)
                        if choices.isEmpty {
                            CardButton(label: "Uber", icon: "car") { openURL(MapsLinks.uber(card.name, ride)) }
                        } else {
                            Menu {
                                ForEach(choices) { choice in
                                    Button { openFirst(choice.links) } label: {
                                        Text(choice.name)
                                        Text(choice.note)
                                    }
                                }
                            } label: { CardButtonLabel(label: "Ride", icon: "car", trailing: "chevron.down") }
                        }
                    }
                    let apple = MapsLinks.search(card.query, at: point, near: card.near)
                    if GoogleMaps.installed {
                        Menu {
                            Button("Apple Maps") { openURL(apple) }
                            Button("Google Maps") { openURL(GoogleMaps.search(card.query, at: card.near ?? point)) }
                        } label: { CardButtonLabel(label: "Open in Maps", icon: "map", trailing: "chevron.down") }
                    } else {
                        CardButton(label: "Open in Maps", icon: "map") { openURL(apple) }
                    }
                }
                .padding(.top, 9)
            }
            .padding(14)
        }
        .cardFrame()
        .task(id: card) { found = await PlaceLookup.lookup(card.query, near: card.near) }
    }
}

private struct HereCard: View {
    @Environment(\.openURL) private var openURL
    let point: LatLon
    @State private var address = ""

    var body: some View {
        let coordinates = String(format: "%.5f, %.5f", point.latitude, point.longitude)
        VStack(alignment: .leading, spacing: 0) {
            MapSnippet(point: point, height: 130) {
                openURL(MapsLinks.search(String(format: "%.5f,%.5f", point.latitude, point.longitude), at: point))
            }
            HStack(spacing: 10) {
                Image(systemName: "location.fill").font(.system(size: 16)).foregroundStyle(Mint.brand)
                VStack(alignment: .leading, spacing: 1) {
                    Text("You are here").font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                    Text(address.isEmpty ? coordinates : address).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
        }
        .cardFrame()
        .task(id: point) { address = await PlaceLookup.address(at: point) }
    }
}

private struct ContactsCard: View {
    @Environment(\.openURL) private var openURL
    let people: [ToolCard.Contact]
    @State private var all = false

    var body: some View {
        let shown = all ? people : Array(people.prefix(3))
        VStack(spacing: 0) {
            ForEach(Array(shown.enumerated()), id: \.offset) { index, person in
                if index > 0 { Rectangle().fill(Mint.soft).frame(height: 1) }
                HStack(spacing: 12) {
                    Text(initial(person.name)).font(.body(15, .bold)).foregroundStyle(Mint.ink)
                        .frame(width: 36, height: 36).background(Circle().fill(Mint.tints[index % Mint.tints.count]))
                    VStack(alignment: .leading, spacing: 1) {
                        Text(person.name).font(.body(14, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                        let detail = person.phone.isEmpty ? person.email : person.phone
                        if !detail.isEmpty { Text(detail).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1) }
                    }
                    Spacer(minLength: 0)
                    if !person.phone.isEmpty {
                        round("phone", "Call \(person.name)", "tel:", person.phone)
                        round("message", "Message \(person.name)", "sms:", person.phone)
                    } else if !person.email.isEmpty {
                        round("envelope", "Email \(person.name)", "mailto:", person.email)
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
            }
            if people.count > 3, !all {
                Button { withAnimation(.snappy) { all = true } } label: {
                    Text("Show \(people.count - 3) more").font(.body(13, .semibold)).foregroundStyle(Mint.brand)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(14)
                }
                .buttonStyle(.plain)
            }
        }
        .cardFrame()
    }

    private func round(_ icon: String, _ label: String, _ scheme: String, _ value: String) -> some View {
        Button {
            let target = scheme == "mailto:" ? value : value.filter { $0.isNumber || $0 == "+" }
            if let url = URL(string: scheme + target) { openURL(url) }
        } label: {
            Image(systemName: icon).font(.system(size: 15)).foregroundStyle(Mint.ink)
                .frame(width: 36, height: 36).background(Circle().fill(Mint.page))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

/// A row card with an icon, a title, a line under it and one action, for an app opened or an alarm set.
private struct ActionRowCard: View {
    let icon: String
    let title: String
    let detail: String
    let button: String
    let action: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon).font(.system(size: 16)).foregroundStyle(Mint.ink)
                .frame(width: 34, height: 34).background(RoundedRectangle(cornerRadius: 10).fill(Mint.tints[1]))
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                if !detail.isEmpty { Text(detail).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1) }
            }
            Spacer(minLength: 0)
            CardButton(label: button, icon: "arrow.up.forward.app", action: action)
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .cardFrame()
    }
}

private struct OpenedAppCard: View {
    let app: ToolCard.OpenedApp

    var body: some View {
        ActionRowCard(icon: icon, title: "Opened \(app.app.prefix(1).uppercased() + app.app.dropFirst())",
                      detail: app.uri, button: "Open") {
            PhoneApps.open(app: app.app, uri: app.uri, fallback: app.fallback)
        }
    }

    private var icon: String {
        switch app.app.lowercased() {
        case "maps": "map"
        case "phone": "phone"
        case "clock": "alarm"
        case "gmail": "envelope"
        case "calendar": "calendar"
        case "browser", "chrome": "globe"
        default: "arrow.up.forward.app"
        }
    }
}

private struct AlarmCard: View {
    let alarm: AlarmRequest

    var body: some View {
        // Before iOS 26 the phone cannot set it, so the card asks the user to.
        let title = PhoneAlarms.canSet ? "Alarm \(alarm.time)" : "Set an alarm for \(alarm.time)"
        ActionRowCard(icon: "alarm", title: title,
                      detail: [alarm.label, alarm.dayNames].filter { !$0.isEmpty }.joined(separator: " · "),
                      button: PhoneAlarms.canSet ? "Alarms" : "Clock") {
            if let clock = URL(string: "clock-alarm://") { UIApplication.shared.open(clock) }
        }
    }
}

/// A page's title and description, for a link preview.
struct LinkInfo: Equatable {
    var title: String
    var description: String
}

/// Page titles for link previews, read from the first 64 KiB of each page.
@MainActor
enum LinkInfos {
    private static var found: [URL: LinkInfo] = [:]

    static func of(_ url: URL) async -> LinkInfo? {
        if let hit = found[url] { return hit }
        var request = URLRequest(url: url, timeoutInterval: 6)
        request.setValue("Mozilla/5.0 (iPhone) Beep", forHTTPHeaderField: "User-Agent")
        guard let (bytes, response) = try? await URLSession.shared.bytes(for: request),
              let type = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type"),
              type.contains("html") else { return nil }
        var head = Data()
        do {
            for try await byte in bytes {
                head.append(byte)
                if head.count >= 64 * 1024 { break }
            }
        } catch {
            if head.isEmpty { return nil }
        }
        let info = parse(decode(head, contentType: type))
        found[url] = info
        return info
    }

    /// In the charset the header names, else the one the page declares, else UTF-8.
    private static func decode(_ data: Data, contentType: String) -> String {
        let latin = String(decoding: data.prefix(2048), as: UTF8.self)
        let declared = firstMatch(#"charset=["']?([\w.:-]+)"#, in: contentType) ?? firstMatch(#"charset=["']?([\w.:-]+)"#, in: latin)
        if let declared {
            let cf = CFStringConvertIANACharSetNameToEncoding(declared as CFString)
            if cf != kCFStringEncodingInvalidId,
               let text = String(data: data, encoding: String.Encoding(rawValue: CFStringConvertEncodingToNSStringEncoding(cf))) {
                return text
            }
        }
        return String(decoding: data, as: UTF8.self)
    }

    private static func parse(_ head: String) -> LinkInfo {
        var tags: [String: String] = [:]
        let meta = #"<meta[^>]+(?:property|name)=["'](og:title|og:description|description)["'][^>]*content=["']([^"']*)["']"#
        if let regex = try? NSRegularExpression(pattern: meta, options: .caseInsensitive) {
            for match in regex.matches(in: head, range: NSRange(head.startIndex..., in: head)) {
                guard let key = Range(match.range(at: 1), in: head), let value = Range(match.range(at: 2), in: head) else { continue }
                let name = head[key].lowercased()
                if tags[name] == nil { tags[name] = unescape(String(head[value])) }
            }
        }
        let title = tags["og:title"] ?? firstMatch("<title[^>]*>([^<]*)</title>", in: head).map(unescape)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return LinkInfo(title: title, description: tags["og:description"] ?? tags["description"] ?? "")
    }

    private static func firstMatch(_ pattern: String, in text: String) -> String? {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive),
              let match = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
              let range = Range(match.range(at: 1), in: text) else { return nil }
        return String(text[range])
    }

    private static func unescape(_ text: String) -> String {
        var out = text
        if let regex = try? NSRegularExpression(pattern: "&#(x?)([0-9a-f]+);", options: .caseInsensitive) {
            for match in regex.matches(in: text, range: NSRange(text.startIndex..., in: text)).reversed() {
                guard let whole = Range(match.range, in: out), let hex = Range(match.range(at: 1), in: out),
                      let digits = Range(match.range(at: 2), in: out),
                      let code = UInt32(out[digits], radix: out[hex].isEmpty ? 10 : 16),
                      let scalar = Unicode.Scalar(code) else { continue }
                out.replaceSubrange(whole, with: String(Character(scalar)))
            }
        }
        for (entity, char) in [("&amp;", "&"), ("&quot;", "\""), ("&#39;", "'"), ("&lt;", "<"), ("&gt;", ">"), ("&#x27;", "'")] {
            out = out.replacingOccurrences(of: entity, with: char)
        }
        return out
    }
}

/// Links in a reply worth previewing, at most two.
func linksIn(_ text: String) -> [URL] {
    let pattern = #"https?://[^\s<>()\[\]"'`]+[^\s<>()\[\]"'`.,;:!?]"#
    guard let regex = try? NSRegularExpression(pattern: pattern) else { return [] }
    var out: [URL] = []
    for match in regex.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
        guard let range = Range(match.range, in: text), let url = URL(string: String(text[range])), !out.contains(url) else { continue }
        out.append(url)
        if out.count == 2 { break }
    }
    return out
}

/// A small preview of a web page: title, a line of description and the site. Tapping opens it in the app.
struct LinkCard: View {
    let url: URL
    let open: (URL) -> Void
    @State private var info: LinkInfo?

    var body: some View {
        let host = url.host()?.replacingOccurrences(of: "www.", with: "", options: .anchored) ?? ""
        Button { open(url) } label: {
            HStack(spacing: 10) {
                Text(host.prefix(1).uppercased()).font(.body(15, .bold)).foregroundStyle(Mint.ink)
                    .frame(width: 36, height: 36).background(RoundedRectangle(cornerRadius: 9).fill(Mint.code))
                VStack(alignment: .leading, spacing: 1) {
                    Text(info.flatMap { $0.title.isEmpty ? nil : $0.title } ?? host)
                        .font(.body(13, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    if let description = info?.description, !description.isEmpty {
                        Text(description).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2)
                    }
                    Text(host).font(.body(11)).foregroundStyle(Mint.brand).lineLimit(1)
                }
                .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.soft, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .task(id: url) { info = await LinkInfos.of(url) }
    }
}

/// One step of a task_progress checklist.
struct TaskStep: Equatable {
    var label: String
    var status: String

    /// The steps in the reply's latest task_progress call.
    static func latest(in tools: [ToolCall]) -> [TaskStep] {
        guard let call = tools.last(where: { $0.name == "task_progress" && $0.done && $0.success }),
              let data = call.output.data(using: .utf8),
              let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let steps = json["steps"] as? [[String: Any]] else { return [] }
        return steps.compactMap { step in
            guard let label = step["label"] as? String, let status = step["status"] as? String else { return nil }
            return TaskStep(label: label, status: status)
        }
    }
}

struct TaskProgressCard: View {
    let steps: [TaskStep]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var pulse = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(steps.enumerated()), id: \.offset) { _, step in
                HStack(spacing: 10) {
                    mark(step.status).frame(width: 20, height: 20)
                    Text(step.label).font(.body(14, step.status == "in_progress" ? .medium : .regular))
                        .foregroundStyle(step.status == "failed" ? Mint.error
                                         : step.status == "pending" ? Mint.muted : Mint.ink)
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardFrame()
        .onAppear {
            guard !reduceMotion else { return }
            withAnimation(.easeInOut(duration: 0.6).repeatForever(autoreverses: true)) { pulse = true }
        }
    }

    @ViewBuilder private func mark(_ status: String) -> some View {
        switch status {
        case "in_progress":
            Circle().fill(Mint.accent).frame(width: 12, height: 12).scaleEffect(pulse ? 1 : 0.7)
        case "completed":
            Image(systemName: "checkmark").font(.system(size: 9, weight: .heavy)).foregroundStyle(.white)
                .frame(width: 18, height: 18).background(Circle().fill(Mint.ok))
                .accessibilityLabel("Completed")
        case "failed":
            Image(systemName: "xmark").font(.system(size: 9, weight: .heavy)).foregroundStyle(.white)
                .frame(width: 18, height: 18).background(Circle().fill(Mint.error))
                .accessibilityLabel("Failed")
        default:
            Circle().stroke(Mint.soft, lineWidth: 1.5).frame(width: 14, height: 14)
        }
    }
}

/// Replies the model offered, as chips. A choice gets full-width buttons so it reads as a question to answer.
struct SuggestedReplies: View {
    let replies: ToolCard.Replies
    let pick: (String) -> Void

    var body: some View {
        if replies.choice {
            VStack(spacing: 10) {
                ForEach(Array(replies.replies.enumerated()), id: \.offset) { index, reply in
                    Button { pick(reply) } label: {
                        Text(reply).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                            .multilineTextAlignment(.leading)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 16).padding(.vertical, 12)
                            .chunky(Mint.tints[index % Mint.tints.count], radius: 12)
                    }
                    .buttonStyle(.plain)
                    .popIn(delay: 0.05 * Double(index))
                }
            }
            .padding(.trailing, 6).padding(.bottom, 4)
        } else {
            FlowLayout(spacing: 8) {
                ForEach(Array(replies.replies.enumerated()), id: \.offset) { index, reply in
                    Button { pick(reply) } label: {
                        Text(reply).font(.body(12, .medium)).foregroundStyle(Mint.ink).lineLimit(1)
                            .padding(.horizontal, 11).padding(.vertical, 6)
                            .chunky(radius: 12, drop: 2)
                    }
                    .buttonStyle(.plain)
                    .popIn(delay: 0.04 * Double(index))
                }
            }
            .padding(.trailing, 6).padding(.bottom, 4)
        }
    }
}

/// Cards for one message's tool calls. Suggested replies are drawn separately, under the newest reply only.
struct ToolCards: View {
    @Environment(AppModel.self) private var model
    let cards: [ToolCard]
    let browse: (URL) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(cards.enumerated()), id: \.offset) { _, card in
                switch card {
                case .place(let place): PlaceMapCard(card: place)
                case .here(let point): HereCard(point: point)
                case .contacts(let people): ContactsCard(people: people)
                case .openedApp(let app): OpenedAppCard(app: app)
                case .link(let url): LinkCard(url: url, open: browse)
                case .alarm(let alarm): AlarmCard(alarm: alarm)
                case .images(let images):
                    ForEach(images, id: \.self) { image in
                        VStack(alignment: .leading, spacing: 4) {
                            ReplyImage(picture: image)
                            if !image.alt.isEmpty { Text(image.alt).font(.body(13)).foregroundStyle(Mint.muted) }
                        }
                    }
                case .products(let products): ProductRow(products: products)
                case .recs(let recs): RecRow(cards: recs)
                case .file(let file): FileCard(file: file)
                case .savedPlace(let place):
                    // The mock shows a saved place under the reply that names it.
                    PlaceCard(place: place) { model.destination = .placeRoutine(place.id) }.cardFrame()
                case .route(let callId): RouteCardView(callId: callId)
                case .replies: EmptyView()
                }
            }
        }
        .padding(.trailing, 4)
    }
}
