import AlarmKit
import Contacts
import SwiftUI
import UIKit

/// An alarm a set_alarm call asked for. `days` are weekdays, empty for one that rings once.
struct AlarmRequest: Equatable {
    var hour: Int
    var minute: Int
    var label: String
    var days: [Locale.Weekday]

    var time: String { String(format: "%d:%02d", hour, minute) }

    private static let weekdays: [String: Locale.Weekday] = [
        "monday": .monday, "tuesday": .tuesday, "wednesday": .wednesday, "thursday": .thursday,
        "friday": .friday, "saturday": .saturday, "sunday": .sunday,
    ]

    init?(_ json: [String: Any]) {
        guard let hour = json["hour"] as? Int, (0...23).contains(hour),
              let minute = json["minute"] as? Int, (0...59).contains(minute) else { return nil }
        self.hour = hour
        self.minute = minute
        label = json["label"] as? String ?? ""
        days = (json["days"] as? [String] ?? []).compactMap { Self.weekdays[$0] }
    }

    /// "Mon, Wed" in the phone's language.
    var dayNames: String {
        let symbols = Calendar.current.shortWeekdaySymbols
        let order: [Locale.Weekday] = [.sunday, .monday, .tuesday, .wednesday, .thursday, .friday, .saturday]
        return days.compactMap { day in order.firstIndex(of: day).map { symbols[$0] } }.joined(separator: ", ")
    }
}

/// Opens the apps open_app names, with the iOS link for each and the web page when the app is missing.
enum PhoneApps {
    @MainActor
    static func open(app: String, uri: String, fallback: String) {
        let links = [iosLink(app: app, uri: uri), URL(string: fallback)].compactMap { $0 }.filter { $0.scheme != nil }
        open(links)
    }

    @MainActor
    private static func open(_ links: [URL]) {
        guard let first = links.first else { return }
        UIApplication.shared.open(first) { opened in
            if !opened { Task { @MainActor in open(Array(links.dropFirst())) } }
        }
    }

    /// The server sends Android links; most apps have an iOS link of their own.
    private static func iosLink(app: String, uri: String) -> URL? {
        if uri.hasPrefix("https://") || uri.hasPrefix("http://") { return URL(string: uri) }
        switch app.lowercased() {
        case "spotify": return URL(string: uri.hasPrefix("spotify:") ? uri : "spotify:")
        case "youtube": return URL(string: "youtube://")
        case "whatsapp": return URL(string: uri.hasPrefix("whatsapp:") ? uri : "whatsapp://send")
        case "maps": return mapsLink(geo: uri)
        case "phone": return URL(string: uri.hasPrefix("tel:") && uri.count > 4 ? uri : "mobilephone://")
        case "clock": return URL(string: "clock-alarm://")
        case "gmail": return URL(string: "googlegmail://")
        case "calendar": return URL(string: "calshow://")
        default: return URL(string: uri)
        }
    }

    /// `geo:0,0?q=Dubai+Mall` as an Apple Maps link.
    private static func mapsLink(geo: String) -> URL? {
        var link = URLComponents(string: "maps://")!
        guard geo.hasPrefix("geo:") else { return link.url }
        let body = geo.dropFirst(4)
        let parts = body.split(separator: "?", maxSplits: 1).map(String.init)
        var items: [URLQueryItem] = []
        if let at = parts.first, at != "0,0", !at.isEmpty { items.append(.init(name: "ll", value: at)) }
        if parts.count > 1, let q = URLComponents(string: "?" + parts[1])?.queryItems?.first(where: { $0.name == "q" })?.value {
            items.append(.init(name: "q", value: q.replacingOccurrences(of: "+", with: " ")))
        }
        link.queryItems = items.isEmpty ? nil : items
        return link.url
    }
}

/// Alarms in the Clock, through AlarmKit on iOS 26 and later. Earlier versions cannot set one.
enum PhoneAlarms {
    static var canSet: Bool {
        if #available(iOS 26, *) { return true }
        return false
    }

    static func set(_ alarm: AlarmRequest) async {
        guard #available(iOS 26, *) else { return }
        try? await schedule(alarm)
    }

    @available(iOS 26, *)
    private static func schedule(_ alarm: AlarmRequest) async throws {
        let manager = AlarmManager.shared
        if manager.authorizationState == .notDetermined { _ = try await manager.requestAuthorization() }
        guard manager.authorizationState == .authorized else { return }
        let time = Alarm.Schedule.Relative.Time(hour: alarm.hour, minute: alarm.minute)
        let schedule = Alarm.Schedule.relative(.init(time: time, repeats: alarm.days.isEmpty ? .never : .weekly(alarm.days)))
        let title = alarm.label.isEmpty ? "Alarm" : alarm.label
        let alert: AlarmPresentation.Alert
        if #available(iOS 26.1, *) {
            alert = AlarmPresentation.Alert(title: LocalizedStringResource(stringLiteral: title))
        } else {
            alert = AlarmPresentation.Alert(title: LocalizedStringResource(stringLiteral: title),
                                            stopButton: AlarmButton(text: "Stop", textColor: .white, systemImageName: "stop.circle"))
        }
        let attributes = AlarmAttributes<BeepAlarm>(presentation: AlarmPresentation(alert: alert), tintColor: Mint.brand)
        _ = try await manager.schedule(id: UUID(), configuration: .alarm(schedule: schedule, attributes: attributes))
    }
}

@available(iOS 26, *)
struct BeepAlarm: AlarmMetadata {}

/// The query_contacts lookup, answered from the phone's contacts in the shape Android sends.
enum PhoneContacts {
    static func search(_ name: String) async -> String {
        let store = CNContactStore()
        let allowed: Bool
        switch CNContactStore.authorizationStatus(for: .contacts) {
        case .authorized, .limited: allowed = true
        case .notDetermined: allowed = (try? await store.requestAccess(for: .contacts)) ?? false
        default: allowed = false
        }
        guard allowed else {
            return json(["error": "permission_denied",
                         "message": "Contacts access is off. The user needs to allow it in Settings, Beep, Contacts."])
        }
        return await Task.detached { json(found(name, in: store)) }.value
    }

    private static func found(_ name: String, in store: CNContactStore) -> [String: Any] {
        let keys: [CNKeyDescriptor] = [
            CNContactFormatter.descriptorForRequiredKeys(for: .fullName),
            CNContactPhoneNumbersKey as CNKeyDescriptor, CNContactEmailAddressesKey as CNKeyDescriptor,
        ]
        var contacts: [CNContact] = []
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty {
            let request = CNContactFetchRequest(keysToFetch: keys)
            try? store.enumerateContacts(with: request) { contact, stop in
                contacts.append(contact)
                if contacts.count >= 10 { stop.pointee = true }
            }
        } else {
            contacts = (try? store.unifiedContacts(matching: CNContact.predicateForContacts(matchingName: trimmed),
                                                    keysToFetch: keys)) ?? []
        }
        let list: [[String: Any]] = contacts.prefix(10).compactMap { contact in
            guard let full = CNContactFormatter.string(from: contact, style: .fullName), !full.isEmpty else { return nil }
            var entry: [String: Any] = ["name": full]
            let phones = contact.phoneNumbers.map { phone -> [String: String] in
                ["number": phone.value.stringValue,
                 "type": phone.label.map { CNLabeledValue<CNPhoneNumber>.localizedString(forLabel: $0) } ?? ""]
            }
            if !phones.isEmpty { entry["phones"] = phones }
            let emails = contact.emailAddresses.map { ["address": $0.value as String] }
            if !emails.isEmpty { entry["emails"] = emails }
            return entry
        }
        return ["contacts": list, "count": list.count]
    }

    private static func json(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object) else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }
}
