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

    @Test fun unknown_values_normalize_to_system() {
        assertEquals("system", Skins.normalize("mint_quiet"))
        assertEquals("system", Skins.normalize(null))
        assertEquals("tokyo", Skins.normalize("tokyo"))
    }

    @Test fun small_radius_is_set_or_three_quarters() {
        assertEquals(8f, Skins.all.first { it.id == "classic" }.smallRadius.value)
        assertEquals(6f, Skins.all.first { it.id == "linear" }.smallRadius.value)
    }
}
