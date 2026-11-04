import SwiftUI

/// How the user, or one of their groups, likes a recurring task done. Beep writes these
/// itself; here they can be read, paused, edited, undone or deleted.
struct SkillsView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var loaded = false
    @State private var error = ""

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Skills", back: { path.removeAll() })
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, Mint.pageInset)
            }
            if !loaded, model.skills.isEmpty {
                ProgressView().padding(40)
                Spacer()
            } else if model.skills.isEmpty {
                Text("No skills yet.\nWhen Beep has to work out how you like something done, like booking a court "
                     + "or a weekly summary, it writes it down here and does it your way next time.")
                    .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                    .multilineTextAlignment(.center)
                    .padding(40)
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        Text("Beep writes these from how you ask for things, and updates them when you correct it.")
                            .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(3)
                            .padding(.horizontal, 4)
                        ForEach(sections, id: \.name) { section in
                            Text(section.name).font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                                .padding(.horizontal, 4).padding(.top, 8)
                            ForEach(section.skills, id: \.id) { skill in
                                SkillCard(skill: skill) { path.append(.skill(skill.id)) }
                                    .padding(.trailing, 3).padding(.bottom, 3)
                            }
                        }
                    }
                    .padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
                }
                .refreshable { await load() }
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await load() } }
    }

    private struct Section {
        var name: String
        var skills: [Skill]
    }

    /// The user's own first, then one section per group.
    private var sections: [Section] {
        let own = model.skills.filter { $0.groupId == nil }
        let groups = Dictionary(grouping: model.skills.filter { $0.groupId != nil }) { $0.groupName ?? "Group" }
            .map { Section(name: $0.key, skills: $0.value) }
            .sorted { $0.name.localizedCompare($1.name) == .orderedAscending }
        return (own.isEmpty ? [] : [Section(name: "Yours", skills: own)]) + groups
    }

    private func load() async {
        do {
            try await model.refreshSkills()
            error = ""
        } catch {
            self.error = describe(error)
        }
        loaded = true
    }
}

private struct SkillCard: View {
    let skill: Skill
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(skill.name).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                    Text(skill.description).font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3)
                    Text((skill.enabled ? usage(skill) : "Paused") + (skill.edited ? " · edited by you" : ""))
                        .font(.body(12)).foregroundStyle(Mint.dim).padding(.top, 2)
                }
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                Image(systemName: "chevron.right").font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Mint.muted).padding(.top, 2)
            }
            .padding(Mint.spacing)
            .card()
            .opacity(skill.enabled ? 1 : 0.65)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// One skill: whether Beep uses it, its steps, and who changed it last.
struct SkillView: View {
    @Environment(AppModel.self) private var model
    let id: String
    let back: () -> Void
    @State private var error = ""
    @State private var editing = false
    @State private var confirmDelete = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                BackArrow(action: back)
                Text(skill?.name ?? "Skill").font(.display(18)).foregroundStyle(Mint.ink)
                    .lineLimit(1).truncationMode(.tail)
                Spacer()
                if skill != nil {
                    iconButton("pencil", "Edit", color: Mint.ink) { editing = true }
                    iconButton("trash", "Delete", color: Mint.error) { confirmDelete = true }
                }
            }
            .padding(.horizontal, 6).frame(minHeight: 56)
            if let skill {
                ScrollView {
                    details(skill).padding(.horizontal, Mint.pageInset).padding(.vertical, 8)
                }
            } else {
                ProgressView().padding(40)
                Spacer()
            }
        }
        .background(Mint.page)
        .task(id: model.connected) {
            if model.connected, skill == nil { try? await model.refreshSkills() }
        }
        .sheet(isPresented: $editing) {
            if let skill { EditSkillSheet(skill: skill) }
        }
        .alert("Delete this skill?", isPresented: $confirmDelete) {
            Button("Cancel", role: .cancel) {}
            Button("Delete", role: .destructive) { delete() }
        } message: {
            Text("Beep forgets how you like \"\(skill?.name ?? "it")\" done. It may learn it again if you keep asking for it.")
        }
    }

    private var skill: Skill? { model.skills.first { $0.id == id } }

    @ViewBuilder private func details(_ skill: Skill) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if let group = skill.groupName {
                Text(group).font(.body(11, .semibold)).foregroundStyle(Mint.ink)
                    .padding(.horizontal, 8).padding(.vertical, 2)
                    .background(Capsule().fill(Mint.tints[2]))
                    .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1.5))
            }
            Text(skill.description).font(.body(14)).foregroundStyle(Mint.muted).lineSpacing(3)
            Toggle(isOn: Binding(get: { skill.enabled }, set: { setEnabled($0) })) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Use this skill").font(.body(14, .medium)).foregroundStyle(Mint.ink)
                    Text(skill.enabled ? usage(skill) : "Paused: Beep works it out each time")
                        .font(.body(12)).foregroundStyle(Mint.muted)
                }
            }
            .tint(Mint.ok)
            .padding(Mint.spacing)
            .card()
            .padding(.trailing, 3).padding(.bottom, 3)
            VStack(alignment: .leading, spacing: 8) {
                if !skill.body.hasPrefix("#") {
                    Text("Steps").font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                }
                MarkdownText(text: skill.body, size: 14)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Mint.spacing)
            .card()
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
            }
            HStack(spacing: 10) {
                Image(systemName: "wand.and.stars").font(.system(size: 16, weight: .semibold)).foregroundStyle(Mint.ink)
                Text(changed(skill)).font(.body(13)).foregroundStyle(Mint.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if skill.canUndo {
                    Button("Undo") { undo() }.buttonStyle(TextAction())
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .frame(minHeight: 44)
            .card(Mint.tints[1], radius: 14)
        }
    }

    private func iconButton(_ icon: String, _ label: String, color: Color, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 18, weight: .semibold))
                .foregroundStyle(color).frame(width: 44, height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }

    private func setEnabled(_ on: Bool) {
        Task {
            do {
                try await model.updateSkill(id, enabled: on)
                error = ""
            } catch {
                self.error = describe(error)
            }
        }
    }

    private func undo() {
        Task {
            do {
                try await model.undoSkill(id)
                error = ""
            } catch {
                self.error = describe(error)
            }
        }
    }

    private func delete() {
        Task {
            do {
                try await model.deleteSkill(id)
                back()
            } catch {
                self.error = describe(error)
            }
        }
    }
}

/// The mock's "Edit skill" dialog, as a sheet like the new routine one.
private struct EditSkillSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let skill: Skill
    @State private var name = ""
    @State private var description = ""
    @State private var instructions = ""
    @State private var saving = false
    @State private var error = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("Edit skill").font(.display(18)).foregroundStyle(Mint.ink)
                field("Name", text: $name)
                field("When it applies", text: $description, lines: 1...3)
                field("Instructions", text: $instructions, lines: 7...16, size: 13)
                if !error.isEmpty {
                    Text(error).font(.body(12)).foregroundStyle(Mint.error)
                }
                HStack(spacing: 8) {
                    Spacer()
                    Button("Cancel") { dismiss() }.buttonStyle(TextAction())
                    Button { save() } label: {
                        Text("Save").font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                            .padding(.horizontal, Mint.pageInset).frame(minHeight: 44)
                            .chunky(Mint.accent, radius: 18)
                    }
                    .buttonStyle(.plain)
                    .disabled(!valid || saving)
                    .opacity(valid ? 1 : 0.5)
                }
                .padding(.top, 4)
            }
            .padding(24)
        }
        .background(Mint.page)
        .presentationDetents([.large])
        .presentationBackground(Mint.page)
        .presentationCornerRadius(28)
        .onAppear {
            name = skill.name
            description = skill.description
            instructions = skill.body
        }
    }

    private func field(_ title: String, text: Binding<String>, lines: ClosedRange<Int> = 1...1,
                       size: CGFloat = 15) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.body(12, .medium)).foregroundStyle(Mint.muted)
            TextField("", text: text, axis: .vertical)
                .lineLimit(lines)
                .font(.body(size)).foregroundStyle(Mint.ink)
                .padding(.horizontal, 14).padding(.vertical, 12)
                .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.surface))
                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.soft, lineWidth: 1))
        }
    }

    private func trim(_ text: String) -> String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    private var valid: Bool { !trim(name).isEmpty && !trim(description).isEmpty && !trim(instructions).isEmpty }

    /// Sends only what changed.
    private func save() {
        let newName = trim(name), newDescription = trim(description), newBody = trim(instructions)
        guard newName != skill.name || newDescription != skill.description || newBody != skill.body else {
            dismiss()
            return
        }
        saving = true
        error = ""
        Task {
            defer { saving = false }
            do {
                try await model.updateSkill(skill.id,
                                            name: newName == skill.name ? nil : newName,
                                            description: newDescription == skill.description ? nil : newDescription,
                                            body: newBody == skill.body ? nil : newBody)
                dismiss()
            } catch BeepError.Conflict {
                error = "You already have a skill with that name"
            } catch {
                self.error = describe(error)
            }
        }
    }
}

/// "Used yesterday · 6 uses", or "Not used yet".
private func usage(_ skill: Skill) -> String {
    guard skill.uses > 0 else { return "Not used yet" }
    let count = skill.uses == 1 ? "1 use" : "\(skill.uses) uses"
    guard let last = skill.lastUsed.flatMap(parseInstant) else { return count }
    return "Used \(relativeDay(last)) · \(count)"
}

/// "Beep updated this yesterday." or "You edited this on 9 Oct."
private func changed(_ skill: Skill) -> String {
    let who = skill.edited ? "You edited this" : "Beep updated this"
    guard let at = parseInstant(skill.updatedAt) else { return who + "." }
    let day = relativeDay(at)
    let calendar = Calendar.current
    let near = calendar.isDateInToday(at) || calendar.isDateInYesterday(at)
    return "\(who) \(near ? day : "on " + day)."
}

/// "today", "yesterday", a weekday within the last week, else "2 Oct".
private func relativeDay(_ at: Date) -> String {
    let calendar = Calendar.current
    if calendar.isDateInToday(at) { return "today" }
    if calendar.isDateInYesterday(at) { return "yesterday" }
    let ago = calendar.dateComponents([.day], from: calendar.startOfDay(for: at), to: calendar.startOfDay(for: .now)).day ?? 0
    if ago > 0, ago < 7 { return at.formatted(.dateTime.weekday(.wide)) }
    let sameYear = calendar.isDate(at, equalTo: .now, toGranularity: .year)
    return sameYear ? at.formatted(.dateTime.day().month(.abbreviated))
        : at.formatted(.dateTime.day().month(.abbreviated).year())
}
