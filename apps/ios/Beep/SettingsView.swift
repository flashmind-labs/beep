import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List {
            Section {
                HStack(spacing: 12) {
                    TintIcon(system: "person", tint: Mint.tints[1])
                    VStack(alignment: .leading, spacing: 2) {
                        Text(model.accountName).font(.body(16, .semibold))
                        Text("This iPhone's account").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
                if let file = keyFile {
                    ShareLink(item: file) { Label("Save key", systemImage: "key") }
                }
            } footer: {
                Text("The key file signs another phone into this account. Keep it private.")
            }

            Section("Server") {
                HStack {
                    Text("Status")
                    Spacer()
                    Circle().fill(model.connected ? Mint.ok : Mint.error).frame(width: 8, height: 8)
                    Text(model.connected ? "Connected" : (model.connectionError.isEmpty ? "Connecting" : model.connectionError))
                        .foregroundStyle(Mint.muted).lineLimit(1)
                }
            }

            if let models = model.models, !models.choices.isEmpty {
                Section("Model") {
                    Picker("Model", selection: Binding(
                        get: { models.selected },
                        set: { id in Task { await model.setModel(id) } }
                    )) {
                        ForEach(models.choices, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    Picker("Thinking", selection: Binding(
                        get: { models.thinking },
                        set: { level in Task { await model.setThinking(level) } }
                    )) {
                        Text("Off").tag(ReasoningLevel.off)
                        Text("Low").tag(ReasoningLevel.low)
                        Text("Medium").tag(ReasoningLevel.medium)
                        Text("High").tag(ReasoningLevel.high)
                    }
                    .pickerStyle(.segmented)
                    if models.autoAvailable {
                        Toggle("Pick the model for me", isOn: Binding(
                            get: { models.auto },
                            set: { on in Task { await model.setAuto(on) } }
                        ))
                    }
                }
            }

            Section {
                NavigationLink(value: Route.memories) {
                    Label("Memories", systemImage: "brain")
                }
                NavigationLink(value: Route.routines) {
                    Label("Routines", systemImage: "clock")
                }
            }
        }
        .font(.body(15))
        .scrollContentBackground(.hidden)
        .background(Mint.page)
        .navigationTitle("Settings")
    }

    /// The key as the same JSON profile the Android app saves and imports.
    private var keyFile: URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("beep-key.json")
        let data = Data(Identity.export(model.identity).utf8)
        guard (try? data.write(to: url, options: [.atomic, .completeFileProtection])) != nil
        else { return nil }
        return url
    }
}
