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
    static let tints = [Color(hex: 0xFDE7E2), Color(hex: 0xE2F4EA), Color(hex: 0xFFF4CC)]
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

/// A thick ink border with a solid drop under it, the app's "chunky" look.
struct Chunky: ViewModifier {
    var fill: Color = Mint.surface
    var radius: CGFloat = 12
    var drop: CGFloat = 3

    func body(content: Content) -> some View {
        content
            .background(RoundedRectangle(cornerRadius: radius).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: radius).stroke(Mint.ledge, lineWidth: 1.5))
            .background(RoundedRectangle(cornerRadius: radius).fill(Mint.ledge).offset(y: drop))
    }
}

extension View {
    func chunky(_ fill: Color = Mint.surface, radius: CGFloat = 12, drop: CGFloat = 3) -> some View {
        modifier(Chunky(fill: fill, radius: radius, drop: drop))
    }

    func card(_ fill: Color = Mint.surface, radius: CGFloat = 16) -> some View {
        background(RoundedRectangle(cornerRadius: radius).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: radius).stroke(Mint.ledge, lineWidth: 1.5))
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
            .background(RoundedRectangle(cornerRadius: 14).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(Mint.ledge, lineWidth: 1.5))
            .offset(y: configuration.isPressed ? 2 : 0)
            .background(RoundedRectangle(cornerRadius: 14).fill(Mint.ledge).offset(y: 3))
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
            .overlay(Capsule().stroke(Mint.ledge, lineWidth: 1.5))
            .opacity(configuration.isPressed ? 0.7 : 1)
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
            .background(RoundedRectangle(cornerRadius: size / 4).fill(tint))
            .overlay(RoundedRectangle(cornerRadius: size / 4).stroke(Mint.ledge, lineWidth: 1.5))
    }
}
