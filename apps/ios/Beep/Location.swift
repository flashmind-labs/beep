import CoreLocation

/// Sends the phone's location to the server when the user allowed it.
@MainActor
final class LocationReporter: NSObject, CLLocationManagerDelegate {
    struct Fix {
        var latitude: Double
        var longitude: Double
        var accuracy: Double
        var place: String?
    }

    var onUpdate: ((Fix) -> Void)?
    private let manager = CLLocationManager()
    private let geocoder = CLGeocoder()

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
    }

    var authorized: Bool {
        [.authorizedWhenInUse, .authorizedAlways].contains(manager.authorizationStatus)
    }

    func request() {
        manager.requestWhenInUseAuthorization()
    }

    /// Asks for one fix if allowed.
    func refresh() {
        if authorized { manager.requestLocation() }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        Task { @MainActor in self.refresh() }
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
