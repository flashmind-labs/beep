package com.flashmind.beep

import com.flashmind.beep.proto.Approval
import org.junit.Assert.assertEquals
import org.junit.Test

class LiveStretchTest {
    private fun user(id: String, spoken: Boolean) = ChatMessage(id, "user", "said $id", spoken = spoken)
    private fun reply(id: String) = ChatMessage(id, "assistant", "reply $id")

    @Test
    fun spokenTurnsAndTheirRepliesFormOneStretchUntilATypedPrompt() {
        val messages = listOf(user("a", false), reply("b"), user("c", true), reply("d"), user("e", true), reply("f"),
            user("g", false), reply("h"))
        val stretches = liveStretches(messages)
        assertEquals(listOf("c"), stretches.keys.toList())
        assertEquals(listOf("c", "d", "e", "f"), stretches.getValue("c").map { it.id })
    }

    @Test
    fun aRoutineOrAWaitingCardEndsTheStretch() {
        val waiting = ChatMessage("w", "assistant", "", approvals = listOf(ApprovalPrompt("q", Approval("Fastmail", "send email", emptyList()))))
        val messages = listOf(user("a", true), reply("b"), waiting, user("c", true), ChatMessage("s", "scheduled", "Plants"), reply("d"))
        val stretches = liveStretches(messages)
        assertEquals(listOf("a", "c"), stretches.keys.toList())
        assertEquals(listOf("a", "b"), stretches.getValue("a").map { it.id })
        assertEquals(listOf("c"), stretches.getValue("c").map { it.id })
    }
}
