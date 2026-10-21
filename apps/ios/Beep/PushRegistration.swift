import Foundation

/// What the server needs to push to this install through APNs, besides the token.
enum PushRegistration {
    /// A random id for this install, kept so registering again replaces its old token.
    static var deviceId: String {
        let key = "push_device_id"
        if let id = UserDefaults.standard.string(forKey: key) { return id }
        let id = UUID().uuidString.lowercased()
        UserDefaults.standard.set(id, forKey: key)
        return id
    }

    /// The APNs server this build's tokens belong to. A build signed for development, as from
    /// Xcode, says so in its provisioning profile; TestFlight and App Store builds have none
    /// or a distribution one, and use production.
    static let environment: ApnsEnvironment = {
        #if targetEnvironment(simulator)
        return .sandbox
        #else
        guard let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision"),
              let data = try? Data(contentsOf: url),
              let text = String(data: data, encoding: .isoLatin1)
        else { return .production }
        let development = #"<key>aps-environment</key>\s*<string>development</string>"#
        return text.range(of: development, options: .regularExpression) == nil ? .production : .sandbox
        #endif
    }()
}
