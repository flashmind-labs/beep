import SwiftUI
import VisionKit

/// The camera, reading the first QR code it sees.
struct QRScanner: View {
    @Environment(\.dismiss) private var dismiss
    let found: (String) -> Void

    /// False on the simulator and on phones without a usable camera.
    static var available: Bool { DataScannerViewController.isSupported && DataScannerViewController.isAvailable }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Scanner { text in
                dismiss()
                found(text)
            }
            .ignoresSafeArea()
            Button("Cancel") { dismiss() }
                .buttonStyle(PillButton())
                .padding(20)
        }
        .background(Color.black)
    }

    private struct Scanner: UIViewControllerRepresentable {
        let found: (String) -> Void

        func makeUIViewController(context: Context) -> DataScannerViewController {
            let scanner = DataScannerViewController(
                recognizedDataTypes: [.barcode(symbologies: [.qr])],
                qualityLevel: .balanced, isHighlightingEnabled: true)
            scanner.delegate = context.coordinator
            try? scanner.startScanning()
            return scanner
        }

        func updateUIViewController(_ controller: DataScannerViewController, context: Context) {}

        func makeCoordinator() -> Coordinator { Coordinator(found: found) }

        final class Coordinator: NSObject, DataScannerViewControllerDelegate {
            let found: (String) -> Void
            private var done = false

            init(found: @escaping (String) -> Void) { self.found = found }

            func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem],
                             allItems: [RecognizedItem]) {
                guard !done else { return }
                for case .barcode(let code) in items {
                    guard let text = code.payloadStringValue else { continue }
                    done = true
                    scanner.stopScanning()
                    found(text)
                    return
                }
            }
        }
    }
}

extension View {
    /// Shows the scanner while `isPresented`. Without a camera, as on the
    /// simulator, it reads a copied code instead.
    func qrScanner(isPresented: Binding<Bool>, found: @escaping (String) -> Void) -> some View {
        fullScreenCover(isPresented: Binding(
            get: { isPresented.wrappedValue && QRScanner.available },
            set: { isPresented.wrappedValue = $0 }
        )) {
            QRScanner(found: found)
        }
        .onChange(of: isPresented.wrappedValue) { _, shown in
            guard shown, !QRScanner.available else { return }
            isPresented.wrappedValue = false
            found(UIPasteboard.general.string ?? "")
        }
    }
}

/// A link code the phone read, waiting for the user to confirm it.
private struct Scanned: Identifiable {
    let uri: String
    let preview: LinkPreview
    var id: String { uri }
}

/// A join code the phone read, and the computer that shows it.
private struct Invited: Identifiable {
    let code: String
    let preview: JoinPreview
    var id: String { code }
}

/// Joins a computer's account, or links a computer to this one, from the code it shows.
/// A link code links that computer to this phone's account; a join code moves this
/// phone's account into the computer's. Both ask first.
struct ComputerCard: View {
    @Environment(AppModel.self) private var model
    @State private var scanning = false
    @State private var typing = false
    @State private var typed = ""
    @State private var scanned: Scanned?
    @State private var invited: Invited?
    @State private var busy = false
    @State private var error = ""
    @State private var done: (title: String, text: String)?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                TintIcon(system: "laptopcomputer", tint: Mint.tints[1])
                VStack(alignment: .leading, spacing: 3) {
                    Text(done?.title ?? "Using Beep on a computer?").font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                    Text(done?.text ?? "Scan the code Beep shows on your computer to use the same chats and memories here.")
                        .font(.body(13)).foregroundStyle(Mint.muted)
                }
                Spacer(minLength: 4)
                if done != nil {
                    Image(systemName: "checkmark").font(.system(size: 16, weight: .bold)).foregroundStyle(Mint.ok)
                }
            }
            if done == nil {
                HStack(spacing: 8) {
                    if busy {
                        ProgressView()
                    } else {
                        Button("Scan code") { scanning = true }.buttonStyle(PillButton(fill: Mint.user))
                        Button("Type code") { typed = ""; typing = true }.buttonStyle(TextAction(color: Mint.muted))
                    }
                }
            }
            if !error.isEmpty { Text(error).font(.body(12)).foregroundStyle(Mint.error) }
        }
        .padding(14)
        .card()
        .qrScanner(isPresented: $scanning, found: read)
        .alert("Type the code", isPresented: $typing) {
            TextField("K7QM4-X9TRP", text: $typed)
                .font(.system(.body, design: .monospaced))
                .textInputAutocapitalization(.characters)
                .autocorrectionDisabled()
            Button("Cancel", role: .cancel) {}
            Button("Next") { read(typed) }
        } message: {
            Text("The code your computer shows under Settings, Devices.")
        }
        .sheet(item: $scanned) { offer in
            LinkSheet(preview: offer.preview) { scanned = nil } link: {
                scanned = nil
                link(offer)
            }
        }
        .sheet(item: $invited) { invite in
            JoinSheet(preview: invite.preview) { invited = nil } join: {
                invited = nil
                join(invite)
            }
        }
    }

    private func read(_ text: String) {
        let uri = text.trimmingCharacters(in: .whitespacesAndNewlines)
        error = ""
        if let code = joinCode(input: uri) {
            Task {
                do {
                    invited = Invited(code: code, preview: try await model.peekInvite(code))
                } catch {
                    self.error = joinMessage(error)
                }
            }
            return
        }
        if let preview = try? linkPreview(offerUri: uri) {
            scanned = Scanned(uri: uri, preview: preview)
        } else {
            error = "That is not a Beep code."
        }
    }

    private func link(_ offer: Scanned) {
        busy = true
        Task {
            do {
                let device = try await model.linkDevice(offer.uri)
                done = ("Linked \(device.name)", "It sees the chats and memories on this phone.")
            } catch {
                self.error = describe(error)
            }
            busy = false
        }
    }

    private func join(_ invite: Invited) {
        busy = true
        Task {
            do {
                try await model.joinAccount(invite.code)
                let name = invite.preview.name
                done = ("On \(name)'s account", "This phone now shows \(name)'s chats and memories.")
            } catch {
                self.error = joinMessage(error)
            }
            busy = false
        }
    }

    private func joinMessage(_ error: Error) -> String {
        if case BeepError.NotFound = error { return "That code is used up or expired. Show a new one on the computer." }
        return describe(error)
    }
}

/// Asks before linking a computer, with the code to check against its window.
private struct LinkSheet: View {
    let preview: LinkPreview
    let cancel: () -> Void
    let link: () -> Void

    var body: some View {
        SheetBody {
            SheetHeader(icon: "laptopcomputer", tint: Mint.tints[0], title: "Link \(preview.name)?", subtitle: "Desktop")
            Text("It will see your chats and memories.").font(.body(15)).foregroundStyle(Mint.ink)
            VStack(alignment: .leading, spacing: 4) {
                row("Code") { Text(preview.code).font(.system(size: 13, design: .monospaced)).tracking(1.3) }
                row("Stays here") { Text("Contacts, alarms, location").font(.body(13)) }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 14).fill(Mint.code))
            Text("Check that the code matches the one on the computer.").font(.body(12)).foregroundStyle(Mint.muted)
            HStack(spacing: 8) {
                Spacer()
                Button("Cancel", action: cancel).buttonStyle(TextAction(color: Mint.muted))
                Button("Link", action: link).buttonStyle(PillButton(fill: Mint.accent))
            }
        }
    }

    private func row(_ label: String, @ViewBuilder value: () -> some View) -> some View {
        HStack(spacing: 0) {
            Text(label).font(.body(13)).foregroundStyle(Mint.muted).frame(width: 84, alignment: .leading)
            value().foregroundStyle(Mint.ink)
        }
    }
}

/// Says what joining a computer's account moves.
private struct JoinSheet: View {
    let preview: JoinPreview
    let cancel: () -> Void
    let join: () -> Void

    var body: some View {
        SheetBody {
            SheetHeader(icon: preview.kind == .desktop ? "laptopcomputer" : "iphone", tint: Mint.tints[0],
                        title: "Join \(preview.name)'s account?",
                        subtitle: preview.kind == .desktop ? "Desktop" : "Phone")
            Text("This iPhone's chats, memories, routines and places move to \(preview.name)'s account, "
                + "and from then on both show the same chats. Apps you connected on this iPhone may need connecting again.")
                .font(.body(15)).foregroundStyle(Mint.ink)
            HStack(spacing: 8) {
                Spacer()
                Button("Cancel", action: cancel).buttonStyle(TextAction(color: Mint.muted))
                Button("Join", action: join).buttonStyle(PillButton(fill: Mint.accent))
            }
        }
    }
}
