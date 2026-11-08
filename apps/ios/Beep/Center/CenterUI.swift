import SwiftUI

// The command center's building blocks, sized as in mockups/ios/06-command-center.css.

/// True inside a bottom tab's root screen, which has no back button.
private struct TabRootKey: EnvironmentKey { static let defaultValue = false }

extension EnvironmentValues {
    var tabRoot: Bool {
        get { self[TabRootKey.self] }
        set { self[TabRootKey.self] = newValue }
    }
}

enum CCKind {
    case plain, wait, bad, ok
    var color: Color {
        switch self { case .plain: Mint.muted; case .wait: Mint.warn; case .bad: Mint.error; case .ok: Mint.ok }
    }
}

extension Font {
    static func mono(_ size: CGFloat) -> Font { .system(size: size, design: .monospaced) }
}

/// The 48pt bar: a title on the left at the root, centered under a back button on a pushed screen.
struct CCTop<Trailing: View>: View {
    let title: String
    var back: (() -> Void)?
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 4) {
            if let back {
                CCIconButton(system: "chevron.left", label: "Back", action: back)
                Text(title).font(.body(17, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    .frame(maxWidth: .infinity)
            } else {
                Text(title).font(.body(17, .semibold)).foregroundStyle(Mint.ink).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            trailing
        }
        .padding(.leading, back == nil ? 16 : 4).padding(.trailing, 8)
        .frame(height: 48)
        .overlay(alignment: .bottom) { Rectangle().fill(Mint.soft).frame(height: 1) }
    }
}

extension CCTop where Trailing == EmptyView {
    init(title: String, back: (() -> Void)? = nil) { self.init(title: title, back: back, trailing: { EmptyView() }) }
}

struct CCIconButton: View {
    let system: String
    let label: String
    var badge = 0
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: system).font(.system(size: 17)).foregroundStyle(Mint.muted)
                .frame(width: 36, height: 36)
                .overlay(alignment: .topTrailing) {
                    if badge > 0 {
                        Text("\(badge)").font(.body(9.5, .bold)).foregroundStyle(Color(hex: 0x111111))
                            .padding(.horizontal, 4).frame(minWidth: 15, minHeight: 15)
                            .background(Mint.warn, in: Capsule()).offset(x: -3, y: 4)
                    }
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain).accessibilityLabel(label)
    }
}

struct CCPill<Content: View>: View {
    let action: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) { content }.font(.body(12)).foregroundStyle(Mint.muted)
                .padding(.horizontal, 9).frame(height: 26)
                .overlay(Capsule().stroke(Mint.soft, lineWidth: 1))
        }.buttonStyle(.plain)
    }
}

/// A scrolling page body: 12pt insets, 14pt between blocks.
struct CCBody<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12).padding(.top, 12).padding(.bottom, 20)
        }
    }
}

struct CCSection<Content: View, Trailing: View>: View {
    let title: String
    var count: Int?
    @ViewBuilder var trailing: Trailing
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                HStack(spacing: 6) {
                    Text(title)
                    if let count { Text("\(count)").fontWeight(.medium).foregroundStyle(Mint.dim) }
                }
                .font(.body(12, .semibold)).foregroundStyle(Mint.muted)
                Spacer()
                trailing.font(.body(12)).foregroundStyle(Mint.brand)
            }.padding(.horizontal, 2)
            content
        }
    }
}

extension CCSection where Trailing == EmptyView {
    init(_ title: String, count: Int? = nil, @ViewBuilder content: () -> Content) {
        self.init(title: title, count: count, trailing: { EmptyView() }, content: content)
    }
}

/// Rows separated by 1pt lines inside a bordered, rounded surface.
struct CCPanel<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        // Each row draws its top line; the first one is pushed out of the clip.
        VStack(spacing: 0) { content }.padding(.top, -1)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Mint.surface)
            .clipShape(RoundedRectangle(cornerRadius: Mint.radius))
            .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.soft, lineWidth: 1))
    }
}

/// A 46pt row: a lead, a title with a muted one-line subtitle, and a right side.
struct CCRow<Lead: View, Right: View>: View {
    let title: String
    var subtitle = ""
    var mono = false
    var chevron = false
    var action: (() -> Void)?
    @ViewBuilder var lead: Lead
    @ViewBuilder var right: Right

    var body: some View {
        let row = HStack(spacing: 10) {
            lead
            VStack(alignment: .leading, spacing: 0) {
                Text(title).font(mono ? .mono(13) : .body(13, .medium)).foregroundStyle(Mint.ink).lineLimit(1)
                if !subtitle.isEmpty { Text(subtitle).font(.body(11.5)).foregroundStyle(Mint.muted).lineLimit(1) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 6) {
                right
                if chevron { Image(systemName: "chevron.right").font(.system(size: 12)).foregroundStyle(Mint.dim) }
            }
            .font(.body(11.5)).foregroundStyle(Mint.muted)
        }
        .padding(.horizontal, 11).padding(.vertical, 9).frame(maxWidth: .infinity, minHeight: 46)
        .overlay(alignment: .top) { Rectangle().fill(Mint.soft).frame(height: 1) }
        .contentShape(Rectangle())
        if let action { Button(action: action) { row }.buttonStyle(.plain) } else { row }
    }
}

/// The 24pt square that leads a row: a short label, colored by kind.
struct CCKindTile: View {
    let text: String
    var kind = CCKind.plain
    var body: some View {
        Text(text).font(.body(10, .bold)).foregroundStyle(kind.color)
            .frame(width: 24, height: 24).background(Mint.raised, in: RoundedRectangle(cornerRadius: 6))
    }
}

/// The agent's tile. These three colors are fixed, as in the mock.
struct CCAgentTile: View {
    let agent: String
    var body: some View {
        let style: (String, Color, Color) = switch agent {
        case "codex": ("CX", Color(hex: 0x1F2A3A), Color(hex: 0x8AB4F8))
        case "gemini": ("GM", Color(hex: 0x20303A), Color(hex: 0x7DD3C0))
        case "claude": ("CC", Color(hex: 0x3B2A20), Color(hex: 0xF0A070))
        default: (String(agent.prefix(2)).uppercased(), Color(hex: 0x3B2A20), Color(hex: 0xF0A070))
        }
        Text(style.0).font(.body(9, .bold)).foregroundStyle(style.2)
            .frame(width: 24, height: 24).background(style.1, in: RoundedRectangle(cornerRadius: 6))
    }
}

struct CCTag: View {
    let text: String
    var kind = CCKind.plain
    var body: some View {
        Text(text).font(.body(11)).lineLimit(1).foregroundStyle(kind.color)
            .padding(.horizontal, 7).frame(height: 20)
            .background(kind == .plain ? Mint.raised : kind.color.opacity(0.14), in: Capsule())
    }
}

struct CCButton: View {
    enum Style { case plain, primary, danger }
    let title: String
    var style = Style.plain
    var big = false
    var leading = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title).font(.body(big ? 14 : 12.5, .medium)).lineLimit(1)
                .foregroundStyle(style == .primary ? Mint.onAccent : style == .danger ? Mint.error : Mint.ink)
                .padding(.horizontal, 11)
                .frame(maxWidth: big || leading ? .infinity : nil, alignment: leading ? .leading : .center)
                .frame(height: big ? 40 : leading ? 36 : 30)
                .background(style == .primary ? Mint.accent : Mint.raised, in: RoundedRectangle(cornerRadius: Mint.smallRadius))
                .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius)
                    .stroke(style == .primary ? Mint.accent : Mint.soft, lineWidth: 1))
        }.buttonStyle(.plain)
    }
}

struct CCSegmented<Value: Hashable>: View {
    struct Item { let value: Value; let label: String; var count: Int? }
    let items: [Item]
    @Binding var selection: Value

    var body: some View {
        HStack(spacing: 2) {
            ForEach(items.indices, id: \.self) { index in
                let item = items[index], on = item.value == selection
                Button { selection = item.value } label: {
                    HStack(spacing: 4) {
                        Text(item.label)
                        if let count = item.count { Text("\(count)").fontWeight(.regular).foregroundStyle(Mint.muted) }
                    }
                    .font(.body(12.5, on ? .medium : .regular)).lineLimit(1)
                    .foregroundStyle(on ? Mint.ink : Mint.muted)
                    .frame(maxWidth: .infinity).frame(height: 28)
                    .background(on ? Mint.raised : .clear, in: RoundedRectangle(cornerRadius: Mint.smallRadius))
                }.buttonStyle(.plain).accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(2).background(Mint.surface, in: RoundedRectangle(cornerRadius: Mint.radius))
        .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.soft, lineWidth: 1))
    }
}

struct CCChips: View {
    let items: [String]
    @Binding var selection: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(items, id: \.self) { item in
                    let on = item == selection
                    Button { selection = item } label: {
                        Text(item).font(.body(12)).foregroundStyle(on ? Mint.ink : Mint.muted)
                            .padding(.horizontal, 10).frame(height: 28)
                            .background(on ? Mint.accent.opacity(0.16) : .clear, in: Capsule())
                            .overlay(Capsule().stroke(on ? Mint.accent : Mint.soft, lineWidth: 1))
                    }.buttonStyle(.plain)
                }
            }.padding(.horizontal, 12)
        }.padding(.horizontal, -12)
    }
}

struct CCDot: View {
    var online = true
    var warn = false
    var body: some View {
        Circle().fill(warn ? Mint.warn : online ? Mint.ok : Mint.dim).frame(width: 7, height: 7)
    }
}

struct CCToggle: View {
    @Binding var isOn: Bool
    var body: some View {
        Button { isOn.toggle() } label: {
            Capsule().fill(isOn ? Mint.accent : Mint.raised).frame(width: 38, height: 22)
                .overlay(Capsule().stroke(isOn ? Mint.accent : Mint.soft, lineWidth: 1))
                .overlay(alignment: isOn ? .trailing : .leading) {
                    Circle().fill(isOn ? Mint.onAccent : Mint.muted).frame(width: 16, height: 16).padding(2)
                }
        }.buttonStyle(.plain).accessibilityAddTraits(.isToggle).accessibilityValue(isOn ? "On" : "Off")
    }
}

struct CCBar: View {
    let fraction: Double
    var warn = false
    var body: some View {
        Capsule().fill(Mint.raised).frame(height: 4)
            .overlay(alignment: .leading) {
                GeometryReader { proxy in
                    Capsule().fill(warn ? Mint.warn : Mint.accent).frame(width: proxy.size.width * min(max(fraction, 0), 1))
                }
            }
    }
}

struct CCEmpty: View {
    let title: String
    let detail: String
    var body: some View {
        VStack(spacing: 10) {
            Text(title).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
            Text(detail).font(.body(13)).foregroundStyle(Mint.muted)
        }
        .multilineTextAlignment(.center).frame(maxWidth: .infinity).padding(.horizontal, 16).padding(.vertical, 22)
    }
}

struct CCNote: View {
    let text: String
    var body: some View {
        Text(text).font(.body(12)).foregroundStyle(Mint.muted).lineSpacing(3).fixedSize(horizontal: false, vertical: true)
    }
}

/// A machine of the horizontal strip.
struct CCStripCell: View {
    let name: String
    let detail: String
    var online = true
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) { CCDot(online: online); Text(name).font(.body(12.5, .semibold)).foregroundStyle(Mint.ink).lineLimit(1) }
            Text(detail).font(.body(11.5)).foregroundStyle(Mint.muted).lineLimit(1)
        }
        .padding(.horizontal, 10).padding(.vertical, 8).frame(minWidth: 132, alignment: .leading)
        .background(Mint.surface, in: RoundedRectangle(cornerRadius: Mint.radius))
        .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.soft, lineWidth: 1))
    }
}
