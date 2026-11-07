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
}
