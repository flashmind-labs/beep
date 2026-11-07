import SwiftUI

/// A skin's colors and corner.
struct Palette {
    var page, surface, raised, ink, muted, dim: Color
    var line, soft, user, onUser: Color
    var brand, accent, onAccent, ok, warn, error: Color
    var tints: [Color]
    var code, onCode: Color
    var outline: CGFloat = 1
    var radius: CGFloat = 8
    var body: String? = "Inter-Regular"

    /// Built from the generated table in Skins.swift.
    static func skin(page: UInt32, surface: UInt32, raised: UInt32, ink: UInt32, muted: UInt32, dim: UInt32,
                     line: UInt32, accent: UInt32, onAccent: UInt32, ok: UInt32, warn: UInt32, error: UInt32,
                     brand: UInt32, user: UInt32, tint: UInt32, radius: CGFloat) -> Palette {
        Palette(page: Color(hex: page), surface: Color(hex: surface), raised: Color(hex: raised), ink: Color(hex: ink),
            muted: Color(hex: muted), dim: Color(hex: dim), line: Color(hex: line), soft: Color(hex: line),
            user: Color(hex: user), onUser: Color(hex: ink),
            brand: Color(hex: brand), accent: Color(hex: accent), onAccent: Color(hex: onAccent),
            ok: Color(hex: ok), warn: Color(hex: warn), error: Color(hex: error),
            tints: [Color(hex: raised), Color(hex: surface), Color(hex: tint)],
            code: Color(hex: raised), onCode: Color(hex: ink), radius: radius)
    }
}

/// One of the desktop's skins.
struct Skin {
    let id: String
    let name: String
    let light: Bool
    let palette: Palette
}

extension Skins {
    static let system = "system"

    /// The picked skin. "system", and any old or unknown value, is Linear in dark mode and Light in light mode.
    static func resolve(_ choice: String, dark: Bool) -> Skin {
        let id = all.contains { $0.id == choice } ? choice : (dark ? "linear" : "paper")
        return all.first { $0.id == id }!
    }
}

/// The picked skin, saved on the device. Views read it through `Mint`, so observation
/// redraws everything in the new skin when it changes.
@Observable final class Appearance {
    static let shared = Appearance()
    private static let key = "style"

    private(set) var palette: Palette
    private(set) var skin: Skin

    var choice: String {
        didSet {
            UserDefaults.standard.set(choice, forKey: Self.key)
            apply()
        }
    }

    /// The system appearance; the root view keeps it current.
    var dark: Bool {
        didSet { if dark != oldValue { apply() } }
    }

    private init() {
        let choice = UserDefaults.standard.string(forKey: Self.key) ?? Skins.system
        let dark = UITraitCollection.current.userInterfaceStyle != .light
        self.choice = choice
        self.dark = dark
        let skin = Skins.resolve(choice, dark: dark)
        self.skin = skin
        palette = skin.palette
    }

    private func apply() {
        skin = Skins.resolve(choice, dark: dark)
        palette = skin.palette
    }

    static var palette: Palette { shared.palette }
}

/// The current skin's colors. The name is from when Mint Quiet was the only look.
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
    var showsBack = true
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 4) {
            if showsBack { BackArrow(action: back) }
            Text(title.display).font(.display(18)).foregroundStyle(Mint.ink).lineLimit(1)
            Spacer()
            trailing
        }
        .padding(.leading, showsBack ? 6 : 18).padding(.trailing, 18)
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
    init(title: String, back: @escaping () -> Void, showsBack: Bool = true) {
        self.init(title: title, back: back, showsBack: showsBack) { EmptyView() }
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
