import SwiftUI
import AVFoundation

/// The phone and the computers linked to its account, as on Android. A computer with no
/// account shows a link code; scanning it here asks first, then links it. A computer with its
/// own account shows a join code instead, and joining moves this phone's account into it.
struct LinkedDevicesView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    var home = false
    @State private var devices: [LinkedDevice]?
    /// Keys that are machines running beep-node.
    @State private var machines: Set<String> = []
    @State private var removing: LinkedDevice?
    @State private var error = ""

    var body: some View {
        if home && !serverEnabled() {
            WorkspaceLinkView(firstRun: true)
                .toolbar(.hidden, for: .navigationBar)
        } else {
            devicesPage
        }
    }

    private var devicesPage: some View {
        VStack(spacing: 0) {
            BackBar(title: "Linked devices", back: back, showsBack: !home)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    WorkspaceLinkView()
                    if serverEnabled() {
                    Text("Devices on this account").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                        .padding(.bottom, 4)
                    let list = devices ?? []
                    let owner = list.contains { $0.primary && $0.thisDevice }
                    ForEach(list, id: \.id) { device in
                        row(device, canRemove: owner)
                    }
                    Rectangle().fill(Mint.soft).frame(height: 1).padding(.vertical, 10)
                    Text(note(list)).font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(4)
                    if !error.isEmpty {
                        Text(error).font(.body(12)).foregroundStyle(Mint.error).padding(.top, 8)
                    }
                    }
                }
                .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
            }
            if serverEnabled(), devices != nil {
                ComputerCard(buttons: true) { Task { await refresh() } }
                    .padding(.horizontal, Mint.pageInset).padding(.bottom, 8)
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if serverEnabled(), model.connected { await refresh() } }
        .alert("Remove \(removing?.name ?? "")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
               presenting: removing) { device in
            Button("Remove", role: .destructive) { Task { await remove(device) } }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("It is signed out at once and can no longer see your chats or memories.")
        }
    }

    private func note(_ list: [LinkedDevice]) -> String {
        let start = if let joined = list.first(where: { $0.primary && !$0.thisDevice }) {
            "This phone is on \(joined.name)'s account. "
        } else if list.count > 1 {
            "A linked device sees your chats and memories. "
        } else {
            "No computers linked yet. "
        }
        return start + "Any device on the account can add another. Phone actions such as contacts and alarms stay on this phone."
    }

    private func row(_ device: LinkedDevice, canRemove: Bool) -> some View {
        let computer = device.kind == .desktop
        let machine = machines.contains(device.id)
        let note = if device.thisDevice {
            "This phone"
        } else if machine {
            "Machine, linked " + device.linkedAt.formatted(.relative(presentation: .named))
        } else if computer && device.primary {
            "Desktop"
        } else if computer {
            "Desktop, linked " + device.linkedAt.formatted(.relative(presentation: .named))
        } else {
            "Phone"
        }
        return HStack(spacing: 14) {
            Image(systemName: machine ? "server.rack" : computer ? "laptopcomputer" : "iphone")
                .font(.system(size: 18)).foregroundStyle(Mint.ink)
                .frame(width: 40, height: 40)
                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.tints[machine ? 2 : computer ? 0 : 1]))
            VStack(alignment: .leading, spacing: 1) {
                Text(device.thisDevice && !computer ? UIDevice.current.name : device.name)
                    .font(.body(14, .medium)).foregroundStyle(Mint.ink)
                Text(note).font(.body(12)).foregroundStyle(Mint.muted)
            }
            Spacer(minLength: 0)
            if device.primary {
                Text("Owner").font(.body(12, .medium)).foregroundStyle(Mint.ink)
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.tints[1]))
            } else if canRemove {
                Button("Remove") { removing = device }.buttonStyle(TextAction(color: Mint.error))
            }
        }
        .padding(.vertical, 10)
    }

    private func refresh() async {
        do {
            devices = try await model.devices()
            error = ""
            // When the list fails, no key is marked as a machine.
            if let list = try? await model.machines() { machines = Set(list.map(\.id)) }
        } catch {
            self.error = describe(error)
        }
    }

    private func remove(_ device: LinkedDevice) async {
        do {
            try await model.removeDevice(device.id)
            await refresh()
        } catch {
            self.error = describe(error)
        }
    }
}

private struct WorkspaceLinkView: View {
    @Environment(AppModel.self) private var model
    var firstRun = false
    @State private var scanningFirstRun = false
    @State private var linked = false
    @State private var adding = false
    @State private var ticket = ""
    @State private var scanning = false
    @State private var busy = false
    @State private var error = ""

    var body: some View {
        SwiftUI.Group {
            if firstRun { welcome } else { workspaceSummary }
        }
        .background(Mint.page)
        .qrScanner(isPresented: $scanningFirstRun) { code in join(code) }
        .task {
            if !serverEnabled() { model.connect() }
            do { linked = try await model.workspace.client().workspaceLinked() } catch { self.error = describe(error) }
        }
        .sheet(isPresented: $adding) { pairingSheet }
    }

    private var welcome: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                Image("flashmind_mark")
                    .resizable().scaledToFit()
                    .frame(width: 32, height: 32)
                    .foregroundStyle(Mint.brand)
                    .frame(width: 64, height: 64)
                    .background(Mint.raised, in: RoundedRectangle(cornerRadius: 16))
                    .accessibilityHidden(true)
                Text("Connect your computer")
                    .font(.system(.largeTitle, design: .default, weight: .semibold))
                    .foregroundStyle(Mint.ink)
                    .accessibilityAddTraits(.isHeader)
                Text("Run coding agents, follow their progress and manage your projects from your phone.")
                    .font(.system(.body)).foregroundStyle(Mint.muted)
                VStack(alignment: .leading, spacing: 16) {
                    Text("On your computer").font(.system(.subheadline, weight: .semibold))
                        .accessibilityAddTraits(.isHeader)
                    step(1, "Open Beep on your desktop.")
                    step(2, "Open **Machines**, then **Add a machine**.")
                    step(3, "Choose **Phone** to show a QR code.")
                    Text("Pairing codes work once and expire after 5 minutes.")
                        .font(.system(.footnote)).foregroundStyle(Mint.muted)
                }
                .foregroundStyle(Mint.ink)
                if let download = URL(string: "https://releases.flashmind.com/beep/macos/Beep-latest.dmg") {
                    ShareLink("Get Beep for Mac", item: download)
                        .font(.system(.subheadline)).tint(Mint.brand)
                        .frame(minHeight: 48)
                        .accessibilityHint("Share the download link with your Mac")
                }
                if !QRScanner.supported {
                    Text("Camera unavailable on this device. Paste a pairing code instead.")
                        .font(.system(.footnote)).foregroundStyle(Mint.muted)
                }
                WorkspaceError(text: error)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(24)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(spacing: 8) {
                Button { startScanning() } label: {
                    if busy { ProgressView("Connecting computer").tint(Mint.onAccent) }
                    else { Label("Scan QR code", systemImage: "qrcode.viewfinder") }
                }
                .buttonStyle(PrimaryButton())
                .disabled(busy || !QRScanner.supported)
                Button("Paste pairing code") { error = ""; adding = true }
                    .buttonStyle(TextAction())
                    .frame(minHeight: 48)
                    .disabled(busy)
                Text("Connect once. Then reach your nodes even when the desktop is offline.")
                    .font(.system(.footnote)).foregroundStyle(Mint.muted)
                    .multilineTextAlignment(.center)
            }
            .padding(.horizontal, 24).padding(.vertical, 16)
            .background(Mint.page)
        }
    }

    private func step(_ number: Int, _ text: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text("\(number).").foregroundStyle(Mint.muted)
            Text(text).frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.system(.subheadline))
        .accessibilityElement(children: .combine)
    }

    private var workspaceSummary: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Workspace").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
            Text(linked ? "Workspace linked. Projects and node sessions connect directly." :
                "Link your desktop workspace to manage projects and node sessions, even when the desktop is offline.")
                .font(.body(12)).foregroundStyle(Mint.muted)
            WorkspaceError(text: error)
            if !linked { Button("Link workspace") { adding = true }.buttonStyle(TextAction()) }
            Divider().overlay(Mint.soft)
        }
    }

    private var pairingSheet: some View {
        NavigationStack {
            Form {
                Text(firstRun ? "Copy the phone pairing code shown in Beep on your desktop." :
                    "On desktop, open Machines, Add a machine, then choose Phone. Scan the QR code it shows.")
                if !firstRun && QRScanner.supported {
                    Button("Scan QR code") { startScanning() }.disabled(busy)
                }
                TextField(firstRun ? "Paste pairing code" : "Phone pairing ticket", text: $ticket, axis: .vertical)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().disabled(busy)
                Text("This phone gets project controls, node sessions and shared credentials. Only pair with a computer you trust.")
                WorkspaceError(text: error)
                Button(busy ? "Connecting…" : firstRun ? "Connect computer" : "Link workspace") { join(ticket) }
                    .disabled(busy || ticket.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .qrScanner(isPresented: $scanning) { code in
                ticket = code
                join(code)
            }
            .navigationTitle(firstRun ? "Paste pairing code" : "Link workspace")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { adding = false }.disabled(busy) } }
        }.interactiveDismissDisabled(busy)
    }

    private func startScanning() {
        error = ""
        Task {
            let allowed = await AVCaptureDevice.requestAccess(for: .video)
            if allowed && QRScanner.available {
                if firstRun { scanningFirstRun = true } else { scanning = true }
            } else {
                error = "Camera unavailable. Allow camera access in Settings, or paste a pairing code."
            }
        }
    }

    private func join(_ code: String) {
        let value = code.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !busy, !value.isEmpty else { return }
        busy = true
        error = ""
        Task {
            defer { busy = false }
            do {
                try await model.workspace.client().joinWorkspace(ticket: value, name: UIDevice.current.name)
                linked = true; adding = false; ticket = ""
                await model.workspace.refresh()
            } catch { self.error = describe(error) }
        }
    }
}
