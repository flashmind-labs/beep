package com.flashmind.beep

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaPlayer
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.NoiseSuppressor
import android.media.audiofx.Visualizer
import android.os.Build
import android.util.Log
import java.io.File
import kotlin.math.min
import kotlin.math.sqrt

/** Sample rate the live session takes: mono 16-bit PCM. */
private const val MIC_RATE = 16_000
/** About 100 ms of microphone sound a chunk. */
private const val MIC_CHUNK_BYTES = MIC_RATE / 10 * 2

/** The speeds Settings offers for Beep's voice, which plays faster at the same pitch. */
internal val VOICE_SPEEDS = listOf(1f, 1.25f, 1.5f, 2f)

/** How fast Beep's lines play on this phone. */
internal fun voiceSpeed(context: Context): Float =
    context.getSharedPreferences("live", Context.MODE_PRIVATE).getFloat("voiceSpeed", 1.25f)

internal fun setVoiceSpeed(context: Context, speed: Float) {
    context.getSharedPreferences("live", Context.MODE_PRIVATE).edit().putFloat("voiceSpeed", speed).apply()
}

/**
 * The audio side of live voice: the microphone in as 16 kHz mono PCM16, through the
 * communication path with echo cancellation so Beep's own voice is not heard back and "stop"
 * can be heard over it, and Beep's spoken lines out, one MP3 after another. Used from the main
 * thread; the microphone runs on its own thread and only calls the closure given to [start].
 */
internal class LiveAudio(private val context: Context) {
    class Failure(message: String) : Exception(message)

    private class Line(val id: ULong, val file: File, val done: () -> Unit)

    private val audio = context.getSystemService(AudioManager::class.java)
    private var record: AudioRecord? = null
    private var echo: AcousticEchoCanceler? = null
    private var noise: NoiseSuppressor? = null
    private var reader: Thread? = null
    @Volatile private var running = false
    private var player: MediaPlayer? = null
    private var visualizer: Visualizer? = null
    private var playing: Line? = null
    private val queue = ArrayDeque<Line>()
    private var previousMode = AudioManager.MODE_NORMAL
    private var scoStarted = false
    private var speakerSet = false
    private val voiceAttributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
        .build()

    /** Whether a line plays or waits to. */
    val busy get() = playing != null || queue.isNotEmpty()

    /**
     * Starts the microphone. [mic] gets each chunk of 16 kHz PCM16 on the microphone's thread;
     * [voice] gets the level of what plays, 0 to 1, on the main thread.
     */
    @SuppressLint("MissingPermission") // LiveVoice checks RECORD_AUDIO before starting.
    fun start(mic: (ByteArray) -> Unit, voice: (Float) -> Unit) {
        this.voice = voice
        previousMode = audio.mode
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        route()
        val size = maxOf(AudioRecord.getMinBufferSize(MIC_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT), MIC_CHUNK_BYTES * 2)
        val recorder = try {
            AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, MIC_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, size)
        } catch (error: Exception) {
            stop()
            throw Failure("No microphone")
        }
        if (recorder.state != AudioRecord.STATE_INITIALIZED) {
            recorder.release()
            stop()
            throw Failure("The microphone is in use by another app")
        }
        // The communication source usually cancels echo already; these make sure where the phone offers them.
        if (AcousticEchoCanceler.isAvailable()) echo = runCatching { AcousticEchoCanceler.create(recorder.audioSessionId)?.apply { enabled = true } }.getOrNull()
        if (NoiseSuppressor.isAvailable()) noise = runCatching { NoiseSuppressor.create(recorder.audioSessionId)?.apply { enabled = true } }.getOrNull()
        try {
            recorder.startRecording()
        } catch (error: Exception) {
            recorder.release()
            stop()
            throw Failure("The microphone is in use by another app")
        }
        record = recorder
        running = true
        reader = Thread({
            val chunk = ByteArray(MIC_CHUNK_BYTES)
            while (running) {
                val read = recorder.read(chunk, 0, chunk.size)
                if (read <= 0) {
                    if (read < 0) Log.w("LiveAudio", "microphone read failed: $read")
                    if (read < 0) break else continue
                }
                mic(if (read == chunk.size) chunk.copyOf() else chunk.copyOf(read))
            }
        }, "live-mic").apply { start() }
    }

    private var voice: (Float) -> Unit = {}

    /** Plays one MP3 line after any still playing; [done] runs on the main thread once it was heard or could not play. */
    fun play(id: ULong, mp3: ByteArray, done: () -> Unit) {
        val file = File(context.cacheDir, "live-$id.mp3")
        try {
            file.writeBytes(mp3)
        } catch (error: Exception) {
            done()
            return
        }
        queue.addLast(Line(id, file, done))
        if (playing == null) next()
    }

    private fun next() {
        val line = queue.removeFirstOrNull() ?: run { playing = null; voice(0f); return }
        playing = line
        val media = player ?: MediaPlayer().also { player = it }
        try {
            media.reset()
            media.setAudioAttributes(voiceAttributes)
            media.setDataSource(line.file.path)
            media.setOnCompletionListener { finish(line) }
            media.setOnErrorListener { _, what, extra ->
                Log.w("LiveAudio", "could not play a line: $what/$extra")
                finish(line)
                true
            }
            media.setOnPreparedListener {
                it.playbackParams = it.playbackParams.setSpeed(voiceSpeed(context))
                it.start()
                listen(it)
            }
            media.prepareAsync()
        } catch (error: Exception) {
            Log.w("LiveAudio", "could not play a line", error)
            finish(line)
        }
    }

    private fun finish(line: Line) {
        if (playing !== line) return
        line.file.delete()
        playing = null
        line.done()
        next()
    }

    /** Follows the level of what plays, for the orb. Without the visualizer the orb stays calm. */
    private fun listen(media: MediaPlayer) {
        if (visualizer != null) return
        visualizer = runCatching {
            Visualizer(media.audioSessionId).apply {
                captureSize = Visualizer.getCaptureSizeRange()[0].coerceAtLeast(256)
                setDataCaptureListener(object : Visualizer.OnDataCaptureListener {
                    override fun onWaveFormDataCapture(v: Visualizer?, wave: ByteArray?, rate: Int) {
                        if (wave != null && playing != null) voice(waveLevel(wave))
                    }
                    override fun onFftDataCapture(v: Visualizer?, fft: ByteArray?, rate: Int) {}
                }, Visualizer.getMaxCaptureRate() / 2, true, false)
                enabled = true
            }
        }.getOrNull()
    }

    /** Drops what plays and what is queued, without calling their `done`. */
    fun stopPlaying() {
        playing?.file?.delete()
        playing = null
        queue.forEach { it.file.delete() }
        queue.clear()
        runCatching { player?.stop() }
        voice(0f)
    }

    /** Stops the microphone and the player, and gives the audio route back. */
    fun stop() {
        running = false
        reader?.join(500)
        reader = null
        record?.let { runCatching { it.stop() }; it.release() }
        record = null
        echo?.release(); echo = null
        noise?.release(); noise = null
        stopPlaying()
        visualizer?.release(); visualizer = null
        player?.release(); player = null
        unroute()
        audio.mode = previousMode
    }

    /**
     * Sends the call audio to a headset when one is connected, or else to the loudspeaker:
     * the communication path would otherwise use the earpiece.
     */
    private fun route() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val devices = audio.availableCommunicationDevices
            val preferred = listOf(AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_WIRED_HEADSET,
                AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
            val device = preferred.firstNotNullOfOrNull { type -> devices.firstOrNull { it.type == type } }
            if (device != null) speakerSet = audio.setCommunicationDevice(device)
        } else {
            @Suppress("DEPRECATION")
            val headset = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }
            val wired = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any {
                it.type == AudioDeviceInfo.TYPE_WIRED_HEADSET || it.type == AudioDeviceInfo.TYPE_WIRED_HEADPHONES || it.type == AudioDeviceInfo.TYPE_USB_HEADSET
            }
            @Suppress("DEPRECATION")
            when {
                headset -> { audio.startBluetoothSco(); audio.isBluetoothScoOn = true; scoStarted = true }
                !wired -> { audio.isSpeakerphoneOn = true; speakerSet = true }
            }
        }
    }

    private fun unroute() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            if (speakerSet) audio.clearCommunicationDevice()
        } else {
            @Suppress("DEPRECATION")
            if (scoStarted) { audio.isBluetoothScoOn = false; audio.stopBluetoothSco() }
            @Suppress("DEPRECATION")
            if (speakerSet) audio.isSpeakerphoneOn = false
        }
        scoStarted = false
        speakerSet = false
    }
}

/** RMS of an unsigned 8-bit waveform, scaled to about 0 to 1 for speech. */
internal fun waveLevel(wave: ByteArray): Float {
    if (wave.isEmpty()) return 0f
    var sum = 0.0
    for (b in wave) {
        val v = ((b.toInt() and 0xFF) - 128) / 128.0
        sum += v * v
    }
    return min(1f, (sqrt(sum / wave.size) * 4).toFloat())
}
