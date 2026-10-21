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

/// How cards are set off from the page.
enum CardLook {
    /// Outline plus hard offset shadow, same as buttons.
    case chunky
    /// No outline, a solid ledge underneath.
    case ledge
    /// Thin soft border, no shadow.
    case hairline
}

/// One look's colors, shapes and fonts: the Android `BeepTokens`.
struct Palette {
    var page, surface, raised, ink, muted, dim: Color
    /// Outline on pressable things. Clear when the look has none.
    var line: Color
    var soft: Color
    /// Hard shadow or clay ledge under surface-coloured pieces.
    var ledge: Color
    var user, onUser, userLedge: Color
    var brand, accent, onAccent, ok, warn, error: Color
    var tints: [Color]
    var code, onCode: Color
    var outline: CGFloat
    var drop: CGSize
    var radius: CGFloat
    var card: CardLook
    /// Whether assistant replies sit in a bubble or read as plain text.
    var botBubble: Bool
    var display: String
    /// For a variable display face; nil when the name already picks the weight.
    var displayWeight: Font.Weight? = nil
    var displayUppercase: Bool
    /// Display tracking in em.
    var displayTracking: CGFloat
    /// Nil for the system font.
    var body: String?

    static func of(_ style: BeepStyle) -> Palette {
        switch style {
        case .mintQuiet: mintQuiet
        case .softClay: softClay
        case .bubblegum: bubblegum
        case .rodeo: rodeo
        }
    }

    static let mintQuiet = Palette(
        page: Color(hex: 0xF4F8F5), surface: .white, raised: Color(hex: 0xE9F2EC),
        ink: Color(hex: 0x10201A), muted: Color(hex: 0x5C6E64), dim: Color(hex: 0x94A49B),
        line: Color(hex: 0x1F3A2E), soft: Color(hex: 0xCFE0D6), ledge: Color(hex: 0x1F3A2E),
        user: Color(hex: 0xD5F1E3), onUser: Color(hex: 0x10201A), userLedge: Color(hex: 0x1F3A2E),
        brand: Color(hex: 0x2F8A69), accent: Color(hex: 0xF07A6A), onAccent: Color(hex: 0x10201A),
        ok: Color(hex: 0x3B9B7A), warn: Color(hex: 0xE0A630), error: Color(hex: 0xD9473A),
        tints: [Color(hex: 0xFDE7E2), Color(hex: 0xE2F4EA), Color(hex: 0xFFF4CC)],
        code: Color(hex: 0xECF3EE), onCode: Color(hex: 0x1C2E26),
        outline: 2, drop: CGSize(width: 3, height: 3), radius: 18, card: .hairline, botBubble: false,
        display: "BricolageGrotesque-96ptExtraBold", displayUppercase: false, displayTracking: -0.03, body: "Rubik-Light")

    static let softClay = Palette(
        page: Color(hex: 0xF2EEFA), surface: .white, raised: Color(hex: 0xEAE4F8),
        ink: Color(hex: 0x2E2547), muted: Color(hex: 0x7A7194), dim: Color(hex: 0xABA4C2),
        line: .clear, soft: Color(hex: 0xE2DBF2), ledge: Color(hex: 0xDCD3F0),
        user: Color(hex: 0xFFD3B4), onUser: Color(hex: 0x4A2A12), userLedge: Color(hex: 0xF2BC96),
        brand: Color(hex: 0x6E52E0), accent: Color(hex: 0x8B6CFF), onAccent: .white,
        ok: Color(hex: 0x2FBF7A), warn: Color(hex: 0xE6A23C), error: Color(hex: 0xE0523F),
        tints: [Color(hex: 0xE4DCFF), Color(hex: 0xFFE3D0), Color(hex: 0xD3F5E3)],
        code: Color(hex: 0xF1EDFA), onCode: Color(hex: 0x3A3058),
        outline: 0, drop: CGSize(width: 0, height: 5), radius: 24, card: .ledge, botBubble: true,
        display: "Fredoka", displayWeight: .semibold, displayUppercase: false, displayTracking: -0.01, body: "Nunito")

    static let bubblegum = Palette(
        page: Color(hex: 0xFAF4F0), surface: .white, raised: Color(hex: 0xF4ECE6),
        ink: Color(hex: 0x161616), muted: Color(hex: 0x6E6862), dim: Color(hex: 0xA39C95),
        line: Color(hex: 0x161616), soft: Color(hex: 0xE6DDD5), ledge: Color(hex: 0x161616),
        user: Color(hex: 0xFF90E8), onUser: Color(hex: 0x161616), userLedge: Color(hex: 0x161616),
        brand: Color(hex: 0x2563EB), accent: Color(hex: 0xFFC900), onAccent: Color(hex: 0x161616),
        ok: Color(hex: 0x29CC6A), warn: Color(hex: 0xDCAE01), error: Color(hex: 0xE40C0C),
        tints: [Color(hex: 0xDBEAFE), Color(hex: 0xFFD3B4), Color(hex: 0xC7F9C6)],
        code: Color(hex: 0xF7F0EA), onCode: Color(hex: 0x161616),
        outline: 2, drop: CGSize(width: 3, height: 3), radius: 16, card: .chunky, botBubble: true,
        display: "Sniglet-ExtraBold", displayUppercase: true, displayTracking: 0, body: nil)

    static let rodeo = Palette(
        page: Color(hex: 0xF3EFE7), surface: Color(hex: 0xFBF9F5), raised: Color(hex: 0xE9E4DA),
        ink: Color(hex: 0x111111), muted: Color(hex: 0x77736C), dim: Color(hex: 0xA9A49B),
        line: Color(hex: 0x111111), soft: Color(hex: 0xE2DCD1), ledge: Color(hex: 0x111111),
        user: Color(hex: 0x111111), onUser: .white, userLedge: Color(hex: 0x111111),
        brand: Color(hex: 0x3B6FD9), accent: Color(hex: 0x111111), onAccent: .white,
        ok: Color(hex: 0x3B6FD9), warn: Color(hex: 0xD9932B), error: Color(hex: 0xDC3B2E),
        tints: [Color(hex: 0xEDE4D3), Color(hex: 0xE2E9F3), Color(hex: 0xF5E1DC)],
        code: Color(hex: 0xEAE5DB), onCode: Color(hex: 0x111111),
        outline: 1.5, drop: .zero, radius: 16, card: .hairline, botBubble: false,
        display: "BricolageGrotesque-96ptExtraBold", displayUppercase: false, displayTracking: -0.04, body: nil)
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
    /// The hard shadow or ledge under a surface-coloured piece.
    static var shadow: Color { Appearance.palette.ledge }
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
    static var drop: CGSize { Appearance.palette.drop }
    static var radius: CGFloat { Appearance.palette.radius }

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

    /// The ledge under a piece filled with `fill`: the look's ledge for plain surfaces, a darker
    /// shade otherwise, as on Android.
    static func ledge(under fill: Color) -> Color {
        let p = Appearance.palette
        if p.outline > 0 || fill == p.surface { return p.ledge }
        if fill == p.user { return p.userLedge }
        return fill.mix(with: .black, by: 0.18)
    }
}

extension Color {
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
    static func display(_ size: CGFloat) -> Font {
        let p = Appearance.palette
        let font = Font.custom(p.display, size: size)
        return p.displayWeight.map { font.weight($0) } ?? font
    }
    static func body(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        guard let body = Appearance.palette.body else { return .system(size: size, weight: weight) }
        return .custom(body, size: size).weight(weight)
    }
}

extension String {
    /// Display text, uppercased when the look asks for it.
    var display: String { Appearance.palette.displayUppercase ? uppercased() : self }
}

/// The frame for things you touch, the Android `beepFrame` in `pressableLook()`: an outline and
/// hard shadow, or a clay ledge in a look without outlines. `drop` scales the look's shadow;
/// 3 is the full one.
struct Chunky: ViewModifier {
    var fill: Color = Mint.surface
    var shape: AnyShape = AnyShape(RoundedRectangle(cornerRadius: 18))
    var drop: CGFloat = 3
    var look: CardLook? = nil

    func body(content: Content) -> some View {
        let p = Appearance.palette
        let look = look ?? (p.card == .hairline ? .chunky : p.card)
        let offset = CGSize(width: p.drop.width * drop / 3, height: p.drop.height * drop / 3)
        switch look {
        case .hairline:
            content
                .background(shape.fill(fill))
                .overlay(shape.stroke(p.soft, lineWidth: 1.5))
        case .chunky, .ledge:
            content
                .background(shape.fill(fill))
                .overlay { if look == .chunky, p.outline > 0 { shape.stroke(p.line, lineWidth: p.outline) } }
                .background(shape.fill(Mint.ledge(under: fill)).offset(offset))
        }
    }
}

/// A message bubble's shape, with the corner by its sender pinched.
func bubbleShape(user: Bool, radius: CGFloat = Mint.radius) -> UnevenRoundedRectangle {
    user ? UnevenRoundedRectangle(topLeadingRadius: radius, bottomLeadingRadius: radius,
                                  bottomTrailingRadius: 6, topTrailingRadius: radius)
        : UnevenRoundedRectangle(topLeadingRadius: 6, bottomLeadingRadius: radius,
                                 bottomTrailingRadius: radius, topTrailingRadius: radius)
}

extension View {
    /// The user's bubble: a plain fill in a hairline look, else framed like a card.
    @ViewBuilder func userBubble<S: Shape>(_ shape: S) -> some View {
        if Appearance.palette.card == .hairline {
            background(shape.fill(Mint.user))
        } else {
            lookCard(Mint.user, shape: shape)
        }
    }

    /// Beep's reply in a bubble, for the looks that have one; plain text otherwise.
    @ViewBuilder func botBubble() -> some View {
        if Appearance.palette.botBubble {
            padding(.horizontal, 14).padding(.vertical, 10)
                .lookCard(shape: bubbleShape(user: false))
                .padding(.trailing, 24)
        } else {
            self
        }
    }

    func chunky(_ fill: Color = Mint.surface, radius: CGFloat = 18, drop: CGFloat = 3) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(RoundedRectangle(cornerRadius: radius)), drop: drop))
    }

    func chunky<S: Shape>(_ fill: Color = Mint.surface, shape: S, drop: CGFloat = 3) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(shape), drop: drop))
    }

    /// A content card in the look's card style, which is quieter than `chunky` in some looks.
    /// An outlined card without a shadow, or a ledge card in a look without outlines.
    @ViewBuilder func card(_ fill: Color = Mint.surface, radius: CGFloat = 18) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius)
        if Mint.outline > 0 {
            background(shape.fill(fill)).overlay(shape.stroke(Mint.ledge, lineWidth: Mint.outline))
        } else {
            modifier(Chunky(fill: fill, shape: AnyShape(shape), look: .ledge))
        }
    }

    /// A card framed exactly as the look frames its cards, hairline included.
    func lookCard<S: Shape>(_ fill: Color = Mint.surface, shape: S) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(shape), look: Appearance.palette.card))
    }

    /// The hard shadow under a piece, as the look draws it.
    func dropShadow<S: Shape>(_ shape: S, fill: Color = Mint.surface) -> some View {
        background(shape.fill(Mint.ledge(under: fill)).offset(Mint.drop))
    }
}

/// A square chunky button that sinks into its shadow while pressed.
struct ChunkyButton: ButtonStyle {
    var fill: Color = Mint.surface
    var size: CGFloat = 42
    var radius: CGFloat = 12

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius)
        let drop = Mint.drop
        let down = configuration.isPressed ? drop : .zero
        configuration.label
            .font(.system(size: size * 0.45, weight: .semibold))
            .foregroundStyle(Mint.on(fill))
            .frame(width: size, height: size)
            .background(shape.fill(fill))
            .overlay { if Mint.outline > 0 { shape.stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .offset(down)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge(under: fill)).offset(drop))
    }
}

/// The small connection status dot, pulsing while it reconnects.
struct ConnectionDot: View {
    let connected: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dim = false

    var body: some View {
        Circle().fill(connected ? Mint.ok : Mint.error).frame(width: 7, height: 7)
            .opacity(!connected && dim ? 0.3 : 1)
            .animation(.default, value: connected)
            .task(id: connected) {
                withTransaction(Transaction(animation: nil)) { dim = false }
                guard !connected, !reduceMotion else { return }
                withAnimation(.easeInOut(duration: 0.6).repeatForever(autoreverses: true)) { dim = true }
            }
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

/// The Android top bar for pushed screens: chunky back button, title, trailing actions.
struct BackBar<Trailing: View>: View {
    let title: String
    let back: () -> Void
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 14) {
            Button(action: back) { Image(systemName: "arrow.left") }
                .buttonStyle(ChunkyButton())
            Text(title.display).font(.display(24)).foregroundStyle(Mint.ink)
            Spacer()
            trailing
        }
        .padding(.horizontal, 18)
        .padding(.top, 6)
        .padding(.bottom, 10)
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
            .background(RoundedRectangle(cornerRadius: 10).fill(selected ? Mint.user : Mint.surface))
            .overlay(RoundedRectangle(cornerRadius: 10)
                .stroke(selected ? Mint.ink : Mint.soft, lineWidth: selected ? 2 : 1))
        }
        .buttonStyle(.plain)
    }
}

/// The coral button with a drop, pressed down on tap.
struct PrimaryButton: ButtonStyle {
    var fill: Color = Mint.accent

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: Mint.radius)
        configuration.label
            .font(.body(16, .semibold))
            .foregroundStyle(Mint.on(fill))
            .frame(maxWidth: .infinity, minHeight: 52)
            .background(shape.fill(fill))
            .overlay { if Mint.outline > 0 { shape.stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .offset(configuration.isPressed ? Mint.drop : .zero)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge(under: fill)).offset(Mint.drop))
    }
}

struct PillButton: ButtonStyle {
    var fill: Color = Mint.surface

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body(14, .medium))
            .foregroundStyle(Mint.on(fill))
            .padding(.horizontal, 14)
            .frame(minHeight: 36)
            .background(Capsule().fill(fill))
            .overlay { if Mint.outline > 0 { Capsule().stroke(Mint.ledge, lineWidth: Mint.outline) } }
            .background { if Mint.outline == 0 { Capsule().fill(Mint.ledge(under: fill)).offset(Mint.drop) } }
            .scaleEffect(configuration.isPressed ? 0.94 : 1)
            .animation(.sink, value: configuration.isPressed)
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
            .background(RoundedRectangle(cornerRadius: 12).fill(tint))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Mint.ledge, lineWidth: 2))
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
        .background(RoundedRectangle(cornerRadius: 12).fill(Mint.ink))
    }
}
