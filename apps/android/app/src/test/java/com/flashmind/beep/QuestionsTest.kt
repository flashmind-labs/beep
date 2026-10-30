package com.flashmind.beep

import com.flashmind.beep.proto.Approval
import com.flashmind.beep.proto.PendingApproval
import org.junit.Assert.assertEquals
import org.junit.Test

class QuestionsTest {
    private val approval = Approval("Fastmail", "send email", emptyList())
    private fun pending(id: String) = PendingApproval("", id, approval)
    private fun answers(messages: List<ChatMessage>) = messages.flatMap { m -> m.approvals.map { it.queryId to it.answer } }

    @Test
    fun aResolvedQuestionStopsWaitingUnlessThisPhoneAnsweredIt() {
        val messages = listOf(questionCard(pending("q1")),
            questionCard(pending("q2")).let { m -> m.copy(approvals = m.approvals.map { it.copy(answer = "allow") }) })
        val after = markResolved(markResolved(messages, "q1"), "q2")
        assertEquals(listOf("q1" to RESOLVED, "q2" to "allow"), answers(after))
    }

    @Test
    fun listedQuestionsGetCardsAndUnlistedOnesStopWaiting() {
        val messages = listOf(ChatMessage("u", "user", "hi"), questionCard(pending("q1")), questionCard(pending("q2")))
        val after = syncQuestions(messages, listOf(pending("q2"), pending("q3")))
        assertEquals(listOf("u", "approval-q1", "approval-q2", "approval-q3"), after.map { it.id })
        assertEquals(listOf("q1" to RESOLVED, "q2" to "", "q3" to ""), answers(after))
    }

    @Test
    fun aTurnCardIsNotTouched() {
        val turn = ChatMessage("a", "assistant", "", approvals = listOf(ApprovalPrompt("q1", approval)))
        assertEquals(listOf(turn), syncQuestions(listOf(turn), emptyList()))
        assertEquals(listOf(turn), markResolved(listOf(turn), "q1"))
    }
}
