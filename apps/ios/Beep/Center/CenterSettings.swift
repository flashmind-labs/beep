import SwiftUI

/// Settings from Home's gear: Appearance, Notifications and the linked devices.
struct CenterSettingsView: View {
    @Binding var path: [Route]

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Settings", back: { if !path.isEmpty { path.removeLast() } })
            CCBody {
                CCPanel {
                    CCRow(title: "Appearance", chevron: true, action: { path.append(.appearance) }) { EmptyView() } right: {
                        Text(Skins.all.first { $0.id == Appearance.shared.choice }?.name ?? "Match system")
                    }
                    CCRow(title: "Notifications", chevron: true, action: { path.append(.notifySettings) }) { EmptyView() } right: { EmptyView() }
                    CCRow(title: "Devices", chevron: true, action: { path.append(.devices) }) { EmptyView() } right: { EmptyView() }
                }
            }
        }
        .background(Mint.page).foregroundStyle(Mint.ink)
        .toolbar(.hidden, for: .navigationBar)
    }
}

struct NotifySettingsView: View {
    @Binding var path: [Route]
    @State private var asks = NotifyPrefs.on(NotifyPrefs.asks)
    @State private var finished = NotifyPrefs.on(NotifyPrefs.finished)
    @State private var ci = NotifyPrefs.on(NotifyPrefs.ci)

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: "Notifications", back: { if !path.isEmpty { path.removeLast() } })
            CCBody {
                CCPanel {
                    CCRow(title: "Agent asks", subtitle: "Allow and Deny on the notification") { EmptyView() } right: { CCToggle(isOn: $asks) }
                    CCRow(title: "Agent finished") { EmptyView() } right: { CCToggle(isOn: $finished) }
                    CCRow(title: "Builds that ended") { EmptyView() } right: { CCToggle(isOn: $ci) }
                }
                CCNote(text: "Asks arrive live while Beep is open. When it is closed, iOS lets Beep check now and then.")
            }
        }
        .background(Mint.page).foregroundStyle(Mint.ink)
        .toolbar(.hidden, for: .navigationBar)
        .onChange(of: asks) { _, on in UserDefaults.standard.set(on, forKey: NotifyPrefs.asks) }
        .onChange(of: finished) { _, on in UserDefaults.standard.set(on, forKey: NotifyPrefs.finished) }
        .onChange(of: ci) { _, on in UserDefaults.standard.set(on, forKey: NotifyPrefs.ci) }
    }
}
