import SwiftUI

struct WorkspacePage<Content: View, Actions: View>: View {
    @Environment(\.tabRoot) private var tabRoot
    let title: String
    let back: () -> Void
    var showsBack = true
    @ViewBuilder var actions: Actions
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            CCTop(title: title, back: showsBack && !tabRoot ? back : nil) { actions }
            content
        }
        .background(Mint.page).foregroundStyle(Mint.ink)
        .toolbar(.hidden, for: .navigationBar)
    }
}

extension WorkspacePage where Actions == EmptyView {
    init(_ title: String, back: @escaping () -> Void, @ViewBuilder content: () -> Content) {
        self.init(title: title, back: back, actions: { EmptyView() }, content: content)
    }
}

struct WorkspaceRow: View {
    let icon: String
    let title: String
    var detail = ""
    var badge = ""
    var color: Color = Mint.muted
    var action: (() -> Void)?

    var body: some View {
        Button { action?() } label: {
            HStack(spacing: 10) {
                Image(systemName: icon).font(.system(size: 12)).foregroundStyle(Mint.muted)
                    .frame(width: 24, height: 24).background(Mint.raised, in: RoundedRectangle(cornerRadius: 6))
                VStack(alignment: .leading, spacing: 0) {
                    Text(title).font(.body(13, .medium)).foregroundStyle(Mint.ink).lineLimit(1)
                    if !detail.isEmpty { Text(detail).font(.body(11.5)).foregroundStyle(Mint.muted).lineLimit(1) }
                }
                Spacer(minLength: 4)
                if !badge.isEmpty { WorkspaceBadge(text: badge, color: color) }
                if action != nil { Image(systemName: "chevron.right").font(.system(size: 12)).foregroundStyle(Mint.dim) }
            }
            .padding(.horizontal, 11).padding(.vertical, 9).frame(maxWidth: .infinity, minHeight: 46, alignment: .leading)
            .contentShape(Rectangle()).multilineTextAlignment(.leading)
        }
        .buttonStyle(.plain).disabled(action == nil)
    }
}

struct WorkspaceBadge: View {
    let text: String
    var color: Color = Mint.muted
    var body: some View {
        Text(text).font(.body(11)).foregroundStyle(color).lineLimit(1)
            .padding(.horizontal, 7).frame(height: 20)
            .background(color == Mint.muted ? Mint.raised : color.opacity(0.14), in: Capsule())
    }
}

struct WorkspaceBox<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        VStack(spacing: 0) { content }.frame(maxWidth: .infinity, alignment: .leading)
            .background(Mint.surface).clipShape(RoundedRectangle(cornerRadius: Mint.radius))
            .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.soft, lineWidth: 1))
    }
}

struct WorkspaceHeading: View {
    let title: String
    var body: some View { Text(title).font(.body(12, .semibold)).foregroundStyle(Mint.muted).padding(.horizontal, 2).frame(maxWidth: .infinity, alignment: .leading) }
}

struct WorkspaceSearch: View {
    let prompt: String
    @Binding var text: String
    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(Mint.muted)
            TextField(prompt, text: $text).textInputAutocapitalization(.never).autocorrectionDisabled()
            if !text.isEmpty {
                Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(Mint.dim) }
                    .accessibilityLabel("Clear filter")
            }
        }
        .font(.body(13)).padding(12).card()
    }
}

struct WorkspaceError: View {
    let text: String
    var retry: (() async -> Void)?
    var body: some View {
        if !text.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text(text).font(.body(12)).foregroundStyle(Mint.error).textSelection(.enabled)
                if let retry { Button("Retry") { Task { await retry() } }.buttonStyle(TextAction()) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct WorkspaceEmpty: View {
    let title: String
    let detail: String
    var body: some View {
        VStack(spacing: 8) {
            Text(title).font(.body(14, .medium)).foregroundStyle(Mint.ink)
            Text(detail).font(.body(12)).foregroundStyle(Mint.muted)
        }.multilineTextAlignment(.center).frame(maxWidth: .infinity).padding(.vertical, 32)
    }
}

struct WorkspaceLoading: View {
    var body: some View {
        VStack(spacing: 12) {
            ForEach(0..<3) { _ in
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 6).fill(Mint.raised).frame(width: 28, height: 28)
                    VStack(alignment: .leading, spacing: 6) {
                        RoundedRectangle(cornerRadius: 3).fill(Mint.raised).frame(height: 12)
                        RoundedRectangle(cornerRadius: 3).fill(Mint.raised).frame(width: 140, height: 10)
                    }
                }.padding(12).card()
            }
        }.accessibilityLabel("Loading")
    }
}

struct WorkspaceFacts: View {
    let rows: [(String, String)]
    var body: some View {
        WorkspaceBox {
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 0) {
                ForEach(rows.indices, id: \.self) { index in
                    VStack(alignment: .leading, spacing: 3) {
                        Text(rows[index].0).font(.body(11)).foregroundStyle(Mint.muted)
                        Text(rows[index].1).font(.body(13, .medium)).textSelection(.enabled)
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(12)
                }
            }
        }
    }
}

extension View {
    func workspaceField() -> some View {
        font(.body(13)).padding(10).frame(minHeight: 40).card()
            .textInputAutocapitalization(.never).autocorrectionDisabled()
    }
}

func agentName(_ slug: String) -> String {
    switch slug { case "claude": "Claude Code"; case "codex": "Codex"; case "opencode": "OpenCode"; default: slug.capitalized }
}

extension AgentStatus {
    var label: String {
        switch self {
        case .starting: "Starting"; case .working: "Working"; case .asking: "Needs input"
        case .idle: "Idle"; case .ended: "Ended"; case .waiting: "Waiting"; case .running: "Running"
        }
    }
    var color: Color {
        switch self { case .asking, .waiting: Mint.warn; case .working, .running, .starting: Mint.brand; default: Mint.muted }
    }
}

extension RunState {
    var label: String {
        switch self {
        case .queued: "Queued"; case .waiting: "Waiting"; case .running: "Running"; case .passed: "Passed"
        case .failed: "Failed"; case .cancelled: "Cancelled"; case .skipped: "Skipped"; case .unknown: "Unknown"
        }
    }
    var color: Color {
        switch self { case .passed: Mint.ok; case .failed: Mint.error; case .running: Mint.brand; case .waiting: Mint.warn; default: Mint.muted }
    }
}
