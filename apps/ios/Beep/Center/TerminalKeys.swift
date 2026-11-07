import SwiftUI
import UIKit

enum TermKey: Equatable {
    case esc, tab, up, down, left, right, home, end
    case char(Character)
}

enum TermMod: Hashable { case ctrl, alt, shift }

/// The bytes a key sends. Arrows, Home and End carry the xterm modifier number (Shift 2, Alt 3, Ctrl 5).
func bytes(for key: TermKey, mods: Set<TermMod>) -> [UInt8] {
    var number = 1
    if mods.contains(.shift) { number += 1 }
    if mods.contains(.alt) { number += 2 }
    if mods.contains(.ctrl) { number += 4 }
    func csi(_ final: Character) -> [UInt8] {
        Array((number == 1 ? "\u{1b}[\(final)" : "\u{1b}[1;\(number)\(final)").utf8)
    }
    switch key {
    case .esc: return [0x1b]
    case .tab: return mods.contains(.shift) ? Array("\u{1b}[Z".utf8) : [0x09]
    case .up: return csi("A")
    case .down: return csi("B")
    case .right: return csi("C")
    case .left: return csi("D")
    case .home: return csi("H")
    case .end: return csi("F")
    case .char(let c):
        var out = Array((mods.contains(.shift) ? String(c).uppercased() : String(c)).utf8)
        // Ctrl with a letter or one of @ [ \\ ] ^ _ is the control byte.
        if mods.contains(.ctrl), out.count == 1, (0x40...0x7f).contains(out[0]) { out = [out[0] & 0x1f] }
        return mods.contains(.alt) ? [0x1b] + out : out
    }
}

/// Ctrl, Alt and Shift stay armed for one key.
struct TermKeyState {
    private(set) var mods: Set<TermMod> = []

    mutating func toggle(_ mod: TermMod) {
        if mods.contains(mod) { mods.remove(mod) } else { mods.insert(mod) }
    }

    /// Bytes from the soft keyboard. Only a lone printable ASCII byte takes the armed modifiers;
    /// anything longer (pastes, mouse and device replies from the emulator) passes unchanged and
    /// leaves them armed.
    mutating func applyTyped(_ typed: [UInt8]) -> [UInt8] {
        guard !mods.isEmpty, typed.count == 1, typed[0] >= 0x20, typed[0] < 0x7f else { return typed }
        return press(.char(Character(UnicodeScalar(typed[0]))))
    }

    mutating func press(_ key: TermKey) -> [UInt8] {
        defer { mods = [] }
        return bytes(for: key, mods: mods)
    }
}

/// The two rows of nine keys above the keyboard. Empty cells keep the arrows an inverted T.
final class TerminalKeyBar: UIView {
    private enum Cell {
        case key(String, TermKey), mod(String, TermMod), gap
    }
    private static let rows: [[Cell]] = [
        [.key("Esc", .esc), .key("Tab", .tab), .key("|", .char("|")), .key("/", .char("/")), .key("~", .char("~")),
         .key("-", .char("-")), .gap, .key("↑", .up), .gap],
        [.mod("Ctrl", .ctrl), .mod("Alt", .alt), .mod("Shift", .shift), .key("Home", .home), .key("End", .end),
         .gap, .key("←", .left), .key("↓", .down), .key("→", .right)],
    ]

    var send: ([UInt8]) -> Void = { _ in }
    private(set) var state = TermKeyState()
    private var modButtons: [TermMod: UIButton] = [:]

    override init(frame: CGRect) {
        super.init(frame: CGRect(x: 0, y: 0, width: 0, height: 5 + 28 + 4 + 28 + 6))
        backgroundColor = UIColor(red: 0x17 / 255, green: 0x18 / 255, blue: 0x1b / 255, alpha: 1)
        autoresizingMask = .flexibleWidth
        let rows = UIStackView(arrangedSubviews: Self.rows.map(row))
        rows.axis = .vertical; rows.spacing = 4
        rows.translatesAutoresizingMaskIntoConstraints = false
        addSubview(rows)
        NSLayoutConstraint.activate([
            rows.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 5),
            rows.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -5),
            rows.topAnchor.constraint(equalTo: topAnchor, constant: 5),
        ])
    }
    required init?(coder: NSCoder) { fatalError("not used") }

    override var intrinsicContentSize: CGSize { CGSize(width: UIView.noIntrinsicMetric, height: 71) }

    private func row(_ cells: [Cell]) -> UIStackView {
        let stack = UIStackView(arrangedSubviews: cells.map(view))
        stack.axis = .horizontal; stack.spacing = 4; stack.distribution = .fillEqually
        stack.heightAnchor.constraint(equalToConstant: 28).isActive = true
        return stack
    }

    private func view(_ cell: Cell) -> UIView {
        switch cell {
        case .gap: return UIView()
        case .key(let title, let key):
            return button(title) { [weak self] in
                guard let self else { return }
                send(state.press(key)); refresh()
            }
        case .mod(let title, let mod):
            let item = button(title) { [weak self] in self?.state.toggle(mod); self?.refresh() }
            modButtons[mod] = item
            return item
        }
    }

    private func button(_ title: String, action: @escaping () -> Void) -> UIButton {
        var config = UIButton.Configuration.plain()
        config.contentInsets = .zero
        config.background.cornerRadius = 5
        config.attributedTitle = AttributedString(title, attributes: AttributeContainer([.font: UIFont.monospacedSystemFont(ofSize: 11, weight: .regular)]))
        let item = UIButton(configuration: config, primaryAction: UIAction { _ in action() })
        item.accessibilityLabel = title
        item.configurationUpdateHandler = nil
        paint(item, on: false)
        return item
    }

    private func paint(_ item: UIButton, on: Bool) {
        item.configuration?.baseForegroundColor = on ? UIColor(Mint.onAccent) : UIColor(red: 0xd0 / 255, green: 0xd3 / 255, blue: 0xd8 / 255, alpha: 1)
        item.configuration?.background.backgroundColor = on ? UIColor(Mint.accent) : UIColor(red: 0x26 / 255, green: 0x27 / 255, blue: 0x2b / 255, alpha: 1)
    }

    private func refresh() {
        for (mod, item) in modButtons { paint(item, on: state.mods.contains(mod)) }
    }

    func apply(_ typed: [UInt8]) -> [UInt8] {
        defer { refresh() }
        return state.applyTyped(typed)
    }
}
