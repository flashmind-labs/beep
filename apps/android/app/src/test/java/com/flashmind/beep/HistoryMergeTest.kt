package com.flashmind.beep

import com.flashmind.beep.proto.Approval
import org.junit.Assert.assertEquals
import org.junit.Test

class HistoryMergeTest {
    private val approval = Approval("Fastmail", "send email", emptyList())

    @Test
    fun aTurnCardDoesNotTakeTheIdOfAWaitingBackgroundCard() {
        val background = ChatMessage("approval-q1", "assistant", "", approvals = listOf(ApprovalPrompt("q1", approval, background = true)))
        val before = listOf(ChatMessage("u1", "user", "hi"), background)
        // The history has its own card asking before a call, with no text either.
        val fresh = listOf(ChatMessage("history-c-0", "user", "hi"),
            ChatMessage("history-c-1", "assistant", "", approvals = listOf(ApprovalPrompt("q2", approval))))
        val merged = mergeHistory(before, fresh)
        assertEquals(listOf("u1", "history-c-1", "approval-q1"), merged.map { it.id })
    }

    @Test
    fun messagesOnScreenKeepTheirIdsAndScores() {
        val before = listOf(ChatMessage("u1", "user", "hi"), ChatMessage("a1", "assistant", "hello", memoryScore = 0.9))
        val fresh = listOf(ChatMessage("history-c-0", "user", "hi"), ChatMessage("history-c-1", "assistant", "hello"))
        val merged = mergeHistory(before, fresh)
        assertEquals(listOf("u1", "a1"), merged.map { it.id })
        assertEquals(0.9, merged[1].memoryScore!!, 0.0)
    }
}
