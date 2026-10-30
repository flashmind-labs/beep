package com.flashmind.beep

import com.flashmind.beep.proto.Approval
import org.junit.Assert.assertEquals
import org.junit.Test

class LastReplyTest {
    private val background = ChatMessage("approval-q", "assistant", "",
        approvals = listOf(ApprovalPrompt("q", Approval("Fastmail", "send email", emptyList()), background = true)))

    @Test
    fun aBackgroundCardAtTheEndIsNotTheLastReply() {
        val messages = listOf(ChatMessage("u", "user", "hi"), ChatMessage("a", "assistant", "hello"), background)
        assertEquals("a", messages.lastReply()?.id)
        assertEquals("a", messages.lastOfTurns()?.id)
    }

    @Test
    fun aPromptAfterTheReplyIsTheLastOfTheTurns() {
        val messages = listOf(ChatMessage("a", "assistant", "hello"), background, ChatMessage("u", "user", "hi"))
        assertEquals("a", messages.lastReply()?.id)
        assertEquals("u", messages.lastOfTurns()?.id)
    }
}
