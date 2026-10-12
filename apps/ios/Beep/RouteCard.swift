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
                    RouteSketch(route: route).padding(.horizontal, 8).padding(.vertical, 6)
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

/// The route's shape fitted to the box: walks dotted, rides in their line's colour with a stop at each end,
/// a dot where it starts and a coral one where it ends. Longitude is squeezed by latitude so shapes keep their angles.
private struct RouteSketch: View {
    let route: RouteOption

    var body: some View {
        Canvas { context, size in
            let points = route.legs.flatMap(\.points)
            guard let first = points.first, let last = points.last else { return }
            let squeeze = cos(points.map(\.latitude).reduce(0, +) / Double(points.count) * .pi / 180)
            let xs = points.map { $0.longitude * squeeze }, ys = points.map(\.latitude)
            let left = xs.min()!, right = xs.max()!, bottom = ys.min()!, top = ys.max()!
            let span = max(right - left, top - bottom, 1e-9)
            let pad = 8.0
            let scale = min((size.width - 2 * pad) / max(right - left, span * 1e-3),
                            (size.height - 2 * pad) / max(top - bottom, span * 1e-3))
            let dx = (size.width - (right - left) * scale) / 2, dy = (size.height - (top - bottom) * scale) / 2
            func at(_ p: RoutePoint) -> CGPoint {
                CGPoint(x: dx + (p.longitude * squeeze - left) * scale, y: dy + (top - p.latitude) * scale)
            }
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
        .accessibilityLabel("Sketch of the route")
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
