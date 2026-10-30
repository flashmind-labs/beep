package com.flashmind.beep

import org.junit.Assert.assertEquals
import org.junit.Test

class ToolLabelTest {
    private fun live(name: String, said: String) = ToolCall("c", name, said)
    private fun history(name: String, result: String) = ToolCall("c", name, isDone = true, result = result)

    @Test
    fun aFoldedToolSaysWhatTheServerNamedWhileRunningAndThePastWhenDone() {
        val recall = live("memory", "Recalling memories")
        assertEquals("Recalling memories", callLabel(recall))
        assertEquals("Recalled memories", doneToolLabel(recall.copy(isDone = true)))
        val spawn = live("background", "Starting an agent")
        assertEquals("Starting an agent", callLabel(spawn))
        assertEquals("Started an agent", doneToolLabel(spawn.copy(isDone = true)))
    }

    @Test
    fun aFoldedToolFromHistoryReadsItsActionFromTheOutput() {
        assertEquals("Recalled memories", doneToolLabel(history("memory", """[{"id":"m1","content":"Likes tea"}]""")))
        assertEquals("Remembered", doneToolLabel(history("memory", """{"stored":true,"id":"m1","content":"Likes tea"}""")))
        assertEquals("Forgot a memory", doneToolLabel(history("memory", """{"forgotten":true}""")))
        assertEquals("Saved a skill", doneToolLabel(history("skill", """{"created":true,"id":"s1","name":"Trips"}""")))
        assertEquals("Logged a habit", doneToolLabel(history("habit", """{"logged":true,"habit":"Run","value":1}""")))
        assertEquals("Started an agent", doneToolLabel(history("background", "Started agent w1. Its report comes back to this chat when it ends.")))
        assertEquals("Checked background work", doneToolLabel(history("background", "w1 agent, running 3 min: Compare flights")))
    }

    @Test
    fun aCallCachedWithTheToolNameAsItsDescriptionStillGetsALabel() {
        assertEquals("Checking memories", callLabel(live("memory", "Memory")))
    }

    @Test
    fun everyKnownToolHasAWordedLabel() {
        assertEquals("Drew a chart", doneToolLabel(history("show_chart", "{}")))
        assertEquals("Planned a route", doneToolLabel(history("get_route", "")))
        assertEquals("Gmail: send email", doneToolLabel(history("GMAIL_SEND_EMAIL", "")))
    }
}
