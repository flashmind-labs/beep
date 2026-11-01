import SwiftUI

/// What the event triage agent found in the user's apps, on its own card above an empty chat's
/// greeting rather than inside a reply. Three show; the rest fold behind "N more".
struct UpdatesCard: View {
    let updates: [AppUpdate]
    let open: (AppUpdate) -> Void
    let dismiss: ([Int64]) -> Void
    @State private var expanded = false

    private static let shown = 3

    var body: some View {
        let rows = expanded ? updates : Array(updates.prefix(Self.shown))
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Updates").font(.body(13, .semibold)).foregroundStyle(Mint.ink)
                Spacer(minLength: 0)
                Button("Clear") { dismiss(updates.map(\.id)) }
                    .buttonStyle(TextAction(color: Mint.muted))
                    .font(.body(12))
            }
            .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 2)
            ForEach(Array(rows.enumerated()), id: \.element.id) { index, update in
                Divider().overlay(Mint.soft)
                row(update, index: index)
            }
            if !expanded && updates.count > Self.shown {
                Divider().overlay(Mint.soft)
                Button { expanded = true } label: {
                    Text("\(updates.count - Self.shown) more").font(.body(12, .medium)).foregroundStyle(Mint.muted)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 12).padding(.vertical, 10)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .chunky(Mint.tints[1 % Mint.tints.count])
    }

    private func row(_ update: AppUpdate, index: Int) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Button { open(update) } label: {
                HStack(alignment: .top, spacing: 10) {
                    Text(update.appName.prefix(1).uppercased())
                        .font(.body(11, .semibold)).foregroundStyle(Mint.ink)
                        .frame(width: 26, height: 26)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Mint.tints[index % Mint.tints.count]))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(update.summary).font(.body(13)).foregroundStyle(Mint.ink)
                            .multilineTextAlignment(.leading)
                        Text(update.appName.isEmpty ? updateTime(update.at) : "\(update.appName) · \(updateTime(update.at))")
                            .font(.body(11.5)).foregroundStyle(Mint.muted)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Starts a chat about this")
            Button { dismiss([update.id]) } label: {
                Image(systemName: "xmark").font(.system(size: 12, weight: .medium)).foregroundStyle(Mint.muted)
                    .frame(width: 32, height: 32)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss")
        }
        .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 8)
    }
}

/// "09:41" today, then "Yesterday", "Thu" this week, "3 Oct".
private func updateTime(_ date: Date) -> String {
    let calendar = Calendar.current
    if calendar.isDateInToday(date) { return date.formatted(date: .omitted, time: .shortened) }
    if calendar.isDateInYesterday(date) { return "Yesterday" }
    return shortDay(date)
}
