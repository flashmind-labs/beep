import SwiftUI
import UniformTypeIdentifiers
import UserNotifications

struct OnboardingView: View {
    @Environment(AppModel.self) private var model
    @State private var notificationsAllowed = false
    @State private var importing = false
    @State private var imported = false
    @State private var busy = false
    @State private var error = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("b")
                    .font(.display(24))
                    .frame(width: 56, height: 56)
                    .chunky(Mint.user, radius: 18)
                Text("Almost ready.").font(.display(36)).foregroundStyle(Mint.ink)
                (Text("You are ") + Text(model.accountName).fontWeight(.semibold).foregroundColor(Mint.ink)
                    + Text(". Beep made a key for this iPhone and keeps it in the Keychain. Both permissions are optional."))
                    .font(.body(15))
                    .foregroundStyle(Mint.muted)
                    .lineSpacing(4)

                PermissionCard(
                    icon: "mappin.and.ellipse", tint: Mint.tints[0], title: "Location",
                    detail: "Nearby places, weather and directions.", done: model.location.authorized
                ) {
                    model.location.request()
                }
                PermissionCard(
                    icon: "bell", tint: Mint.tints[1], title: "Notifications",
                    detail: "When a routine finishes or a tool needs your OK.", done: notificationsAllowed
                ) {
                    Task {
                        let granted = try? await UNUserNotificationCenter.current()
                            .requestAuthorization(options: [.alert, .sound, .badge])
                        notificationsAllowed = granted ?? false
                    }
                }
                PermissionCard(
                    icon: "key", tint: Mint.tints[2],
                    title: imported ? "Signed in as \(model.accountName)" : "Already use Beep?",
                    detail: imported
                        ? "From the key file. It replaces the new account."
                        : "Import the key file saved from Settings on your other phone.",
                    done: imported, action: "Import", busy: busy
                ) { importing = true }
                ComputerCard()
                if !error.isEmpty {
                    Text(error).font(.body(13)).foregroundStyle(Mint.error)
                }

                Spacer(minLength: 24)
                Button("Continue") { model.onboarded = true }
                    .buttonStyle(PrimaryButton())
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 24)
        }
        .background(Mint.page)
        .fileImporter(isPresented: $importing, allowedContentTypes: [.json, .plainText, .data]) { result in
            guard case .success(let url) = result else { return }
            Task { await importKey(url) }
        }
    }

    private func importKey(_ url: URL) async {
        busy = true
        error = ""
        defer { busy = false }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let data = try Data(contentsOf: url)
            guard data.count <= 65536 else { throw BeepError.BadRequest(reason: "The file is too large") }
            try await model.importKey(String(decoding: data, as: UTF8.self))
            imported = true
        } catch {
            self.error = describe(error)
        }
    }
}

private struct PermissionCard: View {
    let icon: String
    let tint: Color
    let title: String
    let detail: String
    let done: Bool
    var action = "Allow"
    var busy = false
    let onTap: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            TintIcon(system: icon, tint: tint)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                Text(detail).font(.body(13)).foregroundStyle(Mint.muted)
            }
            Spacer(minLength: 4)
            if done {
                Image(systemName: "checkmark").font(.system(size: 16, weight: .bold)).foregroundStyle(Mint.ok)
            } else if busy {
                ProgressView()
            } else {
                Button(action, action: onTap).buttonStyle(PillButton())
            }
        }
        .padding(14)
        .card()
    }
}
