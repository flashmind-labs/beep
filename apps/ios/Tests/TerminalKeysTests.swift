import XCTest
@testable import Beep

final class TerminalKeysTests: XCTestCase {
    func test_ctrl_c_is_03() {
        XCTAssertEqual(bytes(for: .char("c"), mods: [.ctrl]), [0x03])
    }

    func test_shift_up_is_csi_1_2A() {
        XCTAssertEqual(bytes(for: .up, mods: [.shift]), [0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x41])
        XCTAssertEqual(bytes(for: .up, mods: [.ctrl]), Array("\u{1b}[1;5A".utf8))
        XCTAssertEqual(bytes(for: .left, mods: [.alt]), Array("\u{1b}[1;3D".utf8))
        XCTAssertEqual(bytes(for: .down, mods: []), Array("\u{1b}[B".utf8))
    }

    func test_alt_x_is_esc_x() {
        XCTAssertEqual(bytes(for: .char("x"), mods: [.alt]), [0x1b, 0x78])
    }

    func test_sticky_ctrl_clears_after_one_key() {
        var keys = TermKeyState()
        keys.toggle(.ctrl)
        XCTAssertTrue(keys.mods.contains(.ctrl))
        XCTAssertEqual(keys.press(.char("c")), [0x03])
        XCTAssertTrue(keys.mods.isEmpty)
        XCTAssertEqual(keys.press(.char("c")), [0x63])
    }

    func test_home_end_bytes() {
        XCTAssertEqual(bytes(for: .home, mods: []), Array("\u{1b}[H".utf8))
        XCTAssertEqual(bytes(for: .end, mods: []), Array("\u{1b}[F".utf8))
        XCTAssertEqual(bytes(for: .esc, mods: []), [0x1b])
        XCTAssertEqual(bytes(for: .tab, mods: []), [0x09])
    }
}
