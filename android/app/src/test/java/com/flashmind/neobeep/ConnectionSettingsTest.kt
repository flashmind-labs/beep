package com.flashmind.neobeep

import org.junit.Assert.*
import org.junit.Test

class ConnectionSettingsTest {
    @Test fun testingProfileDoesNotRequireCredentials() {
        val config = ConnectionSettings.fromJson("""{"address":"beep.example:4433","hostname":"localhost","ca":"public-certificate"}""")
        assertTrue(config.configured)
        assertEquals("", config.token)
        assertFalse(ConnectionSettings(address = "beep.example:4433").configured)
    }
    @Test fun existingAuthenticatedProfilesStillLoad() {
        val config = ConnectionSettings.fromJson("""{"address":"beep:4433","hostname":"beep","token":"app-token"}""")
        assertTrue(config.configured)
        assertEquals("app-token", config.token)
    }
}
