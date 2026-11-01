package com.flashmind.beep

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.flashmind.beep.ffi.ReplyVoice
import com.flashmind.beep.ffi.ReplyVoiceEvent
import com.flashmind.beep.ffi.ReplyVoiceListener
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import java.io.File

/**
 * Reads a chat reply aloud while it streams, for the composer's speak button. The server voices
 * the reply sentence by sentence; this plays each MP3 in turn at the voice speed from Settings.
 * Used from the main thread.
 */
internal class ReplySpeaker(private val context: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val audio = context.getSystemService(AudioManager::class.java)
    private val attributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ASSISTANT)
        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
        .build()
    private var starting: Job? = null
    private var voice: ReplyVoice? = null
    private var player: MediaPlayer? = null
    private var file: File? = null
    private var focus: AudioFocusRequest? = null
    /** Text handed over before the reading started, and whether the reply already ended. */
    private var text = ""
    private var finished = false
    /** Counts readings, so events from one that was stopped are dropped. */
    private var reading = 0

    /** Whether a reading runs: from the tap until every sentence played or it was stopped. */
    var speaking by mutableStateOf(false)
        private set

    fun start() {
        if (speaking) return
        val client = BeepClients.current() ?: return
        speaking = true
        text = ""
        finished = false
        takeFocus()
        val current = ++reading
        starting = scope.launch {
            val started = try {
                client.speakReply(object : ReplyVoiceListener {
                    override fun onEvent(event: ReplyVoiceEvent) { scope.launch { if (reading == current) heard(event) } }
                })
            } catch (error: Exception) {
                Log.w("ReplySpeaker", "could not start reading the reply", error)
                stop()
                return@launch
            }
            if (!speaking) { started.destroy(); return@launch }
            voice = started
            if (text.isNotEmpty()) started.update(text)
            if (finished) started.finish()
        }
    }

    /** The reply's text so far; only what is new gets read. */
    fun update(text: String) {
        if (!speaking || text.isEmpty()) return
        this.text = text
        voice?.update(text)
    }

    /** The reply ended: what is left is read, then the reading stops by itself. */
    fun finish() {
        if (!speaking || finished) return
        finished = true
        voice?.finish()
    }

    fun stop() {
        if (!speaking) return
        speaking = false
        reading++
        starting?.cancel()
        starting = null
        voice?.let { runCatching { it.destroy() } }
        voice = null
        player?.release()
        player = null
        file?.delete()
        file = null
        focus?.let { audio.abandonAudioFocusRequest(it) }
        focus = null
    }

    /** Stops for good, when the screen that owns it goes. */
    fun destroy() {
        stop()
        scope.cancel()
    }

    private fun heard(event: ReplyVoiceEvent) {
        if (!speaking) return
        when (event) {
            is ReplyVoiceEvent.Play -> play(event.id, event.audio)
            ReplyVoiceEvent.Done -> stop()
        }
    }

    private fun play(id: ULong, mp3: ByteArray) {
        // An empty line is one the server could not voice: skip it.
        if (mp3.isEmpty()) { voice?.played(id); return }
        val line = File(context.cacheDir, "reply-voice-$id.mp3")
        val done = {
            line.delete()
            if (file == line) file = null
            voice?.played(id)
        }
        try {
            line.writeBytes(mp3)
            file = line
            val media = player ?: MediaPlayer().also { player = it }
            media.reset()
            media.setAudioAttributes(attributes)
            media.setDataSource(line.path)
            media.setOnCompletionListener { done() }
            media.setOnErrorListener { _, what, extra ->
                Log.w("ReplySpeaker", "could not play a sentence: $what/$extra")
                done()
                true
            }
            media.setOnPreparedListener {
                it.playbackParams = it.playbackParams.setSpeed(voiceSpeed(context))
                it.start()
            }
            media.prepareAsync()
        } catch (error: Exception) {
            Log.w("ReplySpeaker", "could not play a sentence", error)
            done()
        }
    }

    /** Other sound ducks while the reply is read; if another app takes the audio for good, reading stops. */
    private fun takeFocus() {
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
            .setAudioAttributes(attributes)
            .setOnAudioFocusChangeListener { change -> if (change == AudioManager.AUDIOFOCUS_LOSS) scope.launch { stop() } }
            .build()
        focus = request
        audio.requestAudioFocus(request)
    }
}
