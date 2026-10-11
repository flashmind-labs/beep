import SwiftUI

struct RoutinesView: View {
    @Environment(AppModel.self) private var model
    @Binding var path: [Route]
    @State private var crons: [Cron] = []
    @State private var loaded = false
    @State private var error = ""
    @State private var creating = false
    /// Opens the new routine sheet set to arrive at this place, from the places screen.
    var startAt: SavedPlace?
    /// The Always ask shows after the first place routine only; the places screen keeps a banner after that.
    @AppStorage("place_routine_asked") private var askedAlways = false

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Routines", back: { path.removeAll() }) {
                Button { creating = true } label: { Image(systemName: "plus") }
                    .buttonStyle(ChunkyButton(fill: Mint.accent))
                    .accessibilityLabel("New routine")
            }
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 20).padding(.vertical, 8)
            }
            if !loaded, crons.isEmpty {
                ProgressView().padding(40)
                Spacer()
            } else if crons.isEmpty {
                Text("No routines yet.\nSchedule tasks for Beep to run on a timer.")
                    .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                    .multilineTextAlignment(.center)
                    .padding(40)
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(crons, id: \.id) { cron in
                            RoutineCard(cron: cron, toggle: { set(cron, enabled: $0) },
                                        run: { run(cron) }, delete: { delete(cron) })
                        }
                        Text("Swipe right to run now, left to delete")
                            .font(.body(11)).foregroundStyle(Mint.dim)
                            .padding(.vertical, 6)
                    }
                    .padding(.horizontal, 16).padding(.vertical, 8)
                }
                .refreshable { await load() }
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await load() } }
        .sheet(isPresented: $creating) {
            NewRoutineView(places: model.places, place: startAt) { label, prompt, schedule in
                try await model.createCron(label: label, prompt: prompt, schedule: schedule)
                await load()
                guard PlaceTrigger(schedule: schedule) != nil else { return }
                await model.refreshPlaces()
                // Arriving only reaches Beep with the app closed when location is Always.
                if !model.location.always, !askedAlways {
                    askedAlways = true
                    try? await Task.sleep(for: .milliseconds(400))
                    path.append(.locationAlways)
                }
            }
        }
        .onAppear { if startAt != nil, !loaded { creating = true } }
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
        withAnimation(.snappy) { crons.removeAll { $0.id == cron.id } }
        Task {
            do { try await model.deleteCron(cron.id) } catch { self.error = describe(error) }
            await load()
        }
    }

    private func run(_ cron: Cron) {
        Task {
            do {
                try await model.runCron(cron.id)
                path = [.chat]
            } catch {
                self.error = describe(error)
            }
        }
    }
}

/// A routine card that runs when swiped right and is deleted when swiped left.
private struct RoutineCard: View {
    @Environment(AppModel.self) private var model
    let cron: Cron
    let toggle: (Bool) -> Void
    let run: () -> Void
    let delete: () -> Void
    @State private var offset: CGFloat = 0

    private static let threshold: CGFloat = 110

    var body: some View {
        let armed = abs(offset) > Self.threshold
        let toDelete = offset < 0
        ZStack {
            if offset != 0 {
                HStack(spacing: 8) {
                    if !toDelete { Image(systemName: "play.fill") }
                    Text(toDelete ? "Delete" : "Run now").font(.body(13, .semibold))
                    if toDelete { Image(systemName: "trash") }
                }
                .scaleEffect(armed ? 1.1 : 1)
                .foregroundStyle(Mint.ink)
                .padding(.horizontal, 22)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: toDelete ? .trailing : .leading)
                .background(RoundedRectangle(cornerRadius: 18)
                    .fill(!armed ? Mint.soft : toDelete ? Mint.error : Mint.accent))
            }
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(cron.label.isEmpty ? "Reminder" : cron.label)
                        .font(.body(13, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    Text(cron.prompt).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(2).lineSpacing(4)
                        .padding(.top, 3)
                    Text(describeSchedule(cron.schedule, places: model.places)).font(.body(11)).foregroundStyle(Mint.dim)
                        .padding(.top, 4)
                    if cron.enabled, parseInstant(cron.schedule) == nil,
                       let next = cron.nextRunAt.flatMap(parseInstant) {
                        Text("Next: " + describeDay(next)).font(.body(11)).foregroundStyle(Mint.dim)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Toggle("Enabled", isOn: Binding(get: { cron.enabled }, set: toggle))
                    .labelsHidden()
                    .tint(Mint.ok)
            }
            .padding(14)
            .chunky()
            .offset(x: offset)
        }
        .padding(.trailing, 3).padding(.bottom, 3)
        .sensoryFeedback(.impact, trigger: armed) { _, now in now }
        .gesture(DragGesture(minimumDistance: 20)
            .onChanged { drag in
                if abs(drag.translation.width) > abs(drag.translation.height) { offset = drag.translation.width }
            }
            .onEnded { _ in
                let done = abs(offset) > Self.threshold
                if done, offset < 0 {
                    delete()
                } else {
                    if done { run() }
                    withAnimation(.spring(duration: 0.3)) { offset = 0 }
                }
            })
    }
}

private enum Repeat: String, CaseIterable {
    case once = "Once", daily = "Daily", weekdays = "Weekdays", weekly = "Weekly", hourly = "Hourly"
}

/// The Android "New routine" dialog, as a sheet.
private struct NewRoutineView: View {
    @Environment(\.dismiss) private var dismiss
    let places: [SavedPlace]
    var place: SavedPlace?
    let create: (String, String, String) async throws -> Void
    @State private var when: PlaceTrigger.Kind?
    @State private var placeId = ""
    @State private var placeRepeat = PlaceTrigger.Repeat.daily
    @State private var label = ""
    @State private var prompt = ""
    @State private var repeatMode = Repeat.daily
    @State private var days: Set<Int> = [1]
    @State private var time = Calendar.current.date(bySettingHour: 9, minute: 0, second: 0, of: .now) ?? .now
    @State private var saving = false
    @State private var error = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("New routine").font(.display(22)).foregroundStyle(Mint.ink)
                field("Name", text: $label, placeholder: "Morning briefing")
                VStack(alignment: .leading, spacing: 6) {
                    Text("When").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                    HStack(spacing: 8) {
                        FilterChip(label: "At a time", selected: when == nil) { when = nil }
                        FilterChip(label: "I arrive", selected: when == .arrive) { when = .arrive }
                        FilterChip(label: "I leave", selected: when == .leave) { when = .leave }
                    }
                }
                if when != nil {
                    placeChoice
                } else {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(Repeat.allCases, id: \.self) { mode in
                                FilterChip(label: mode.rawValue, selected: repeatMode == mode) { repeatMode = mode }
                            }
                        }
                    }
                }
                if when == nil, repeatMode == .weekly {
                    HStack(spacing: 6) {
                        ForEach(cronDays, id: \.self) { day in
                            FilterChip(label: String(cronDayName(day).prefix(2)), selected: days.contains(day)) {
                                if days.contains(day) { days.remove(day) } else { days.insert(day) }
                            }
                        }
                    }
                }
                if when == nil { timePicker }
                field("What should Beep do?", text: $prompt, placeholder: "Give me a summary of...", lines: 2...4)
                if !error.isEmpty {
                    Text(error).font(.body(12)).foregroundStyle(Mint.error)
                }
                HStack(spacing: 8) {
                    Spacer()
                    Button("Cancel") { dismiss() }.buttonStyle(TextAction())
                    Button { save() } label: {
                        Text("Create").font(.body(15, .semibold)).foregroundStyle(Mint.ink)
                            .padding(.horizontal, 20).frame(minHeight: 44)
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
        .presentationDetents([.medium, .large])
        .presentationBackground(Mint.page)
        .presentationCornerRadius(28)
        .onAppear {
            if let place {
                when = .arrive
                placeId = place.id
            } else if let first = places.first {
                placeId = first.id
            }
        }
    }

    @ViewBuilder private var placeChoice: some View {
        if places.isEmpty {
            Text("No saved places yet. Tell Beep \"save this place as Gym\" while you're there.")
                .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(3)
        } else {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(places) { place in
                        FilterChip(label: place.name, selected: placeId == place.id) { placeId = place.id }
                    }
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("How often").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                HStack(spacing: 8) {
                    ForEach(PlaceTrigger.Repeat.allCases, id: \.self) { repeats in
                        FilterChip(label: repeats.title, selected: placeRepeat == repeats) { placeRepeat = repeats }
                    }
                }
                Text(placeRepeat.hint).font(.body(12)).foregroundStyle(Mint.muted)
            }
        }
    }

    @ViewBuilder private var timePicker: some View {
        HStack {
            Text(repeatMode == .once ? "When" : repeatMode == .hourly ? "Past the hour" : "At")
                .font(.body(14)).foregroundStyle(Mint.muted)
            Spacer()
            if repeatMode == .once {
                DatePicker("When", selection: $time, in: Date.now...).labelsHidden()
            } else if repeatMode == .hourly {
                Picker("Minute", selection: Binding(
                    get: { Calendar.current.component(.minute, from: time) },
                    set: { time = Calendar.current.date(bySettingHour: 9, minute: $0, second: 0, of: time) ?? time }
                )) {
                    ForEach(Array(stride(from: 0, to: 60, by: 5)), id: \.self) { Text(String(format: ":%02d", $0)).tag($0) }
                }
                .tint(Mint.ink)
            } else {
                DatePicker("Time", selection: $time, displayedComponents: .hourAndMinute).labelsHidden()
            }
        }
        .tint(Mint.brand)
    }

    private func field(_ title: String, text: Binding<String>, placeholder: String,
                       lines: ClosedRange<Int> = 1...1) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.body(12, .medium)).foregroundStyle(Mint.muted)
            TextField("", text: text, prompt: Text(placeholder).foregroundStyle(Mint.dim), axis: .vertical)
                .lineLimit(lines)
                .font(.body(15)).foregroundStyle(Mint.ink)
                .padding(.horizontal, 14).padding(.vertical, 12)
                .background(RoundedRectangle(cornerRadius: 12).fill(Mint.surface))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.soft, lineWidth: 1))
        }
    }

    private var valid: Bool {
        !label.trimmingCharacters(in: .whitespaces).isEmpty
            && !prompt.trimmingCharacters(in: .whitespaces).isEmpty
            && !schedule.isEmpty
    }

    private var schedule: String {
        if let when {
            return places.contains { $0.id == placeId } ? PlaceTrigger(kind: when, placeId: placeId, repeats: placeRepeat).schedule : ""
        }
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

func cronDayName(_ day: Int, full: Bool = false) -> String {
    let names = full ? Calendar.current.weekdaySymbols : Calendar.current.shortWeekdaySymbols
    return names[day % 7]
}

func clockTime(hour: Int, minute: Int) -> String {
    let date = Calendar.current.date(bySettingHour: hour, minute: minute, second: 0, of: .now) ?? .now
    return date.formatted(date: .omitted, time: .shortened)
}

func parseInstant(_ text: String) -> Date? {
    let trimmed = text.trimmingCharacters(in: .whitespaces)
    return (try? Date(trimmed, strategy: .iso8601))
        ?? (try? Date(trimmed, strategy: Date.ISO8601FormatStyle(includingFractionalSeconds: true)))
}

/// Reads the cron shapes the sheet writes as words, the same as Android. Anything else shows as written.
func describeSchedule(_ expression: String, places: [SavedPlace] = []) -> String {
    if let trigger = PlaceTrigger(schedule: expression) {
        let name = places.first { $0.id == trigger.placeId }?.name ?? "a saved place"
        return (trigger.kind == .arrive ? "When you arrive at " : "When you leave ") + name
            + (trigger.repeats.note.map { " · " + $0 } ?? "")
    }
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
func describeDay(_ at: Date) -> String {
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
