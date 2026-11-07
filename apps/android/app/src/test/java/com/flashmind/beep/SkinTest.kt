package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test

class SkinTest {
    @Test fun old_style_key_is_match_system() =
        assertEquals("linear", Skins.resolve("mint_quiet", dark = true).id)

    @Test fun match_system_follows_dark_mode() {
        assertEquals("linear", Skins.resolve("system", dark = true).id)
        assertEquals("paper", Skins.resolve("system", dark = false).id)
    }

    @Test fun picked_skin_ignores_mode() =
        assertEquals("tokyo", Skins.resolve("tokyo", dark = false).id)

    @Test fun all_seventeen_skins() {
        assertEquals(17, Skins.all.size)
        assertEquals(
            listOf("flashmind", "linear", "classic", "quiet", "github", "zed", "tokyo", "mocha", "nord",
                "gruvbox", "rosepine", "geist", "mono", "paper", "solarized", "macos", "warm"),
            Skins.all.map { it.id })
    }
}
