import XCTest
@testable import Beep

final class SkinTests: XCTestCase {
    func test_old_style_key_is_match_system() {
        XCTAssertEqual(Skins.resolve("mint_quiet", dark: true).id, "linear")
    }

    func test_match_system_follows_dark_mode() {
        XCTAssertEqual(Skins.resolve("system", dark: true).id, "linear")
        XCTAssertEqual(Skins.resolve("system", dark: false).id, "paper")
    }

    func test_picked_skin_ignores_mode() {
        XCTAssertEqual(Skins.resolve("tokyo", dark: false).id, "tokyo")
    }

    func test_all_seventeen_skins() {
        XCTAssertEqual(Skins.all.count, 17)
        XCTAssertEqual(Skins.all.map(\.id), ["flashmind", "linear", "classic", "quiet", "github", "zed", "tokyo",
            "mocha", "nord", "gruvbox", "rosepine", "geist", "mono", "paper", "solarized", "macos", "warm"])
    }

    func test_unknown_values_normalize_to_system() {
        XCTAssertEqual(Skins.normalize("mint_quiet"), "system")
        XCTAssertEqual(Skins.normalize("tokyo"), "tokyo")
    }

    func test_small_radius_is_set_or_three_quarters() {
        XCTAssertEqual(Skins.all.first { $0.id == "classic" }?.palette.smallRadius, 8)
        XCTAssertEqual(Skins.all.first { $0.id == "linear" }?.palette.smallRadius, 6)
    }
}
