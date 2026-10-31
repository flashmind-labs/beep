package com.flashmind.beep

import com.flashmind.beep.proto.WebSource
import org.junit.Assert.assertEquals
import org.junit.Test

class SourcesTest {
    private val page = WebSource("https://www.example.com/a", "A")

    @Test
    fun sourcesGoOnTheLastReplyWithTextOfTheirTurn() {
        val messages = listOf(
            ChatMessage("u1", "user", "first", sentAt = 10),
            ChatMessage("a1", "assistant", "one"),
            ChatMessage("a2", "assistant", "two"),
            ChatMessage("a3", "assistant", ""),
            ChatMessage("u2", "user", "second", sentAt = 20),
            ChatMessage("a4", "assistant", "three"),
        )
        val out = withSources(messages, mapOf(10L to listOf(page)))
        assertEquals(listOf("a2"), out.filter { it.sources.isNotEmpty() }.map { it.id })
    }

    @Test
    fun hostDropsWww() {
        assertEquals("example.com", sourceHost("https://www.example.com/a?b=c"))
        assertEquals("docs.example.com", sourceHost("https://docs.example.com"))
        assertEquals("not a url", sourceHost("not a url"))
    }

    @Test
    fun moreThanThreeFold() {
        assertEquals(3 to 0, sourceFold(3, expanded = false))
        assertEquals(3 to 2, sourceFold(5, expanded = false))
        assertEquals(5 to 0, sourceFold(5, expanded = true))
    }
}
