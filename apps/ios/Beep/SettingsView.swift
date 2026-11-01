import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    /// Auto picks only the thinking: the model below always applies,
    /// and with agents on the agent Jev picks brings its prompt and thinking.
    @State private var agents: Agents?
    @State private var agentsError = ""
    @State private var machinesAllowed = false

    private var agentsOn: Bool { agents?.enabled == true }

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Settings", back: { path.removeAll() })
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    LookPicker()
                    AvatarPicker()
                    TalkToBeepCard { path.removeAll(); model.live.start(from: .actionButton) }
                    if let models = model.models, !models.choices.isEmpty {
                        modelPicker(models)
                    }
                    if let agents, !agents.agents.isEmpty { agentsSwitch(agents) }
                    VStack(spacing: 0) {
                        divider
                        if let file = keyFile {
                            ShareLink(item: file) {
                                row("key", model.accountName.isEmpty ? "Account" : model.accountName,
                                    "Kept in your iPhone backups · tap to save it to a file")
                            }
                        }
                        divider
                        Button { path.append(.devices) } label: {
                            row("laptopcomputer", "Linked devices", "Use Beep from your computer")
                        }
                        divider
                        Button { path.append(.grants(nil)) } label: {
                            row("checkmark.shield", "Routine grants", "What routines may do without asking")
                        }
                        divider
                        // Nodes are for admins for now: the row shows once the server lists machines.
                        if machinesAllowed {
                            Button { path.append(.machines) } label: {
                                row("server.rack", "Machines", "Where Beep runs commands and coding agents")
                            }
                            divider
                        }
                        Button { if !model.connected { model.connect() } } label: {
                            row("server.rack", "Server", model.connected ? "Connected"
                                : (model.connectionError.isEmpty ? "Connecting" : model.connectionError),
                                dot: true)
                        }
                        divider
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 20).padding(.vertical, 8)
            }
        }
        .background(Mint.page)
        .task(id: model.models?.choices.isEmpty == false) {
            if model.models?.choices.isEmpty == false { agents = await model.agents() }
        }
        .task { machinesAllowed = (try? await model.machines()) != nil }
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
                             ? (agentsOn ? "Jev picks an agent, which brings its prompt and thinking, on the model below" : "Jev picks the thinking for each prompt, on the model below")
                             : (agentsOn ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below"))
                            .font(.body(12)).foregroundStyle(Mint.muted)
                    }
                    Spacer()
                    Toggle("Auto", isOn: Binding(get: { models.auto }, set: { on in Task { await model.setAuto(on) } }))
                        .labelsHidden().tint(Mint.ok)
                }
                .padding(.bottom, 8)
            }
            choicePicker(models)
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

    /// The server-wide agents switch, with the agents Jev picks from listed under it.
    private func agentsSwitch(_ current: Agents) -> some View {
        let choices = model.models?.choices ?? []
        return VStack(alignment: .leading, spacing: 0) {
            Text("Agents (admin)").font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.bottom, 8)
            HStack {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Agents").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                    Text(current.enabled ? "On for everyone: Jev picks an agent for each prompt"
                         : "Off: everyone uses the general assistant")
                        .font(.body(12)).foregroundStyle(Mint.muted)
                }
                Spacer()
                Toggle("Agents", isOn: Binding(get: { current.enabled }, set: { on in
                    agents?.enabled = on
                    agentsError = ""
                    Task {
                        do { agents = try await model.setAgents(on) } catch {
                            agents = current
                            agentsError = describe(error)
                        }
                    }
                }))
                .labelsHidden().tint(Mint.ok)
            }
            .padding(.bottom, 8)
            if !agentsError.isEmpty {
                Text(agentsError).font(.body(12)).foregroundStyle(Mint.error).padding(.bottom, 8)
            }
            VStack(alignment: .leading, spacing: 8) {
                ForEach(current.agents, id: \.id) { agent in
                    let name = choices.first { $0.id == agent.model }?.name
                        ?? agent.model.split(separator: "/").last.map(String.init) ?? agent.model
                    HStack(spacing: 8) {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(agent.name).font(.body(14, .medium)).foregroundStyle(Mint.ink)
                            Text(agent.when).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                        }
                        Spacer(minLength: 0)
                        Text("\(name), \(agent.thinking)").font(.body(12)).foregroundStyle(Mint.muted)
                    }
                }
            }
            .opacity(current.enabled ? 1 : 0.5)
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

/// Small previews of the looks. Tapping one switches the whole app.
private struct LookPicker: View {
    @State private var appearance = Appearance.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Look").font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.bottom, 8)
            VStack(spacing: 6) {
                ForEach(BeepStyle.allCases) { style in
                    let p = Palette.of(style)
                    let selected = style == appearance.style
                    Button { withAnimation(.snappy) { appearance.style = style } } label: {
                        HStack(spacing: 12) {
                            HStack(spacing: 4) {
                                Circle().fill(p.user).frame(width: 14, height: 14)
                                    .overlay(Circle().stroke(p.line, lineWidth: p.outline > 0 ? 1.5 : 0))
                                Circle().fill(p.accent).frame(width: 14, height: 14)
                                Circle().fill(p.ok.mix(with: p.page, by: 0.15)).frame(width: 14, height: 14)
                            }
                            Text(p.displayUppercase ? style.label.uppercased() : style.label)
                                .font(p.displayWeight.map { Font.custom(p.display, size: 14).weight($0) }
                                      ?? .custom(p.display, size: 14))
                                .foregroundStyle(p.ink).lineLimit(1)
                            Spacer(minLength: 0)
                            if selected {
                                Image(systemName: "checkmark").font(.system(size: 13, weight: .semibold)).foregroundStyle(p.ink)
                            }
                        }
                        .padding(.horizontal, 12).padding(.vertical, 11)
                        .background(RoundedRectangle(cornerRadius: 14).fill(p.page))
                        .overlay(RoundedRectangle(cornerRadius: 14)
                            .strokeBorder(selected ? Mint.ink : Mint.soft, lineWidth: selected ? 2 : 1))
                        .offset(x: selected ? 4 : 0)
                        .contentShape(RoundedRectangle(cornerRadius: 14))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

/// Beep's look on the home screen. The pager is the main icon; the others are alternates.
private struct AvatarPicker: View {
    private static let avatars: [(key: String, label: String, icon: String?)] = [
        ("pager", "Pager", nil), ("clay", "Clay", "AppIcon-Clay"), ("jelly", "Jelly", "AppIcon-Jelly"),
    ]
    @State private var current = UIApplication.shared.alternateIconName
    @State private var error = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Avatar").font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.bottom, 8)
            HStack(spacing: 10) {
                ForEach(Self.avatars, id: \.key) { avatar in
                    let selected = avatar.icon == current
                    Button { pick(avatar.icon) } label: {
                        VStack(spacing: 4) {
                            Image("avatar_\(avatar.key)").resizable().frame(width: 44, height: 44)
                                .clipShape(Circle())
                                .overlay(Circle().stroke(selected ? Mint.ink : .clear, lineWidth: 2))
                            Text(avatar.label).font(.body(11, selected ? .semibold : .regular))
                                .foregroundStyle(selected ? Mint.ink : Mint.muted)
                        }
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(selected ? .isSelected : [])
                }
            }
            Text(error.isEmpty ? "How Beep looks on your home screen." : error)
                .font(.body(11)).foregroundStyle(error.isEmpty ? Mint.muted : Mint.error).padding(.top, 6)
        }
    }

    private func pick(_ icon: String?) {
        guard icon != current, UIApplication.shared.supportsAlternateIcons else { return }
        error = ""
        UIApplication.shared.setAlternateIconName(icon) { failure in
            Task { @MainActor in
                if let failure { error = failure.localizedDescription } else { current = icon }
            }
        }
    }
}

/// How to start live voice from anywhere: the Action Button first, Back Tap without one, the
/// controls and widget, the side button in Japan; and whether a pause sends.
private struct TalkToBeepCard: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let tryIt: () -> Void

    var body: some View {
        @Bindable var live = model.live
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Image(systemName: "waveform").font(.system(size: 18, weight: .semibold)).foregroundStyle(Mint.ink)
                    .frame(width: 40, height: 40)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Mint.tints[1]))
                VStack(alignment: .leading, spacing: 1) {
                    Text("Talk to Beep").font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                    Text("Live voice from anywhere, without opening a chat first.")
                        .font(.body(13)).foregroundStyle(Mint.muted)
                }
            }
            VStack(alignment: .leading, spacing: 4) {
                Text("1. Open **Settings**, then **Action Button**.")
                Text("2. Swipe to **Shortcut** and pick **Beep**, **Talk to Beep**.")
            }
            .font(.body(13)).foregroundStyle(Mint.ink)
            Text("No Action Button? Use Back Tap: **Settings**, **Accessibility**, **Touch**, **Back Tap**, then **Double Tap** and **Talk to Beep**. Or add the Talk to Beep control to Control Center or the Lock Screen, or the widget to the Home Screen. In Japan you can also choose Beep for pressing and holding the side button.")
                .font(.body(12)).foregroundStyle(Mint.muted)
            HStack(spacing: 8) {
                Button("Open Settings") {
                    if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                }
                .font(.body(14, .medium)).foregroundStyle(Mint.ink)
                .padding(.horizontal, 14).frame(minHeight: 36)
                .background(Capsule().fill(Mint.user))
                Button("Try it", action: tryIt)
                    .font(.body(14, .medium)).foregroundStyle(Mint.muted)
                    .padding(.horizontal, 8).frame(minHeight: 36)
            }
            .buttonStyle(.plain)
            Rectangle().fill(Mint.soft).frame(height: 1)
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Send when I stop talking").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                    Text(live.sendOnPause ? "A pause sends. Say “send” to send at once." : "Only “send” or “stop” sends; pauses never do.")
                        .font(.body(12)).foregroundStyle(Mint.muted)
                }
                Spacer()
                Toggle("Send when I stop talking", isOn: $live.sendOnPause).labelsHidden().tint(Mint.ok)
            }
            Rectangle().fill(Mint.soft).frame(height: 1)
            VStack(alignment: .leading, spacing: 8) {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Voice speed").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                    Text("How fast Beep talks in live mode.").font(.body(12)).foregroundStyle(Mint.muted)
                }
                HStack(spacing: 6) {
                    ForEach([1.0, 1.25, 1.5, 2.0], id: \.self) { speed in
                        FilterChip(label: speed.formatted() + "×", selected: live.voiceSpeed == speed) {
                            live.voiceSpeed = speed
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
    }
}
