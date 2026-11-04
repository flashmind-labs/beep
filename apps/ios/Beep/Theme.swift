import SwiftUI

/// The looks a user can pick, as on Android. The raw value is what gets stored, so never rename one.
enum BeepStyle: String, CaseIterable, Identifiable {
    case mintQuiet = "mint_quiet"
    case softClay = "soft_clay"
    case bubblegum
    case rodeo

    var id: String { rawValue }

    var label: String {
        switch self {
        case .mintQuiet: "Mint Quiet"
        case .softClay: "Soft Clay"
        case .bubblegum: "Bubblegum"
        case .rodeo: "Rodeo"
        }
    }
}

/// Linear colors resolve against the current system appearance.
struct Palette {
    var page, surface, raised, ink, muted, dim: Color
    var line, soft, user, onUser: Color
    var brand, accent, onAccent, ok, warn, error: Color
    var tints: [Color]
    var code, onCode: Color
    var outline: CGFloat = 1
    var radius: CGFloat = 8
    var body: String? = "Inter-Regular"

    static func of(_ style: BeepStyle) -> Palette {
        let page = Color.adaptive(0xF7F8FA, 0x0F1011)
        let surface = Color.adaptive(0xFFFFFF, 0x151617)
        let raised = Color.adaptive(0xF0F1F4, 0x1C1D1F)
        let ink = Color.adaptive(0x202126, 0xE3E4E6)
        let line = Color.adaptive(0xE1E3E8, 0x222326)
        let accent: Color = switch style {
        case .mintQuiet: Color(hex: 0x5E6AD2)
        case .softClay: Color(hex: 0x8B6CD2)
        case .bubblegum: .adaptive(0x9C357F, 0xE18BC7)
        case .rodeo: .adaptive(0x676C76, 0x8A8F98)
        }
        return Palette(page: page, surface: surface, raised: raised, ink: ink,
            muted: .adaptive(0x676C76, 0x8A8F98), dim: .adaptive(0x767B85, 0x5B5F65),
            line: line, soft: line,
            user: .adaptive(0xD5D9F1, 0x202439), onUser: ink,
            brand: .adaptive(0x4F5BC0, 0x8A9CF0), accent: accent, onAccent: .white,
            ok: .adaptive(0x268557, 0x4CB782), warn: .adaptive(0x946B00, 0xF2C94C),
            error: .adaptive(0xC33838, 0xEB5757),
            tints: [raised, surface, accent.opacity(0.12)], code: raised, onCode: ink)
    }
}

/// The picked look, saved on the device. Views read it through `Mint`, so observation
/// redraws everything in the new look when it changes.
@Observable final class Appearance {
    static let shared = Appearance()
    private static let key = "style"

    private(set) var palette: Palette

    var style: BeepStyle {
        didSet {
            palette = Palette.of(style)
            UserDefaults.standard.set(style.rawValue, forKey: Self.key)
        }
    }

    private init() {
        let style = UserDefaults.standard.string(forKey: Self.key).flatMap(BeepStyle.init(rawValue:)) ?? .mintQuiet
        self.style = style
        palette = Palette.of(style)
    }

    static var palette: Palette { shared.palette }
}

/// The current look's colors. The name is from when Mint Quiet was the only one.
enum Mint {
    static var page: Color { Appearance.palette.page }
    static var surface: Color { Appearance.palette.surface }
    static var raised: Color { Appearance.palette.raised }
    static var ink: Color { Appearance.palette.ink }
    static var muted: Color { Appearance.palette.muted }
    static var dim: Color { Appearance.palette.dim }
    /// The outline on pressable things and cards; clear in a look without outlines.
    static var ledge: Color { Appearance.palette.line }
    static var soft: Color { Appearance.palette.soft }
    static var user: Color { Appearance.palette.user }
    static var onUser: Color { Appearance.palette.onUser }
    static var brand: Color { Appearance.palette.brand }
    static var accent: Color { Appearance.palette.accent }
    static var onAccent: Color { Appearance.palette.onAccent }
    static var ok: Color { Appearance.palette.ok }
    static var warn: Color { Appearance.palette.warn }
    static var error: Color { Appearance.palette.error }
    static var code: Color { Appearance.palette.code }
    static var tints: [Color] { Appearance.palette.tints }
    static var outline: CGFloat { Appearance.palette.outline }
    static var radius: CGFloat { Appearance.palette.radius }
    static let smallRadius: CGFloat = 6
    static let spacing: CGFloat = 12
    static let pageInset: CGFloat = 16
    static let activity = Color(hex: 0x151617)
    static var nameColors: [Color] { [error, ok, warn] }

    /// A number for a name or id that stays the same across launches, unlike `hashValue`.
    static func tintIndex(_ key: String) -> Int { key.unicodeScalars.reduce(0) { $0 + Int($1.value) } }

    /// The same tint for a group, place or person everywhere it shows.
    static func tint(for key: String) -> Color { tints[tintIndex(key) % tints.count] }

    /// Text and icons on `fill`: white on a dark accent or user color in the looks that have one.
    static func on(_ fill: Color) -> Color {
        let p = Appearance.palette
        if fill == p.accent { return p.onAccent }
        if fill == p.user { return p.onUser }
        return p.ink
    }

}

extension Color {
    static func adaptive(_ light: UInt32, _ dark: UInt32) -> Color {
        Color(uiColor: UIColor { traits in
            let hex = traits.userInterfaceStyle == .dark ? dark : light
            return UIColor(red: CGFloat((hex >> 16) & 255) / 255,
                           green: CGFloat((hex >> 8) & 255) / 255,
                           blue: CGFloat(hex & 255) / 255, alpha: 1)
        })
    }

    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255)
    }

    /// Before iOS 18 there is no `mix`, so it falls back to this color.
    func mix(with other: Color, by amount: Double) -> Color {
        if #available(iOS 18, *) { return mix(with: other, by: amount, in: .perceptual) }
        return self
    }
}

extension Font {
    static func display(_ size: CGFloat) -> Font { .custom("Inter-Regular", size: size).weight(.semibold) }
    static func body(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font { .custom("Inter-Regular", size: size).weight(weight) }
}

extension String {
    var display: String { self }
}

struct Chunky: ViewModifier {
    var fill: Color = Mint.surface
    var shape: AnyShape = AnyShape(RoundedRectangle(cornerRadius: Mint.radius))

    func body(content: Content) -> some View {
        content.background(shape.fill(fill)).overlay(shape.stroke(Mint.soft, lineWidth: 1))
    }
}

/// A message bubble's shape, with the corner by its sender pinched.
func bubbleShape(user: Bool, radius: CGFloat = Mint.radius) -> UnevenRoundedRectangle {
    user ? UnevenRoundedRectangle(topLeadingRadius: 12, bottomLeadingRadius: 12,
                                  bottomTrailingRadius: 4, topTrailingRadius: 12)
        : UnevenRoundedRectangle(topLeadingRadius: 6, bottomLeadingRadius: radius,
                                 bottomTrailingRadius: radius, topTrailingRadius: radius)
}

extension View {
    func userBubble<S: Shape>(_ shape: S) -> some View { background(shape.fill(Mint.user)) }

    func chunky(_ fill: Color = Mint.surface, radius: CGFloat = Mint.radius) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(RoundedRectangle(cornerRadius: radius))))
    }

    func chunky<S: Shape>(_ fill: Color = Mint.surface, shape: S) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(shape)))
    }

    /// A content card with a fine border.
    func card(_ fill: Color = Mint.surface, radius: CGFloat = Mint.radius) -> some View {
        lookCard(fill, shape: RoundedRectangle(cornerRadius: radius))
    }

    /// A card framed exactly as the look frames its cards, hairline included.
    func lookCard<S: Shape>(_ fill: Color = Mint.surface, shape: S) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(shape)))
    }
}

/// A compact icon button with press feedback.
struct ChunkyButton: ButtonStyle {
    var fill: Color = Mint.surface
    var size: CGFloat = 40
    var radius: CGFloat = Mint.smallRadius

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius)
        configuration.label
            .font(.system(size: size * 0.45, weight: .semibold))
            .foregroundStyle(Mint.on(fill))
            .frame(width: size, height: size)
            .background(shape.fill(fill))
            .overlay { if Mint.outline > 0 { shape.stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .opacity(configuration.isPressed ? 0.7 : 1)
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

struct ConnectionDot: View {
    let connected: Bool
    var body: some View {
        Circle().fill(connected ? Mint.ok : Mint.error).frame(width: 7, height: 7)
    }
}

/// A plain text button in the brand color, as Android's TextButton.
struct TextAction: ButtonStyle {
    var color: Color = Mint.brand

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body(14, .medium))
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .frame(minHeight: 36)
            .opacity(configuration.isPressed ? 0.6 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

/// The Android top bar for pushed screens: a plain back arrow, title, trailing actions.
struct BackBar<Trailing: View>: View {
    let title: String
    let back: () -> Void
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 4) {
            BackArrow(action: back)
            Text(title.display).font(.display(18)).foregroundStyle(Mint.ink).lineLimit(1)
            Spacer()
            trailing
        }
        .padding(.leading, 6).padding(.trailing, 18)
        .frame(minHeight: 54)
        .overlay(alignment: .bottom) { Rectangle().fill(Mint.soft).frame(height: 1) }
    }
}

/// Android's back icon button: the arrow alone, with a full size touch target.
struct BackArrow: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: "chevron.left").font(.system(size: 20, weight: .medium)).foregroundStyle(Mint.ink)
                .frame(width: 48, height: 48).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Back")
    }
}

extension BackBar where Trailing == EmptyView {
    init(title: String, back: @escaping () -> Void) {
        self.init(title: title, back: back) { EmptyView() }
    }
}

/// Android's FilterChip: a capsule that fills with the user color when selected.
struct FilterChip: View {
    let label: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if selected { Image(systemName: "checkmark").font(.system(size: 12, weight: .bold)) }
                Text(label).font(.body(13, .medium))
            }
            .foregroundStyle(selected ? Mint.onUser : Mint.ink)
            .padding(.horizontal, 12)
            .frame(minHeight: 32)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(selected ? Mint.user : Mint.surface))
            .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius)
                .stroke(selected ? Mint.accent : Mint.soft, lineWidth: 1))
        }
        .buttonStyle(.plain)
    }
}

/// The primary action.
struct PrimaryButton: ButtonStyle {
    var fill: Color = Mint.accent

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: Mint.radius)
        configuration.label
            .font(.body(14, .semibold))
            .foregroundStyle(Mint.on(fill))
            .frame(maxWidth: .infinity, minHeight: 44)
            .background(shape.fill(fill))
            .overlay { if Mint.outline > 0 { shape.stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .opacity(configuration.isPressed ? 0.7 : 1)
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

struct PillButton: ButtonStyle {
    var fill: Color = Mint.surface

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body(14, .medium))
            .foregroundStyle(Mint.on(fill))
            .padding(.horizontal, 12)
            .frame(minHeight: 36)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(fill))
            .overlay { if Mint.outline > 0 { RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .opacity(configuration.isPressed ? 0.7 : 1)
            .animation(.easeOut(duration: 0.15), value: configuration.isPressed)
    }
}

/// A rounded square with a tinted fill and an icon, as on the Android cards.
struct TintIcon: View {
    let system: String
    let tint: Color
    var size: CGFloat = 40

    var body: some View {
        Image(systemName: system)
            .font(.system(size: size * 0.42, weight: .semibold))
            .foregroundStyle(Mint.ink)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(tint))
            .overlay(RoundedRectangle(cornerRadius: Mint.smallRadius).stroke(Mint.soft, lineWidth: 1))
    }
}

/// Android's snackbar with an action, such as Undo after a delete.
struct UndoBar: View {
    let text: String
    let undo: () -> Void

    var body: some View {
        HStack {
            Text(text).font(.body(14)).foregroundStyle(Mint.page)
            Spacer()
            Button("Undo", action: undo).buttonStyle(TextAction(color: Mint.tints[1]))
        }
        .padding(.leading, 16).padding(.trailing, 6).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: Mint.smallRadius).fill(Mint.ink))
    }
}
