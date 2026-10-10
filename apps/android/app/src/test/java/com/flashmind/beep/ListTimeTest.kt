package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.ZoneOffset
import java.time.ZonedDateTime

class ListTimeTest {
    private val now = ZonedDateTime.of(2026, 10, 9, 15, 0, 0, 0, ZoneOffset.UTC) // a Friday

    private fun at(days: Long, hour: Int = 9) =
        now.minusDays(days).withHour(hour).withMinute(41).toInstant().toEpochMilli()

    @Test fun thisWeek() = assertEquals("Sat", listTime(at(6), now))

    @Test fun yesterday() = assertEquals("Yesterday", listTime(at(1), now))

    @Test fun earlierThisYear() = assertEquals("1 Oct", listTime(now.withDayOfMonth(1).toInstant().toEpochMilli(), now))

    @Test fun lastYear() = assertEquals("3 Oct 2025", listTime(now.minusYears(1).withDayOfMonth(3).toInstant().toEpochMilli(), now))

    @Test fun todayShowsTheTime() = assert(listTime(at(0), now).contains("41"))
}
