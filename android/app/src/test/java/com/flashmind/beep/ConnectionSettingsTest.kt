package com.flashmind.beep

import org.junit.Assert.*
import org.junit.Test

class ConnectionSettingsTest {
    @Test fun nodeIdIsEnoughToConfigure() {
        val config = ConnectionSettings.fromJson("""{"node_id":"example-node-id"}""")
        assertTrue(config.configured)
        assertEquals("", config.token)
        assertFalse(ConnectionSettings(nodeId = " ").configured)
    }
    @Test fun tokenLoads() {
        val config = ConnectionSettings.fromJson("""{"node_id":"example-node-id","token":"app-token"}""")
        assertTrue(config.configured)
        assertEquals("app-token", config.token)
    }
}
