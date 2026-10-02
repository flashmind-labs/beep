package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test

class NdjsonDecoderTest {
    @Test fun preservesUtf8WhenEveryByteIsASingleChunk() {
        val output = mutableListOf<String>()
        val decoder = NdjsonDecoder { output.add(it) }
        val lines = listOf("{\"delta\":\"Café ☕ مرحبا\"}", "{\"seq\":2}")
        (lines.joinToString("\n") + "\n").toByteArray().forEach { decoder.feed(byteArrayOf(it)) }
        decoder.finish()
        assertEquals(lines, output)
    }
    @Test(expected = IllegalStateException::class) fun detectsTruncatedEventsForReplay() {
        val decoder = NdjsonDecoder { }
        decoder.feed("{\"seq\":3".toByteArray())
        decoder.finish()
    }
    @Test fun handlesMultipleEventsPerChunkAndBlankLines() {
        val output = mutableListOf<String>()
        val decoder = NdjsonDecoder { output.add(it) }
        decoder.feed("\n{\"seq\":1}\r\n{\"seq\":2}\n".toByteArray())
        decoder.finish()
        assertEquals(listOf("{\"seq\":1}", "{\"seq\":2}"), output)
    }
}
