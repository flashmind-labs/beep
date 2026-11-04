import SwiftUI

/// The phone and the computers linked to its account, as on Android. A computer with no
/// account shows a link code; scanning it here asks first, then links it. A computer with its
/// own account shows a join code instead, and joining moves this phone's account into it.
struct LinkedDevicesView: View {
    @Environment(AppModel.self) private var model
    let back: () -> Void
    @State private var devices: [LinkedDevice]?
    /// Keys that are machines running beep-node.
    @State private var machines: Set<String> = []
    @State private var removing: LinkedDevice?
    @State private var error = ""

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Linked devices", back: back)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
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
                .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
            }
            if devices != nil {
                ComputerCard(buttons: true) { Task { await refresh() } }
                    .padding(.horizontal, Mint.pageInset).padding(.bottom, 8)
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await refresh() } }
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
