import SwiftUI

struct RoutinesView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var crons: [Cron] = []
    @State private var loaded = false
    @State private var error = ""
    @State private var creating = false

    var body: some View {
        List {
            if !error.isEmpty {
                Text(error).foregroundStyle(Mint.error)
            }
            ForEach(crons, id: \.id) { cron in
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(cron.label.isEmpty ? "Reminder" : cron.label).font(.body(16, .medium)).foregroundStyle(Mint.ink)
                        Text(describeSchedule(cron.schedule)).font(.body(13)).foregroundStyle(Mint.muted)
                        if cron.enabled, let next = cron.nextRunAt.flatMap(parseInstant) {
                            Text("Next: " + describeDay(next)).font(.body(12)).foregroundStyle(Mint.dim)
                        }
                    }
                    Spacer()
                    Toggle("Enabled", isOn: Binding(get: { cron.enabled }, set: { set(cron, enabled: $0) }))
                        .labelsHidden()
                        .tint(Mint.brand)
                }
                .swipeActions(edge: .leading) {
                    Button { run(cron) } label: { Label("Run now", systemImage: "play") }.tint(Mint.brand)
                }
                .swipeActions(edge: .trailing) {
                    Button(role: .destructive) { delete(cron) } label: { Label("Delete", systemImage: "trash") }
                }
            }
        }
        .font(.body(15))
        .scrollContentBackground(.hidden)
        .background(Mint.page)
        .overlay {
            if loaded, crons.isEmpty, error.isEmpty {
                ContentUnavailableView("No routines", systemImage: "clock",
                    description: Text("Beep can run a prompt for you on a schedule."))
            }
        }
        .navigationTitle("Routines")
        .toolbar {
            Button { creating = true } label: { Image(systemName: "plus") }
                .accessibilityLabel("New routine")
        }
        .task(id: model.connected) { if model.connected { await load() } }
        .refreshable { await load() }
        .sheet(isPresented: $creating) {
            NewRoutineView { label, prompt, schedule in
                try await model.createCron(label: label, prompt: prompt, schedule: schedule)
                await load()
            }
        }
    }

    private func load() async {
        do {
            crons = try await model.crons()
            error = ""
        } catch {
            self.error = describe(error)
        }
        loaded = true
    }

    private func set(_ cron: Cron, enabled: Bool) {
        Task {
            do { try await model.setCron(cron.id, enabled: enabled) } catch { self.error = describe(error) }
            await load()
        }
    }

    private func delete(_ cron: Cron) {
        crons.removeAll { $0.id == cron.id }
        Task {
            do { try await model.deleteCron(cron.id) } catch { self.error = describe(error) }
            await load()
        }
    }

    private func run(_ cron: Cron) {
        Task {
            do {
                try await model.runCron(cron.id)
                path.removeAll()
            } catch {
                self.error = describe(error)
            }
        }
    }
}

private enum Repeat: String, CaseIterable {
    case once = "Once", daily = "Daily", weekdays = "Weekdays", weekly = "Days", hourly = "Hourly"
}

private struct NewRoutineView: View {
    @Environment(\.dismiss) private var dismiss
    let create: (String, String, String) async throws -> Void
    @State private var label = ""
    @State private var prompt = ""
    @State private var repeatMode = Repeat.daily
    @State private var days: Set<Int> = [1]
    @State private var time = Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now) ?? .now
    @State private var saving = false
    @State private var error = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $label)
                    TextField("What should Beep do?", text: $prompt, axis: .vertical).lineLimit(3...8)
                }
                Section("Repeat") {
                    Picker("Repeat", selection: $repeatMode) {
                        ForEach(Repeat.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    if repeatMode == .weekly {
                        HStack {
                            ForEach(cronDays, id: \.self) { day in
                                let on = days.contains(day)
                                Button(String(cronDayName(day).prefix(2))) {
                                    if on { days.remove(day) } else { days.insert(day) }
                                }
                                .buttonStyle(PillButton(fill: on ? Mint.user : Mint.surface))
                            }
                        }
                    }
                    if repeatMode == .once {
                        DatePicker("When", selection: $time, in: Date.now...)
                    } else if repeatMode == .hourly {
                        Picker("Minute", selection: Binding(
                            get: { Calendar.current.component(.minute, from: time) },
                            set: { time = Calendar.current.date(bySettingHour: 9, minute: $0, second: 0, of: time) ?? time }
                        )) {
                            ForEach(Array(stride(from: 0, to: 60, by: 5)), id: \.self) { Text(String(format: ":%02d", $0)).tag($0) }
                        }
                    } else {
                        DatePicker("Time", selection: $time, displayedComponents: .hourAndMinute)
                    }
                }
                if !error.isEmpty {
                    Text(error).foregroundStyle(Mint.error)
                }
            }
            .font(.body(15))
            .navigationTitle("New routine")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") { save() }.disabled(!valid || saving)
                }
            }
        }
        .presentationDetents([.large])
    }

    private var valid: Bool {
        !label.trimmingCharacters(in: .whitespaces).isEmpty
            && !prompt.trimmingCharacters(in: .whitespaces).isEmpty
            && !schedule.isEmpty
    }

    private var schedule: String {
        let parts = Calendar.current.dateComponents([.hour, .minute], from: time)
        let hour = parts.hour ?? 9
        let minute = parts.minute ?? 0
        switch repeatMode {
        case .once: return ISO8601DateFormatter.string(from: time, timeZone: .current, formatOptions: .withInternetDateTime)
        case .daily: return "\(minute) \(hour) * * *"
        case .weekdays: return "\(minute) \(hour) * * 1-5"
        case .weekly:
            let chosen = cronDays.filter(days.contains)
            return chosen.isEmpty ? "" : "\(minute) \(hour) * * " + chosen.map(String.init).joined(separator: ",")
        case .hourly: return "\(minute) * * * *"
        }
    }

    private func save() {
        saving = true
        error = ""
        Task {
            defer { saving = false }
            do {
                try await create(label.trimmingCharacters(in: .whitespaces), prompt.trimmingCharacters(in: .whitespaces), schedule)
                dismiss()
            } catch {
                self.error = describe(error)
            }
        }
    }
}

/// Cron weekday numbers, Sunday being 0, in the order the picker shows them.
private let cronDays = [1, 2, 3, 4, 5, 6, 0]

private func cronDayName(_ day: Int, full: Bool = false) -> String {
    let names = full ? Calendar.current.weekdaySymbols : Calendar.current.shortWeekdaySymbols
    return names[day % 7]
}

private func clockTime(hour: Int, minute: Int) -> String {
    let date = Calendar.current.date(bySettingHour: hour, minute: minute, second: 0, of: .now) ?? .now
    return date.formatted(date: .omitted, time: .shortened)
}

private func parseInstant(_ text: String) -> Date? {
    let plain = ISO8601DateFormatter()
    let fractional = ISO8601DateFormatter()
    fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    let trimmed = text.trimmingCharacters(in: .whitespaces)
    return plain.date(from: trimmed) ?? fractional.date(from: trimmed)
}

/// Reads the cron shapes the sheet writes as words, the same as Android. Anything else shows as written.
private func describeSchedule(_ expression: String) -> String {
    if let once = parseInstant(expression) { return "Once, " + describeDay(once) }
    let parts = expression.split(separator: " ").map(String.init)
    guard parts.count == 5, parts[2] == "*", parts[3] == "*",
        let minute = Int(parts[0]), (0...59).contains(minute)
    else { return expression }
    let days = parts[4]
    if parts[1] == "*", days == "*" {
        return minute == 0 ? "Every hour" : String(format: "Every hour at :%02d", minute)
    }
    guard let hour = Int(parts[1]), (0...23).contains(hour) else { return expression }
    let at = clockTime(hour: hour, minute: minute)
    if days == "*" { return "Every day at \(at)" }
    if days == "1-5" { return "Weekdays at \(at)" }
    if let day = Int(days), (0...6).contains(day) { return "Every \(cronDayName(day, full: true)) at \(at)" }
    let list = days.split(separator: ",").compactMap { Int($0) }.filter { (0...6).contains($0) }
    if list.count == days.split(separator: ",").count {
        return list.map { cronDayName($0) }.joined(separator: ", ") + " at \(at)"
    }
    return expression
}

/// "today 9:00", "tomorrow 9:00", "Mon 9:00" or, further out, "12 Oct 9:00".
private func describeDay(_ at: Date) -> String {
    let calendar = Calendar.current
    let today = calendar.startOfDay(for: .now)
    let day = calendar.startOfDay(for: at)
    let ahead = calendar.dateComponents([.day], from: today, to: day).day ?? 0
    let time = at.formatted(date: .omitted, time: .shortened)
    let label: String
    switch ahead {
    case 0: label = "today"
    case 1: label = "tomorrow"
    case 2..<7: label = cronDayName(calendar.component(.weekday, from: at) - 1)
    default: label = at.formatted(.dateTime.day().month(.abbreviated))
    }
    return "\(label) \(time)"
}
