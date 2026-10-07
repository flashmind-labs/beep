package com.flashmind.beep

import org.junit.Assert.*
import org.junit.Test

class ConnectionSettingsTest {
    @Test fun nodeIdIsEnoughToConfigure() {
        val config = ConnectionSettings.fromJson("""{"node_id":"example-node-id"}""")
        assertTrue(config.configured)
        assertFalse(ConnectionSettings(nodeId = " ").configured)
    }
    @Test fun oldProfilesWithATokenStillLoad() {
        val config = ConnectionSettings.fromJson("""{"node_id":"example-node-id","token":"app-token"}""")
        assertEquals("example-node-id", config.nodeId)
    }
}
