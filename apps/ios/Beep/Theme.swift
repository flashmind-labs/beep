import SwiftUI

/// The Mint Quiet palette the Android app uses.
enum Mint {
    static let page = Color(hex: 0xF4F8F5)
    static let surface = Color(hex: 0xFFFFFF)
    static let ink = Color(hex: 0x10201A)
    static let muted = Color(hex: 0x5C6E64)
    static let dim = Color(hex: 0x94A49B)
    static let ledge = Color(hex: 0x1F3A2E)
    static let soft = Color(hex: 0xCFE0D6)
    static let user = Color(hex: 0xD5F1E3)
    static let brand = Color(hex: 0x2F8A69)
    static let accent = Color(hex: 0xF07A6A)
    static let ok = Color(hex: 0x3B9B7A)
    static let error = Color(hex: 0xD9473A)
    static let code = Color(hex: 0xECF3EE)
    static let tints = [Color(hex: 0xFDE7E2), Color(hex: 0xE2F4EA), Color(hex: 0xFFF4CC)]

    /// A number for a name or id that stays the same across launches, unlike `hashValue`.
    static func tintIndex(_ key: String) -> Int { key.unicodeScalars.reduce(0) { $0 + Int($1.value) } }

    /// The same tint for a group, place or person everywhere it shows.
    static func tint(for key: String) -> Color { tints[tintIndex(key) % tints.count] }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255)
    }
}

extension Font {
    static func display(_ size: CGFloat) -> Font { .custom("BricolageGrotesque-96ptExtraBold", size: size) }
    static func body(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .custom("Rubik-Light", size: size).weight(weight)
    }
}

/// A 2px ink outline with a hard 3,3 shadow, the Android `beepFrame`.
struct Chunky: ViewModifier {
    var fill: Color = Mint.surface
    var shape: AnyShape = AnyShape(RoundedRectangle(cornerRadius: 18))
    var drop: CGFloat = 3

    func body(content: Content) -> some View {
        content
            .background(shape.fill(fill))
            .overlay(shape.stroke(Mint.ledge, lineWidth: 2))
            .background(shape.fill(Mint.ledge).offset(x: drop, y: drop))
    }
}

extension View {
    func chunky(_ fill: Color = Mint.surface, radius: CGFloat = 18, drop: CGFloat = 3) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(RoundedRectangle(cornerRadius: radius)), drop: drop))
    }

    func chunky<S: Shape>(_ fill: Color = Mint.surface, shape: S, drop: CGFloat = 3) -> some View {
        modifier(Chunky(fill: fill, shape: AnyShape(shape), drop: drop))
    }

    func card(_ fill: Color = Mint.surface, radius: CGFloat = 18) -> some View {
        background(RoundedRectangle(cornerRadius: radius).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: radius).stroke(Mint.ledge, lineWidth: 2))
    }
}

/// A square chunky button that sinks into its shadow while pressed.
struct ChunkyButton: ButtonStyle {
    var fill: Color = Mint.surface
    var size: CGFloat = 42
    var radius: CGFloat = 12

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius)
        let down: CGFloat = configuration.isPressed ? 3 : 0
        configuration.label
            .font(.system(size: size * 0.45, weight: .semibold))
            .foregroundStyle(Mint.ink)
            .frame(width: size, height: size)
            .background(shape.fill(fill))
            .overlay(shape.stroke(Mint.ledge, lineWidth: 2))
            .offset(x: down, y: down)
            .animation(.sink, value: configuration.isPressed)
            .background(shape.fill(Mint.ledge).offset(x: 3, y: 3))
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
            Text(title).font(.display(24)).foregroundStyle(Mint.ink)
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
            .foregroundStyle(Mint.ink)
            .padding(.horizontal, 12)
            .frame(minHeight: 32)
            .background(RoundedRectangle(cornerRadius: 10).fill(selected ? Mint.user : Mint.surface))
            .overlay(RoundedRectangle(cornerRadius: 10)
                .stroke(selected ? Mint.ledge : Mint.soft, lineWidth: selected ? 2 : 1))
        }
        .buttonStyle(.plain)
    }
}

/// The coral button with a drop, pressed down on tap.
struct PrimaryButton: ButtonStyle {
    var fill: Color = Mint.accent

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body(16, .semibold))
            .foregroundStyle(Mint.ink)
            .frame(maxWidth: .infinity, minHeight: 52)
            .background(RoundedRectangle(cornerRadius: 18).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: 18).stroke(Mint.ledge, lineWidth: 2))
            .offset(x: configuration.isPressed ? 3 : 0, y: configuration.isPressed ? 3 : 0)
            .animation(.sink, value: configuration.isPressed)
            .background(RoundedRectangle(cornerRadius: 18).fill(Mint.ledge).offset(x: 3, y: 3))
    }
}

struct PillButton: ButtonStyle {
    var fill: Color = Mint.surface

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body(14, .medium))
            .foregroundStyle(Mint.ink)
            .padding(.horizontal, 14)
            .frame(minHeight: 36)
            .background(Capsule().fill(fill))
            .overlay(Capsule().stroke(Mint.ledge, lineWidth: 2))
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
