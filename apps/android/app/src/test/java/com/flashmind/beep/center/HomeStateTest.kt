package com.flashmind.beep.center

import com.flashmind.beep.ffi.AskOption
import com.flashmind.beep.ffi.MachineCell
import com.flashmind.beep.ffi.MobileHome
import com.flashmind.beep.ffi.MobileNotice
import com.flashmind.beep.ffi.Need
import com.flashmind.beep.ffi.NoticeKind
import com.flashmind.beep.proto.AgentOption
import com.flashmind.beep.proto.AgentQuestion
import com.flashmind.beep.proto.AgentStatus
import com.flashmind.beep.proto.MachineSession
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

class HomeStateTest {
    private fun ask(id: UInt): Need.Ask = Need.Ask(MachineSession("m1", "build-box", "s1", "claude", null, "/srv/project", AgentStatus.ASKING,
        null, Instant.EPOCH, Instant.EPOCH, false, AgentQuestion(id, "Run tests?", null, listOf(AgentOption("yes", "Allow", true), AgentOption("no", "Deny", false)))))

    private fun home(vararg needs: Need) = MobileHome(emptyList<MachineCell>(), needs.toList(), emptyList(), emptyList(), emptyList(), emptyList())

    private fun notice(id: String, at: ULong) = MobileNotice(id, NoticeKind.FINISHED, "Done", "", "m1", "claude", id, null, emptyList<AskOption>(), at)

    @Test fun badge_counts_needs() = runBlocking {
        val model = HomeModel(load = { home(ask(1u), ask(2u)) }, send = { _, _, _, _, _ -> })
        assertEquals(0, model.badge)
        model.refresh()
        assertEquals(2, model.badge)
    }

    @Test fun answer_removes_row_optimistically() = runBlocking {
        val session = ask(1u).session
        var remote = listOf<Need>(ask(1u), ask(2u))
        var seen = -1
        lateinit var model: HomeModel
        model = HomeModel(load = { home(*remote.toTypedArray()) }, send = { _, _, _, _, _ ->
            seen = model.badge
            // A refresh that began before the answer landed still lists the question.
            model.refresh()
            remote = remote.drop(1)
        })
        model.refresh()
        model.answer(session, session.question!!.options[0])
        assertEquals("the row is gone while the answer is sent", 1, seen)
        assertEquals("a stale refresh cannot bring it back", 1, model.badge)
        model.refresh()
        assertEquals(1, model.badge)
    }

    @Test fun a_failed_answer_brings_the_row_back() = runBlocking {
        val session = ask(1u).session
        val model = HomeModel(load = { home(ask(1u)) }, send = { _, _, _, _, _ -> error("offline") })
        model.refresh()
        model.answer(session, session.question!!.options[0])
        assertEquals(1, model.badge)
        assertTrue(model.error.isNotEmpty())
    }

    @Test fun bell_keeps_last_hundred() {
        var log = BellLog()
        for (i in 1..150) log = log.add(notice("n$i", i.toULong()))
        assertEquals(100, log.notices.size)
        assertEquals("n150", log.notices.first().id)
        assertEquals("n51", log.notices.last().id)
        assertEquals("a repeat replaces the older notice", 100, log.add(notice("n150", 999uL)).notices.size)
    }

    @Test fun stay_connected_defaults_off() {
        assertFalse(NotifyPrefs.defaultOn(NotifyPrefs.STAY))
        assertTrue(NotifyPrefs.defaultOn(NotifyPrefs.ASKS))
        assertTrue(NotifyPrefs.defaultOn(NotifyPrefs.FINISHED))
        assertTrue(NotifyPrefs.defaultOn(NotifyPrefs.CI))
    }
}
