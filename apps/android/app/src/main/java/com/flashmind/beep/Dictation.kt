package com.flashmind.beep

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Dictation into the composer through the phone's speech recognizer. What is heard goes after
 * the text that was there when dictation started. The recognizer is made on first use, and each
 * start bumps a generation so results from a cancelled or stopped session are dropped. Used from
 * the main thread.
 */
internal class Dictation(private val context: Context) {
    /** Whether the phone has a speech recognition service at all. */
    val available: Boolean = SpeechRecognizer.isRecognitionAvailable(context)
    var listening by mutableStateOf(false)
        private set
    /** The microphone's level while listening, 0 to 1, for the ring around the mic. */
    var level by mutableFloatStateOf(0f)
        private set
    private var recognizer: SpeechRecognizer? = null
    private var generation = 0

    /**
     * Starts listening. [onText] gets [base] with what was heard so far after it; [onError]
     * gets a short line to show when recognition failed.
     */
    fun start(base: String, onText: (String) -> Unit, onError: (String) -> Unit) {
        if (!available) return
        cancel()
        val speech = recognizer ?: SpeechRecognizer.createSpeechRecognizer(context).also { recognizer = it }
        val mine = ++generation
        listening = true
        speech.setRecognitionListener(object : RecognitionListener {
            override fun onReadyForSpeech(params: Bundle?) {}
            override fun onBeginningOfSpeech() {}
            override fun onRmsChanged(rmsdB: Float) {
                // About -2 dB in silence to 10 dB for speech.
                if (mine == generation) level = ((rmsdB + 2f) / 12f).coerceIn(0f, 1f)
            }
            override fun onBufferReceived(buffer: ByteArray?) {}
            override fun onEndOfSpeech() {}
            override fun onError(error: Int) {
                if (mine != generation) return
                done()
                dictationError(error)?.let(onError)
            }
            override fun onResults(results: Bundle?) {
                if (mine != generation) return
                done()
                val text = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty()
                if (text.isNotBlank()) onText(appendDictation(base, text))
                else dictationError(SpeechRecognizer.ERROR_NO_MATCH)?.let(onError)
            }
            override fun onPartialResults(partial: Bundle?) {
                if (mine != generation) return
                val text = partial?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty()
                if (text.isNotBlank()) onText(appendDictation(base, text))
            }
            override fun onEvent(eventType: Int, params: Bundle?) {}
        })
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
        }
        try {
            speech.startListening(intent)
        } catch (error: Exception) {
            done()
            onError("Dictation isn’t available right now")
        }
    }

    /** Stops listening; what was heard still comes in. */
    fun stop() {
        if (listening) recognizer?.stopListening()
    }

    /** Stops listening and drops anything still to come, as on sending or leaving the chat. */
    fun cancel() {
        generation++
        if (listening) recognizer?.cancel()
        done()
    }

    fun destroy() {
        cancel()
        recognizer?.destroy()
        recognizer = null
    }

    private fun done() {
        listening = false
        level = 0f
    }
}

/** [heard] after [base], with a space between them when needed. */
internal fun appendDictation(base: String, heard: String): String = when {
    base.isBlank() -> heard.trim()
    base.last().isWhitespace() -> base + heard.trim()
    else -> base + " " + heard.trim()
}

/** A short line for a recognizer error, or null for none, as when dictation was cancelled. */
internal fun dictationError(code: Int): String? = when (code) {
    SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> "Didn’t catch that. Try again."
    SpeechRecognizer.ERROR_AUDIO -> "The microphone isn’t working"
    SpeechRecognizer.ERROR_NETWORK, SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "Dictation needs a connection"
    SpeechRecognizer.ERROR_SERVER, 11 /* ERROR_SERVER_DISCONNECTED */ -> "Dictation isn’t available right now"
    SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> "Dictation is busy. Try again in a moment."
    SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "Allow the microphone for Beep in Settings"
    10 /* ERROR_TOO_MANY_REQUESTS */ -> "Too many tries. Wait a moment."
    12, 13 /* ERROR_LANGUAGE_NOT_SUPPORTED, ERROR_LANGUAGE_UNAVAILABLE */ -> "Dictation doesn’t support this language yet"
    else -> null
}
