package com.flashmind.beep

import java.time.LocalDate
import java.time.ZoneOffset
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Test

class ChatTimeTest {
    private val now = ZonedDateTime.of(2026, 10, 6, 12, 0, 0, 0, ZoneOffset.UTC)
    private val today = now.toLocalDate()

    private fun at(day: LocalDate, hour: Int) = day.atTime(hour, 0).toInstant(ZoneOffset.UTC).toEpochMilli()

    @Test
    fun daysReadAsTodayYesterdayOrADate() {
        assertEquals("Today", dayLabel(today, today))
        assertEquals("Yesterday", dayLabel(today.minusDays(1), today))
        assertEquals("20 Jan", dayLabel(LocalDate.of(2026, 1, 20), today))
        assertEquals("20 Jan 2025", dayLabel(LocalDate.of(2025, 1, 20), today))
    }

    @Test
    fun aNewDayStartsAtItsFirstPromptAndRepliesStayWithTheirPrompt() {
        val yesterday = today.minusDays(1)
        val messages = listOf(
            ChatMessage("a", "scheduled", "Evening digest", sentAt = at(yesterday, 20)),
            ChatMessage("b", "assistant", "Here is your digest"),
            ChatMessage("c", "scheduled", "Evening digest", sentAt = at(today, 8)),
            ChatMessage("d", "assistant", "Morning"),
            ChatMessage("e", "user", "Thanks", sentAt = at(today, 9)),
            ChatMessage("f", "user", "Not synced yet"),
        )
        assertEquals(mapOf("a" to yesterday, "c" to today), dayBreaks(messages, now))
    }
}
