package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test

class ChatLineTest {
    @Test fun groupBodyNamesTheSender() {
        val line = ChatNotifications.groupLine("Sam: who has the keys?")
        assertEquals("Sam" to "who has the keys?", line.sender to line.text)
    }

    @Test fun groupBodyWithoutNameIsBeep() {
        val line = ChatNotifications.groupLine("bins tonight\nand more: here")
        assertEquals(ChatNotifications.BEEP, line.sender)
    }
}
