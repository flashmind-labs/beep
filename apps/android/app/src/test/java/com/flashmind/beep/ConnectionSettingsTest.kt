package com.flashmind.beep

import org.junit.Assert.*
import org.junit.Test

class ConnectionSettingsTest {
    @Test fun aServerUrlIsEnoughToConfigure() {
        assertTrue(ConnectionSettings("https://beep.example.com").configured)
        assertFalse(ConnectionSettings(" ").configured)
    }
}
