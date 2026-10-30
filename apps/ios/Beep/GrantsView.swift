import SwiftUI

/// What routines may do without asking. Each grant came from an approval card the user allowed
/// once, and ends on its own; Revoke ends it now.
struct GrantsView: View {
    @Environment(AppModel.self) private var model
    /// Only this routine's grants, from its card; every routine's from Settings.
    var routine: String?
    let back: () -> Void
    @State private var grants: [Grant] = []
    @State private var loaded = false
    @State private var revoking: Grant?
    @State private var error = ""

    private var shown: [Grant] { routine.map { id in grants.filter { $0.routineId == id } } ?? grants }

    var body: some View {
        VStack(spacing: 0) {
            BackBar(title: "Routine grants", back: back)
            if !error.isEmpty {
                Text(error).font(.body(12)).foregroundStyle(Mint.error)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 20).padding(.vertical, 8)
            }
            if !loaded, shown.isEmpty {
                ProgressView().padding(40)
                Spacer()
            } else if shown.isEmpty {
                Text("No grants.\nWhen you set up a routine, Beep can ask to let it act without asking each time.")
                    .font(.body(13)).foregroundStyle(Mint.muted).lineSpacing(5)
                    .multilineTextAlignment(.center)
                    .padding(40)
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(shown, id: \.id) { grant in
                            GrantCard(grant: grant) { revoking = grant }
                        }
                        Text("Grants end on their own, after 8 hours unless set otherwise, and never last more than a week.")
                            .font(.body(11)).foregroundStyle(Mint.dim).multilineTextAlignment(.center)
                            .padding(.vertical, 6)
                    }
                    .padding(.horizontal, 16).padding(.vertical, 8)
                }
                .refreshable { await load() }
            }
        }
        .background(Mint.page)
        .task(id: model.connected) { if model.connected { await load() } }
        .alert("Revoke this grant?", isPresented: Binding(get: { revoking != nil }, set: { if !$0 { revoking = nil } }),
               presenting: revoking) { grant in
            Button("Revoke", role: .destructive) { Task { await revoke(grant) } }
            Button("Cancel", role: .cancel) {}
        } message: { grant in
            Text("\(grant.routine) will ask again before it does this.")
        }
    }

    private func load() async {
        do {
            grants = try await model.grants()
            error = ""
        } catch {
            self.error = describe(error)
        }
        loaded = true
    }

    private func revoke(_ grant: Grant) async {
        do {
            try await model.revokeGrant(grant.id)
            withAnimation(.snappy) { grants.removeAll { $0.id == grant.id } }
        } catch {
            self.error = describe(error)
        }
        await load()
    }
}

/// One grant: the routine, what each run may do, when the grant ends and when it was last used.
private struct GrantCard: View {
    let grant: Grant
    let revoke: () -> Void

    /// The card's fields, less the routine (the title) and the end date as it read when
    /// approved (shown relative instead).
    private var fields: [ApprovalField] { grant.fields.filter { $0.label != "Routine" && $0.label != "Ends" } }

    /// "Reply to thread with Gmail".
    private var allows: String {
        let action = grant.action.prefix(1).uppercased() + String(grant.action.dropFirst())
        return "\(action) with \(grant.app)"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                Text(String(grant.app.prefix(1)).uppercased())
                    .font(.body(14, .semibold))
                    .frame(width: 32, height: 32)
                    .background(RoundedRectangle(cornerRadius: 9).fill(Mint.tints[2]))
                    .overlay(RoundedRectangle(cornerRadius: 9).stroke(Mint.ledge, lineWidth: 2))
                VStack(alignment: .leading, spacing: 2) {
                    Text(grant.routine).font(.body(14, .semibold)).lineLimit(2)
                    Text(allows)
                        .font(.body(12)).foregroundStyle(Mint.muted)
                }
                Spacer(minLength: 0)
            }
            if !fields.isEmpty {
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 4) {
                    ForEach(Array(fields.enumerated()), id: \.offset) { _, field in
                        GridRow {
                            Text(field.label).foregroundStyle(Mint.muted)
                            Text(field.value).lineLimit(4)
                        }
                    }
                }
                .font(.body(13))
                .padding(.top, 10)
            }
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(grantEnds(grant.expiresAt)).font(.body(12, .medium))
                    Text(grant.lastUsedAt.map { "Last used " + $0.formatted(.relative(presentation: .named)) }
                         ?? "Not used yet")
                        .font(.body(11)).foregroundStyle(Mint.dim)
                }
                Spacer(minLength: 0)
                Button("Revoke", action: revoke).buttonStyle(TextAction(color: Mint.error))
            }
            .padding(.top, 10)
        }
        .foregroundStyle(Mint.ink)
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
    }
}

/// When a grant ends, counted from now: "ends in 40 min", "ends in 5 h", "ends in 3 days".
func grantEnds(_ at: Date, now: Date = .now) -> String {
    let minutes = Int(at.timeIntervalSince(now) / 60)
    if minutes <= 0 { return "Ended" }
    if minutes < 60 { return "Ends in \(minutes) min" }
    let hours = (minutes + 30) / 60
    if hours < 48 { return "Ends in \(hours) h" }
    return "Ends in \((hours + 12) / 24) days"
}
