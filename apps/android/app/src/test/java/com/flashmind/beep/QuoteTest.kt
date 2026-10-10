package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QuoteTest {
    @Test fun splitsTheQuoteFromTheReply() {
        val q = splitQuote("> Somewhere near the station?\n> Not too fancy\n\nThree places nearby")
        assertEquals("Somewhere near the station?\nNot too fancy", q.quote)
        assertEquals("Three places nearby", q.body)
    }

    @Test fun plainMessageHasNoQuote() {
        val q = splitQuote("Just text\n> later quote")
        assertNull(q.quote)
        assertEquals("Just text\n> later quote", q.body)
    }

    @Test fun aQuoteAloneStaysAsWritten() = assertNull(splitQuote("> only a quote").quote)

    @Test fun withQuoteRoundTrips() {
        val sent = withQuote("Dinner on the 14th\nSomewhere near?", "Not sushi")
        assertEquals("> Dinner on the 14th\n> Somewhere near?\n\nNot sushi", sent)
        assertEquals("Not sushi", splitQuote(sent).body)
    }

    @Test fun findsTheNewestQuotedMessage() {
        val contents = listOf("Dinner on the 14th", "Something else", "Dinner on the 14th, again", "reply")
        assertEquals(2, quotedIndex(contents, 3, "Dinner on the 14th"))
        assertEquals(0, quotedIndex(contents, 2, "Dinner on the 14th"))
        assertNull(quotedIndex(contents, 3, "Nowhere"))
    }
}
