import MapKit
import SwiftUI

/// A get_route call's routes: a sketch of the chosen one, its legs, and Maps. Fetched by call id.
struct RouteCardView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let callId: String
    @State private var card: RouteCard?
    @State private var loaded = false
    @State private var pick = 0

    var body: some View {
        let routes = card?.routes ?? []
        let route = routes.indices.contains(pick) ? routes[pick] : routes.first
        VStack(alignment: .leading, spacing: 0) {
            ZStack {
                Mint.page
                if let route {
                    RouteSketch(route: route)
                } else if loaded {
                    Text("Route no longer available").font(.body(13)).foregroundStyle(Mint.muted)
                } else {
                    ProgressView()
                }
            }
            .frame(height: 96)
            .overlay(alignment: .bottom) { Rectangle().fill(Mint.ledge).frame(height: 1.5) }
            if let route {
                VStack(alignment: .leading, spacing: 0) {
                    Text(head(route)).font(.display(17)).foregroundStyle(Mint.ink)
                    if routes.count > 1 {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 6) {
                                ForEach(routes.indices, id: \.self) { i in
                                    FilterChip(label: chip(routes[i]), selected: i == pick) { pick = i }
                                }
                            }
                        }
                        .padding(.top, 8)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(route.legs.indices, id: \.self) { i in
                            let leg = route.legs[i]
                            if leg.mode == "walk" { WalkLeg(leg: leg) } else { RideLeg(leg: leg) }
                        }
                    }
                    .padding(.top, 12)
                    Text(route.live ? "Live times" : "Timetable times, delays not included")
                        .font(.body(11)).foregroundStyle(Mint.muted).padding(.top, 10)
                    HStack(spacing: 10) {
                        Button { openMaps(route, directions: true) } label: {
                            Label("Directions", systemImage: "arrow.triangle.turn.up.right.diamond")
                                .font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                                .padding(.horizontal, 14).frame(minHeight: 42)
                                .chunky(Mint.accent, radius: 12)
                        }
                        .buttonStyle(.plain)
                        Button { openMaps(route, directions: false) } label: {
                            Label("Open in Maps", systemImage: "map").font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                                .padding(.horizontal, 14).frame(minHeight: 42)
                                .chunky(radius: 12)
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.top, 10)
                }
                .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 14)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 18))
        .card()
        .task(id: callId) {
            card = await model.route(callId: callId)
            loaded = true
        }
    }

    private func head(_ route: RouteOption) -> String {
        "\(route.departs) to \(route.arrives) · \(route.minutes) min" + changes(route, prefix: " · ")
    }

    private func chip(_ route: RouteOption) -> String {
        "\(route.minutes) min" + changes(route, prefix: ", ")
    }

    private func changes(_ route: RouteOption, prefix: String) -> String {
        route.transfers == 0 ? "" : "\(prefix)\(route.transfers) change\(route.transfers > 1 ? "s" : "")"
    }

    /// Apple Maps plans the trip again between the route's ends, with its own live times.
    private func openMaps(_ route: RouteOption, directions: Bool) {
        let points = route.legs.flatMap(\.points)
        guard let first = points.first, let last = points.last else { return }
        let point = { (p: RoutePoint) in "\(p.latitude),\(p.longitude)" }
        var url = URLComponents(string: "maps://")!
        if directions {
            url.queryItems = [.init(name: "saddr", value: point(first)), .init(name: "daddr", value: point(last)),
                              .init(name: "dirflg", value: "r")]
        } else {
            let name = route.legs.last?.to ?? ""
            url.queryItems = [.init(name: "ll", value: point(last)), .init(name: "q", value: name.isEmpty ? point(last) : name)]
        }
        if let link = url.url { openURL(link) }
    }
}

private struct WalkLeg: View {
    let leg: RouteLeg

    var body: some View {
        let km = leg.meters >= 1000 ? String(format: ", %.1f km", Double(leg.meters) / 1000) : ""
        HStack(spacing: 10) {
            Image(systemName: "figure.walk").font(.system(size: 14)).frame(width: 34)
            Text("Walk \(leg.minutes) min\(km) to \(leg.to.isEmpty ? "destination" : leg.to)")
        }
        .font(.body(13)).foregroundStyle(Mint.muted)
    }
}

private struct RideLeg: View {
    let leg: RouteLeg

    var body: some View {
        let color = Color(hexString: leg.color) ?? Mint.brand
        let badge = [leg.line, leg.name, leg.mode.capitalized].first { !$0.isEmpty } ?? ""
        HStack(alignment: .top, spacing: 10) {
            Text(badge).font(.body(11, .bold)).lineLimit(1)
                .foregroundStyle(Color(hexString: leg.textColor) ?? .white)
                .padding(.horizontal, 5).frame(minWidth: 34, minHeight: 22)
                .background(RoundedRectangle(cornerRadius: 6).fill(color))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Mint.ledge, lineWidth: 1.5))
            VStack(alignment: .leading, spacing: 1) {
                Text(leg.name.isEmpty ? badge : leg.name).fontWeight(.semibold)
                    + Text(leg.headsign.isEmpty ? "" : " toward \(leg.headsign)").foregroundStyle(Mint.muted)
                Text(leg.departs).fontWeight(.semibold).monospacedDigit()
                    + Text(" \(leg.from)" + (leg.fromTrack.isEmpty ? "" : ", platform \(leg.fromTrack)"))
                if leg.stops > 0 {
                    Text("\(leg.stops) stop\(leg.stops > 1 ? "s" : "")").foregroundStyle(Mint.muted)
                }
                Text(leg.arrives).fontWeight(.semibold).monospacedDigit()
                    + Text(" \(leg.to.isEmpty ? "destination" : leg.to)")
            }
            .font(.body(13)).foregroundStyle(Mint.ink)
            .padding(.leading, 10)
            .overlay(alignment: .leading) { Rectangle().fill(color).frame(width: 3) }
        }
    }
}

/// The route over a muted Apple Maps snapshot: walks dotted, rides in their line's colour with a stop at each end,
/// a dot where it starts and a coral one where it ends. Both are laid out in MapKit's map points, so they line up.
private struct RouteSketch: View {
    let route: RouteOption
    @Environment(\.colorScheme) private var scheme
    @Environment(\.displayScale) private var displayScale
    @State private var map: UIImage?

    var body: some View {
        GeometryReader { geometry in
            let fit = RouteFit(points: route.legs.flatMap(\.points), size: geometry.size, pad: 14)
            ZStack {
                if let map { Image(uiImage: map).resizable() }
                if let fit { lines(fit) }
            }
            .task(id: fit.map { "\($0.rect.origin.x),\($0.rect.origin.y),\($0.rect.size.width),\(scheme)" }) {
                // An old snapshot would sit under the new route's lines out of place.
                map = nil
                guard let fit else { return }
                map = await fit.snapshot(size: geometry.size, dark: scheme == .dark, scale: displayScale)
            }
        }
        .accessibilityLabel("Map of the route")
    }

    private func lines(_ fit: RouteFit) -> some View {
        Canvas { context, _ in
            let points = route.legs.flatMap(\.points)
            guard let first = points.first, let last = points.last else { return }
            let at = fit.at
            func dot(_ p: RoutePoint, _ radius: Double) -> Path {
                let c = at(p)
                return Path(ellipseIn: CGRect(x: c.x - radius, y: c.y - radius, width: radius * 2, height: radius * 2))
            }
            for leg in route.legs where !leg.points.isEmpty {
                var path = Path()
                path.addLines(leg.points.map(at))
                if leg.mode == "walk" {
                    context.stroke(path, with: .color(Mint.muted),
                                   style: StrokeStyle(lineWidth: 2.5, lineCap: .round, dash: [0.1, 6]))
                } else {
                    context.stroke(path, with: .color(Color(hexString: leg.color) ?? Mint.brand),
                                   style: StrokeStyle(lineWidth: 5, lineCap: .round, lineJoin: .round))
                }
            }
            for leg in route.legs where leg.mode != "walk" {
                for p in [leg.points.first, leg.points.last].compactMap({ $0 }) {
                    context.fill(dot(p, 4), with: .color(Mint.surface))
                    context.stroke(dot(p, 4), with: .color(Mint.ink), lineWidth: 1.8)
                }
            }
            context.fill(dot(first, 5), with: .color(Mint.brand))
            context.fill(dot(last, 7), with: .color(Mint.page))
            context.fill(dot(last, 6), with: .color(Mint.accent))
        }
    }
}

/// The route's points fitted to `size` with `pad` around them, in MapKit's map points (Web Mercator).
private struct RouteFit {
    /// The map points the view shows, edge to edge.
    let rect: MKMapRect
    private let size: CGSize

    init?(points: [RoutePoint], size: CGSize, pad: Double) {
        guard !points.isEmpty, size.width > 0, size.height > 0 else { return nil }
        let mapped = points.map { MKMapPoint(CLLocationCoordinate2D(latitude: $0.latitude, longitude: $0.longitude)) }
        let xs = mapped.map(\.x), ys = mapped.map(\.y)
        let left = xs.min()!, right = xs.max()!, top = ys.min()!, bottom = ys.max()!
        // Screen points per map point, never closer than street level.
        let scale = min((size.width - 2 * pad) / max(right - left, 1e-6),
                        (size.height - 2 * pad) / max(bottom - top, 1e-6),
                        256 * pow(2, 17) / MKMapSize.world.width)
        let width = size.width / scale, height = size.height / scale
        rect = MKMapRect(x: (left + right - width) / 2, y: (top + bottom - height) / 2, width: width, height: height)
        self.size = size
    }

    func at(_ p: RoutePoint) -> CGPoint {
        let m = MKMapPoint(CLLocationCoordinate2D(latitude: p.latitude, longitude: p.longitude))
        return CGPoint(x: (m.x - rect.minX) / rect.width * size.width, y: (m.y - rect.minY) / rect.height * size.height)
    }

    func snapshot(size: CGSize, dark: Bool, scale: CGFloat) async -> UIImage? {
        let options = MKMapSnapshotter.Options()
        options.mapRect = rect
        options.size = size
        options.scale = scale
        options.preferredConfiguration = MKStandardMapConfiguration(emphasisStyle: .muted)
        options.pointOfInterestFilter = .excludingAll
        options.traitCollection = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        return try? await MKMapSnapshotter(options: options).start().image
    }
}

private extension Color {
    /// "#RRGGBB" as the server sends line colours, or nil when there is none.
    init?(hexString: String) {
        let digits = hexString.hasPrefix("#") ? String(hexString.dropFirst()) : hexString
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        self.init(hex: value)
    }
}
