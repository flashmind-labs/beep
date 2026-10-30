package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.Instant

class GrantsTest {
    private val now = Instant.parse("2026-10-30T12:00:00Z")

    @Test
    fun aGrantSaysWhenItEnds() {
        assertEquals("Ends in 8 h", grantEnds(now.plus(Duration.ofHours(8)).minusSeconds(1), now))
        assertEquals("Ends in 5 h", grantEnds(now.plus(Duration.ofMinutes(5 * 60 + 10)), now))
        assertEquals("Ends in 40 min", grantEnds(now.plus(Duration.ofMinutes(40)), now))
        assertEquals("Ends in 1 min", grantEnds(now.plusSeconds(5), now))
        assertEquals("Ends in 7 d", grantEnds(now.plus(Duration.ofDays(7)), now))
        assertEquals("Ended", grantEnds(now.minusSeconds(1), now))
    }

    @Test
    fun aGrantSaysWhenItWasLastUsed() {
        assertEquals("Not used yet", grantLastUsed(null, now))
        assertEquals("Last used just now", grantLastUsed(now.minusSeconds(20), now))
        assertEquals("Last used 40 min ago", grantLastUsed(now.minus(Duration.ofMinutes(40)), now))
        assertEquals("Last used 3 h ago", grantLastUsed(now.minus(Duration.ofMinutes(200)), now))
        assertEquals("Last used 2 d ago", grantLastUsed(now.minus(Duration.ofHours(50)), now))
    }

    @Test
    fun onlyTheCardForARoutineIsAGrant() {
        assertTrue(isRoutineGrant("reply to thread for a routine"))
        assertFalse(isRoutineGrant("send email"))
    }

    @Test
    fun theRoutineAndTheEndAreNotRepeatedInTheDetails() {
        assertFalse(isGrantDetail("Routine"))
        assertFalse(isGrantDetail("Ends"))
        assertTrue(isGrantDetail("To"))
    }

    @Test
    fun aRoutineCardCountsItsGrantsAndNamesTheSoonestEnd() {
        assertNull(routineGrantsLine(emptyList(), now))
        assertEquals("1 grant, ends in 5 h", routineGrantsLine(listOf(now.plus(Duration.ofHours(5))), now))
        assertEquals("2 grants, ends in 2 h", routineGrantsLine(listOf(now.plus(Duration.ofDays(3)), now.plus(Duration.ofHours(2))), now))
    }
}
