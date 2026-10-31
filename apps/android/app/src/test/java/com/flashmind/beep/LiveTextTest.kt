package com.flashmind.beep

import android.speech.SpeechRecognizer
import com.flashmind.beep.ffi.LeaveReason
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveTextTest {
    @Test
    fun dictationGoesAfterWhatWasTyped() {
        assertEquals("book the plumber", appendDictation("", " book the plumber"))
        assertEquals("Remind me tonight. And book it", appendDictation("Remind me tonight.", "And book it"))
        assertEquals("Line one\nnext", appendDictation("Line one\n", "next"))
    }

    @Test
    fun dictationErrorsAreShort() {
        assertEquals("Didn’t catch that. Try again.", dictationError(SpeechRecognizer.ERROR_NO_MATCH))
        assertEquals("Dictation needs a connection", dictationError(SpeechRecognizer.ERROR_NETWORK))
        // Cancelling dictation reports a client error, which shows nothing.
        assertNull(dictationError(SpeechRecognizer.ERROR_CLIENT))
    }

    @Test
    fun draftJoinsTheComposer() {
        assertEquals("move the call", joinDraft("  ", " move the call "))
        assertEquals("Hi there move the call", joinDraft("Hi there ", "move the call"))
    }

    @Test
    fun leaveNoteSaysWhyAndKeepsWords() {
        assertEquals("Left live mode. Your words are kept here.", leaveNoteText(LeaveCause.Closed, kept = true))
        assertEquals("Live mode ended after 2 minutes quiet.", leaveNoteText(LeaveCause.Quiet, kept = false))
    }

    @Test
    fun whatTheAppAskedForWinsOverTheSession() {
        assertEquals(LeaveCause.AudioTaken, leaveCause(LeaveCause.AudioTaken, LeaveReason.INTERRUPTED))
        assertEquals(LeaveCause.Call, leaveCause(null, LeaveReason.INTERRUPTED))
        assertEquals(LeaveCause.Quiet, leaveCause(null, LeaveReason.QUIET))
    }

    @Test
    fun linesWithoutVoiceStayLongerWhenLonger() {
        assertTrue(readingMs("Done.") < readingMs("You have the dentist at 9:30, then a call with Alex at 11."))
        assertEquals(8_000L, readingMs("x".repeat(1_000)))
    }

    @Test
    fun hintsFollowTheStage() {
        assertEquals("Let go to send · Slide away to cancel", liveHint(LiveState(stage = LiveStage.Holding), true, false))
        assertEquals("Beep goes on once you answer", liveHint(LiveState(stage = LiveStage.Waiting), true, false))
        assertNull(liveHint(LiveState(problem = LiveProblem("No microphone")), true, false))
    }

    @Test
    fun silenceIsQuiet() {
        assertEquals(0f, waveLevel(ByteArray(64) { 128.toByte() }), 0.001f)
        assertEquals(1f, waveLevel(ByteArray(64) { if (it % 2 == 0) 0 else 255.toByte() }), 0.001f)
    }
}
