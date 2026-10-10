import CoreLocation
import Observation

/// Sends the phone's location to the server when the user allowed it, and
/// watches saved places so arriving or leaving is reported to the server.
@MainActor
@Observable
final class LocationReporter: NSObject, CLLocationManagerDelegate {
    struct Fix {
        var latitude: Double
        var longitude: Double
        var accuracy: Double
        var place: String?
    }

    @ObservationIgnored var onUpdate: ((Fix) -> Void)?
    /// A saved place's id and whether the phone arrived or left.
    @ObservationIgnored var onCross: ((String, PlaceCrossing) -> Void)?
    private(set) var status = CLAuthorizationStatus.notDetermined
    @ObservationIgnored private let manager = CLLocationManager()
    @ObservationIgnored private let geocoder = CLGeocoder()

    override init() {
        super.init()
        status = manager.authorizationStatus
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
    }

    var authorized: Bool {
        [.authorizedWhenInUse, .authorizedAlways].contains(status)
    }

    /// Geofencing only runs with the app closed when location is set to Always.
    var always: Bool { status == .authorizedAlways }

    func request() {
        manager.requestWhenInUseAuthorization()
    }

    /// Asks for Always. iOS shows the prompt once; after that only Settings can change it.
    func requestAlways() {
        if status == .notDetermined { manager.requestWhenInUseAuthorization() }
        manager.requestAlwaysAuthorization()
    }

    /// Asks for one fix if allowed.
    func refresh() {
        if authorized { manager.requestLocation() }
    }

    /// Monitors each saved place as a region. iOS allows 20 per app, so the first 20 are watched.
    func watch(_ places: [SavedPlace]) {
        let wanted = Dictionary(places.prefix(20).map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        for region in manager.monitoredRegions where wanted[region.identifier] == nil {
            manager.stopMonitoring(for: region)
        }
        guard always, CLLocationManager.isMonitoringAvailable(for: CLCircularRegion.self) else { return }
        for place in wanted.values {
            let center = CLLocationCoordinate2D(latitude: place.latitude, longitude: place.longitude)
            let radius = min(Double(place.radius), manager.maximumRegionMonitoringDistance)
            let region = CLCircularRegion(center: center, radius: radius, identifier: place.id)
            region.notifyOnEntry = true
            region.notifyOnExit = true
            manager.startMonitoring(for: region)
        }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let status = manager.authorizationStatus
        Task { @MainActor in
            self.status = status
            self.refresh()
        }
    }

    /// Crossing a place's edge goes to the server as is. A fix taken now can sit
    /// inside the radius on leaving, so the region event is the answer.
    nonisolated func locationManager(_ manager: CLLocationManager, didEnterRegion region: CLRegion) {
        let id = region.identifier
        Task { @MainActor in self.onCross?(id, .arrive) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didExitRegion region: CLRegion) {
        let id = region.identifier
        Task { @MainActor in self.onCross?(id, .leave) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let last = locations.last else { return }
        Task { @MainActor in
            let marks = try? await self.geocoder.reverseGeocodeLocation(last)
            let place = marks?.first.map { [$0.locality, $0.country].compactMap { $0 }.joined(separator: ", ") }
            self.onUpdate?(Fix(
                latitude: last.coordinate.latitude, longitude: last.coordinate.longitude,
                accuracy: last.horizontalAccuracy, place: place))
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {}
}
