import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    /// With agents on, the agent Jev picks brings the model, and a failed pick
    /// falls back to General, so Auto on leaves nothing to pick here.
    @State private var agentsOn = false

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Settings", back: { path.removeAll() })
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    if let models = model.models, !models.choices.isEmpty {
                        modelPicker(models)
                    }
                    VStack(spacing: 0) {
                        divider
                        if let file = keyFile {
                            ShareLink(item: file) {
                                row("key", model.accountName.isEmpty ? "Account" : model.accountName,
                                    "Kept in your iPhone backups · tap to save it to a file")
                            }
                        }
                        divider
                        Button { if !model.connected { model.connect() } } label: {
                            row("server.rack", "Server", model.connected ? "Connected"
                                : (model.connectionError.isEmpty ? "Connecting" : model.connectionError),
                                dot: true)
                        }
                        divider
                    }
                    .buttonStyle(.plain)
                    ComputerCard()
                }
                .padding(.horizontal, 20).padding(.vertical, 8)
            }
        }
        .background(Mint.page)
        .task(id: model.models?.choices.isEmpty == false) {
            if model.models?.choices.isEmpty == false { agentsOn = await model.agentsOn() }
        }
    }

    private var divider: some View { Rectangle().fill(Mint.soft).frame(height: 1) }

    private func row(_ icon: String, _ label: String, _ detail: String, dot: Bool = false) -> some View {
        HStack(spacing: 14) {
            Image(systemName: icon).font(.system(size: 17)).foregroundStyle(Mint.muted).frame(width: 20)
            VStack(alignment: .leading, spacing: 1) {
                Text(label).font(.body(15, .medium)).foregroundStyle(Mint.ink)
                HStack(spacing: 6) {
                    if dot { ConnectionDot(connected: model.connected) }
                    Text(detail).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            Image(systemName: "chevron.right").font(.system(size: 13, weight: .medium)).foregroundStyle(Mint.muted)
        }
        .padding(.vertical, 14)
        .contentShape(Rectangle())
    }

    private func modelPicker(_ models: Models) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Model (admin)").font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.bottom, 8)
            if models.autoAvailable {
                HStack {
                    VStack(alignment: .leading, spacing: 1) {
                        Text("Auto").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                        Text(models.auto
                             ? (agentsOn ? "Jev picks an agent, which brings its model and thinking" : "Jev picks the model and thinking for each prompt")
                             : (agentsOn ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below"))
                            .font(.body(12)).foregroundStyle(Mint.muted)
                    }
                    Spacer()
                    Toggle("Auto", isOn: Binding(get: { models.auto }, set: { on in Task { await model.setAuto(on) } }))
                        .labelsHidden().tint(Mint.ok)
                }
                .padding(.bottom, 8)
                if models.auto && !agentsOn {
                    Text("Used when Jev cannot pick:").font(.body(12)).foregroundStyle(Mint.muted).padding(.bottom, 6)
                }
            }
            if !(models.autoAvailable && models.auto && agentsOn) {
                choicePicker(models)
            }
            Text(agentsOn ? "Applies to your own turns." : "Applies to your own turns. Everyone else stays on the default.")
                .font(.body(12)).foregroundStyle(Mint.muted).padding(.top, 6)
        }
    }

    private func choicePicker(_ models: Models) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(spacing: 6) {
                ForEach(models.choices, id: \.id) { choice in
                    let selected = choice.id == models.selected
                    Button { Task { await model.setModel(choice.id) } } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(choice.default ? "\(choice.name) (default)" : choice.name)
                                    .font(.body(14, .medium)).foregroundStyle(Mint.ink)
                                Text(modelDetail(choice)).font(.body(12)).foregroundStyle(Mint.muted)
                            }
                            Spacer()
                            if selected {
                                Image(systemName: "checkmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(Mint.ink)
                            }
                        }
                        .padding(.horizontal, 12).padding(.vertical, 9)
                        .overlay(RoundedRectangle(cornerRadius: 14)
                            .strokeBorder(selected ? Mint.ink : Mint.soft, lineWidth: selected ? 2 : 1))
                        .contentShape(RoundedRectangle(cornerRadius: 14))
                    }
                    .buttonStyle(.plain)
                }
            }
            Text("Thinking").font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.top, 12).padding(.bottom, 4)
            HStack(spacing: 6) {
                ForEach([ReasoningLevel.off, .low, .medium, .high], id: \.self) { level in
                    FilterChip(label: "\(level)".capitalized, selected: level == models.thinking) {
                        Task { await model.setThinking(level) }
                    }
                }
            }
        }
    }

    private func modelDetail(_ choice: ModelChoice) -> String {
        let price = { (usd: Double?) in usd.map { String(format: "$%.2f", $0) } ?? "?" }
        let prices = "\(price(choice.inputPrice)) in, \(price(choice.outputPrice)) out per M tokens"
        return choice.seesImages ? prices : prices + ", text only"
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
