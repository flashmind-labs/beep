package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test

class TerminalTextTest {
    @Test fun colorCodesDoNotLeakIntoShellOutput() {
        assertEquals("passed", TerminalText().append("\u001b[32mpassed\u001b[0m".toByteArray()))
    }

    @Test fun unicodeSurvivesByteBoundaries() {
        val terminal = TerminalText()
        var text = ""
        "café €".toByteArray().forEach { text = terminal.append(byteArrayOf(it)) }
        assertEquals("café €", text)
    }

    @Test fun progressLineIsReplaced() {
        assertEquals("done", TerminalText().append("downloading 50%\r\u001b[Kdone".toByteArray()))
    }

    @Test fun cursorMovementUpdatesThePreviousLine() {
        assertEquals("new\nsecond", TerminalText().append("first\nsecond\u001b[1A\rnew\u001b[K".toByteArray()))
    }

    @Test fun titleSequenceCanSpanFrames() {
        val terminal = TerminalText()
        terminal.append("\u001b]0;title".toByteArray())
        terminal.append("\u001b".toByteArray())
        assertEquals("prompt", terminal.append("\\prompt".toByteArray()))
    }

    @Test fun outputHistoryIsBounded() {
        assertEquals("two\nthree", TerminalText(maxLines = 2).append("one\ntwo\nthree".toByteArray()))
    }

    @Test fun clearScreenDropsOldOutput() {
        assertEquals("ready", TerminalText().append("old\noutput\u001b[2Jready".toByteArray()))
    }

    @Test fun longLinesWrapAtTheAttachedWidth() {
        assertEquals("abcd\ne", TerminalText(columns = 4).append("abcde".toByteArray()))
    }
}
