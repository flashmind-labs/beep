import SwiftUI

/// The web pages a reply used, as a wrapping row of chips under it. More than three show
/// the first three and a "+N" chip that shows the rest.
struct SourceChips: View {
    let sources: [WebSource]
    let open: (URL) -> Void
    @State private var expanded = false

    private static let folded = 3

    var body: some View {
        let shown = expanded || sources.count <= Self.folded ? sources : Array(sources.prefix(Self.folded))
        FlowLayout(spacing: 6) {
            ForEach(Array(shown.enumerated()), id: \.offset) { index, source in
                SourceChip(source: source, tint: Mint.tints[index % Mint.tints.count], open: open)
            }
            if shown.count < sources.count {
                Button { withAnimation(.easeOut(duration: 0.2)) { expanded = true } } label: {
                    Text("+\(sources.count - shown.count)")
                        .font(.body(12, .medium)).foregroundStyle(Mint.muted)
                        .padding(.horizontal, 12)
                        .frame(height: 28)
                        .background(Capsule().fill(Mint.surface))
                        .overlay(Capsule().stroke(Mint.soft, lineWidth: 1.5))
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Show \(sources.count - shown.count) more sources")
            }
        }
    }
}

private struct SourceChip: View {
    let source: WebSource
    let tint: Color
    let open: (URL) -> Void

    var body: some View {
        let url = URL(string: source.url)
        let domain = sourceDomain(url) ?? source.url
        Button { if let url { open(url) } } label: {
            HStack(spacing: 6) {
                Text(domain.prefix(1).uppercased())
                    .font(.body(11, .bold)).foregroundStyle(Mint.on(tint))
                    .frame(width: 20, height: 20)
                    .background(Circle().fill(tint))
                Text(domain)
                    .font(.body(12, .medium)).foregroundStyle(Mint.ink)
                    .lineLimit(1).truncationMode(.tail)
            }
            .padding(.leading, 4).padding(.trailing, 10)
            .frame(height: 28)
            .frame(maxWidth: 180, alignment: .leading)
            .background(Capsule().fill(Mint.surface))
            .overlay(Capsule().stroke(Mint.soft, lineWidth: 1.5))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .disabled(url == nil)
        .accessibilityLabel(source.title.isEmpty ? domain : "\(source.title), \(domain)")
    }
}

/// The host of `url` without a leading "www.", or nil when it has none.
private func sourceDomain(_ url: URL?) -> String? {
    guard let host = url?.host(), !host.isEmpty else { return nil }
    return host.replacingOccurrences(of: "www.", with: "", options: .anchored)
}
