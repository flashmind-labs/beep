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

    @Test
    fun aPauseOfOverTenMinutesStartsANewStretch() {
        val minute = 60_000L
        val messages = listOf(ChatMessage("a", "user", "a", spoken = true, sentAt = 0 * minute + 1), reply("b"),
            ChatMessage("c", "user", "c", spoken = true, sentAt = 10 * minute + 1), reply("d"),
            ChatMessage("e", "user", "e", spoken = true, sentAt = 21 * minute + 1), reply("f"))
        val stretches = liveStretches(messages)
        assertEquals(listOf("a", "e"), stretches.keys.toList())
        assertEquals(listOf("a", "b", "c", "d"), stretches.getValue("a").map { it.id })
        assertEquals(listOf("e", "f"), stretches.getValue("e").map { it.id })
    }

    @Test
    fun aReplyWithAFileOrACardKeepsItsOwnPlace() {
        val file = ChatMessage("b", "assistant", "here", files = listOf(ToolCard.File("f1", "invoice.pdf", "application/pdf", 84_000L)))
        val place = ChatMessage("d", "assistant", "there", toolCalls = listOf(ToolCall("t", "get_location", isDone = true,
            result = "{\"latitude\":41.98,\"longitude\":2.82}")))
        val messages = listOf(user("a", true), file, user("c", true), place, user("e", true), reply("f"))
        val stretches = liveStretches(messages)
        assertEquals(listOf("a", "c", "e"), stretches.keys.toList())
        assertEquals(listOf("a"), stretches.getValue("a").map { it.id })
        assertEquals(listOf("e", "f"), stretches.getValue("e").map { it.id })
    }
}
