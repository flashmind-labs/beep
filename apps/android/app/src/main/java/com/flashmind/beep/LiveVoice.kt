package com.flashmind.beep

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.core.content.ContextCompat
import com.flashmind.beep.ffi.LeaveReason
import com.flashmind.beep.ffi.LiveEvent
import com.flashmind.beep.ffi.LiveListener
import com.flashmind.beep.ffi.LivePhase
import com.flashmind.beep.ffi.LiveSession
import com.flashmind.beep.ffi.LiveStart
import com.flashmind.beep.ffi.LiveView
import com.flashmind.beep.ffi.ReplyFailure
import com.flashmind.beep.proto.Approval
import com.flashmind.beep.proto.RunEvent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Where live voice was started from. */
enum class LiveOrigin { App, Hold, Shortcut, Tile, Widget, Assist }

/** Where live mode is, for the dock and the notification. */
enum class LiveStage { Connecting, Listening, Holding, Thinking, Speaking, Waiting }

/** The approval reply [reply] waits on; its card is answered in the chat. */
data class LiveAsk(val reply: ULong, val queryId: String, val approval: Approval)

/** A pause that sends after [ms], counted from [from] in `SystemClock.elapsedRealtime`. */
data class LiveSoon(val from: Long, val ms: Long)

/** Why live voice cannot go on. A denied microphone gets a button to Settings. */
data class LiveProblem(val text: String, val microphone: Boolean = false)

/** What the live dock shows. */
data class LiveState(
    /** Whether live mode is on, so the dock shows. */
    val active: Boolean = false,
    /** Counts launches, so the dock plays its opening again for a new one. */
    val launches: Int = 0,
    val origin: LiveOrigin = LiveOrigin.App,
    val stage: LiveStage = LiveStage.Connecting,
    /** What was said so far this turn. */
    val words: String = "",
    /** Beep's sentence playing or played last, and the one before it. */
    val line: String? = null,
    val prev: String? = null,
    /** The tool step the reply is on. */
    val step: String? = null,
    /** Whether Beep's voice is playing. */
    val speaking: Boolean = false,
    val toast: String? = null,
    val soon: LiveSoon? = null,
    val problem: LiveProblem? = null,
    /** The last message sent from live voice, shown over the step while Beep thinks. */
    val said: String? = null,
    /** The chat live voice talks in; null for a new one, made with the first message. */
    val chatId: String? = null,
    /** The approval the reply waits on. */
    val ask: LiveAsk? = null,
    /** The reply's stream broke and is being fetched again. */
    val reconnecting: Boolean = false,
    /** Beep's voice could not be made, so its lines only show. */
    val voiceUnavailable: Boolean = false,
)

/** Why live mode ended, for the note over the composer. */
enum class LeaveCause { Closed, Quiet, Call, AudioTaken, Notification, Failed, Ended }

/** The cause for the note: what the app asked for, or else what the session said. */
internal fun leaveCause(asked: LeaveCause?, reason: LeaveReason): LeaveCause = asked ?: when (reason) {
    LeaveReason.ASKED -> LeaveCause.Closed
    LeaveReason.QUIET -> LeaveCause.Quiet
    LeaveReason.INTERRUPTED -> LeaveCause.Call
    LeaveReason.FAILED -> LeaveCause.Failed
}

/** How long a line with no voice stays up before the next one: about the time it takes to read. */
internal fun readingMs(text: String): Long = (900L + text.length * 55L).coerceAtMost(8_000L)

/** The note over the composer after live mode ended, in chat [chatId]. */
data class LeaveNote(val chatId: String, val text: String)

/** The note for leaving because of [reason]; [kept] says words not sent went back into the composer. */
internal fun leaveNoteText(reason: LeaveCause, kept: Boolean): String {
    val why = when (reason) {
        LeaveCause.Closed -> "Left live mode."
        LeaveCause.Quiet -> "Live mode ended after 2 minutes quiet."
        LeaveCause.Call -> "A call came in, so live mode ended."
        LeaveCause.AudioTaken -> "Another app took the audio, so live mode ended."
        LeaveCause.Notification -> "Stopped from the notification."
        LeaveCause.Failed -> "Voice is not available right now. Type here instead."
        LeaveCause.Ended -> "Live mode ended."
    }
    return if (kept) "$why Your words are kept here." else why
}

/** [draft] after the text already in the composer, so neither is lost. */
internal fun joinDraft(current: String, draft: String): String =
    if (current.isBlank()) draft.trim() else current.trimEnd() + " " + draft.trim()

/**
 * Live voice: talks with Beep through a `LiveSession` from the bridge, which turns what the
 * microphone hears into turns and reads the replies out. The session says what to send; this
 * sends it through the chat like a typed message and hands the run's events back, so the chat,
 * its approvals and its history work as usual. Lives in the application, so a session goes on
 * with the screen off; [LiveService] keeps the process in the foreground meanwhile.
 */
class LiveVoice(private val context: Context, private val repository: ChatRepository) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val mutableState = MutableStateFlow(LiveState())
    val state: StateFlow<LiveState> = mutableState.asStateFlow()
    /** The microphone's level while listening, Beep's voice while it speaks, 0 to 1. */
    val level = MutableStateFlow(0f)
    /** The note shown over the composer after leaving, until dismissed or it times out. */
    val leaveNote = MutableStateFlow<LeaveNote?>(null)

    private val prefs = context.getSharedPreferences("live", Context.MODE_PRIVATE)
    /** Whether a pause sends what was said, or only "send" does. */
    var sendOnPause: Boolean
        get() = prefs.getBoolean("sendOnPause", true)
        set(value) {
            prefs.edit().putBoolean("sendOnPause", value).apply()
            session?.setSendOnPause(value)
        }

    private var session: LiveSession? = null
    private var audio: LiveAudio? = null
    private var run: Job? = null
    private var events: Channel<LiveEvent>? = null
    private var toastJob: Job? = null
    private var leaveJob: Job? = null
    private var focus: AudioFocusRequest? = null
    /** Why the app asked the session to leave, for the note; null when it left on its own. */
    private var reason: LeaveCause? = null
    /** Lines with no voice, each shown for a while before the session hears it was played; in order. */
    private val readings = SupervisorJob()
    private var lastReading: Job? = null
    /** A hold that started before the session did: true while held, false once let go. */
    private var heldEarly: Boolean? = null
    private var holdCancelled = false
    private val audioManager = context.getSystemService(AudioManager::class.java)

    // Called from the runs' coroutines; the session takes calls from any thread.
    private val replies = object : ChatRepository.LiveReplies {
        override fun reply(reply: ULong, event: RunEvent) {
            session?.replyEvent(reply, event)
            if (mutableState.value.reconnecting) scope.launch { mutableState.update { it.copy(reconnecting = false) } }
        }
        override fun failed(reply: ULong, failure: ReplyFailure) {
            session?.replyFailed(reply, failure)
            scope.launch { mutableState.update { it.copy(reconnecting = false) } }
        }
        override fun reconnecting(reply: ULong) {
            session?.reconnecting(reply)
            scope.launch { mutableState.update { it.copy(reconnecting = true) } }
        }
        override fun answered(reply: ULong) {
            session?.approvalAnswered(reply)
            scope.launch { mutableState.update { if (it.ask?.reply == reply) it.copy(ask = null) else it } }
        }
        override fun cancelled() { scope.launch { session?.interrupt() } }
    }

    // Screen

    /** Opens live voice, or plays the opening again when it is open already. */
    fun start(origin: LiveOrigin, holding: Boolean = false) {
        if (!repository.settings.value.configured) return
        val current = mutableState.value
        if (current.active && current.problem == null) {
            mutableState.update { it.copy(origin = origin, launches = it.launches + 1) }
            if (holding) holdStart()
            return
        }
        end()
        val chat = repository.state.value.current
        heldEarly = if (holding) true else null
        holdCancelled = false
        reason = null
        leaveNote.value = null
        mutableState.value = LiveState(active = true, launches = current.launches + 1, origin = origin,
            stage = if (holding) LiveStage.Holding else LiveStage.Connecting,
            chatId = chat.id.takeIf { chat.messages.isNotEmpty() })
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            fail(LiveProblem("Beep can’t hear you. Allow the microphone for Beep in Settings.", microphone = true))
            return
        }
        LiveService.start(context)
        val channel = Channel<LiveEvent>(Channel.UNLIMITED)
        events = channel
        val listener = object : LiveListener {
            // Called from a background thread; the channel keeps the order.
            override fun onEvent(event: LiveEvent) { channel.trySend(event) }
        }
        run = scope.launch {
            val started = try {
                repository.startLive(listener, LiveStart(sendOnPause, holding))
            } catch (error: Exception) {
                fail(LiveProblem(userMessage(error)))
                return@launch
            }
            if (events !== channel) { started.stop(); started.destroy(); return@launch }
            session = started
            repository.liveReplies = replies
            if (!takeFocus()) {
                reason = LeaveCause.AudioTaken
                started.fail("Another app is using the audio. Try again when it stops.")
            } else try {
                startAudio(started)
            } catch (error: Exception) {
                started.fail(error.message ?: "The microphone is not working")
            }
            // A hold let go before the session started still sends.
            if (heldEarly == false) { if (holdCancelled) started.holdCancel() else started.holdEnd() }
            heldEarly = null
            for (event in channel) handle(event)
        }
    }

    /** The close button: ends the session, keeping what was said and not sent as a draft. */
    fun close(why: LeaveCause = LeaveCause.Closed) {
        val live = session
        if (live != null) {
            reason = why
            live.stop()
            // The session answers with Left; if it never does, leave anyway.
            leaveJob?.cancel()
            leaveJob = scope.launch { delay(2_000); if (session === live) { end(); mutableState.update { it.copy(active = false) } } }
        } else {
            end()
            mutableState.update { LiveState(launches = it.launches) }
        }
    }

    /** Tries again after a problem. */
    fun retry() {
        val origin = mutableState.value.origin
        close()
        start(origin)
    }

    /** A tap on the orb: stops listening when listening, otherwise skips the answer. */
    fun tap() { session?.tap() }

    /** The Cancel control: stops Beep speaking, or cancels the reply it is working on. */
    fun cancelReply() { session?.interrupt() }

    fun holdStart() {
        val live = session
        if (live == null) { heldEarly = true; holdCancelled = false } else live.holdStart()
    }
    fun holdEnd() {
        val live = session
        if (live == null) heldEarly = false else live.holdEnd()
    }
    fun holdCancel() {
        val live = session
        if (live == null) { heldEarly = false; holdCancelled = true } else live.holdCancel()
    }

    fun dismissNote() { leaveNote.value = null }

    // Session

    private fun startAudio(live: LiveSession) {
        val sound = LiveAudio(context)
        sound.start(mic = { pcm ->
            val heard = live.pushAudio(pcm)
            if (!mutableState.value.speaking) level.value = level.value * 0.5f + heard * 0.5f
        }, voice = { value ->
            if (mutableState.value.speaking) level.value = level.value * 0.5f + value * 0.5f
        })
        audio = sound
    }

    private fun handle(event: LiveEvent) {
        val live = session ?: return
        when (event) {
            is LiveEvent.View -> {
                val view = event.view
                mutableState.update { show(it, view) }
            }
            is LiveEvent.Toast -> show(event.text)
            is LiveEvent.Soon -> mutableState.update { it.copy(soon = event.afterMs?.let { ms -> LiveSoon(SystemClock.elapsedRealtime(), ms.toLong()) }) }
            is LiveEvent.Send -> {
                val chat = if (event.newChat) null else mutableState.value.chatId
                repository.liveApproval = null
                val sent = repository.sendSpoken(event.text, chat, event.reply)
                mutableState.update { it.copy(said = event.text, soon = null, chatId = sent, ask = null, reconnecting = false) }
            }
            is LiveEvent.CancelReply -> {
                mutableState.update { it.copy(ask = null, reconnecting = false) }
                repository.liveApproval = null
                mutableState.value.chatId?.let { repository.cancelLive(it, event.reply, event.runId) }
            }
            is LiveEvent.Approval -> {
                val chat = mutableState.value.chatId
                if (chat != null) repository.liveApproval = ChatRepository.LiveApproval(chat, event.queryId, event.reply)
                mutableState.update { it.copy(ask = LiveAsk(event.reply, event.queryId, event.approval)) }
            }
            is LiveEvent.Play -> {
                val sound = audio
                when {
                    // No voice for this line: it shows for as long as it takes to read.
                    event.audio.isEmpty() || sound == null -> {
                        val previous = lastReading
                        lastReading = scope.launch(readings) {
                            previous?.join()
                            delay(if (event.filler) 300 else readingMs(event.text))
                            session?.played(event.id)
                        }
                    }
                    else -> sound.play(event.id, event.audio) { session?.played(event.id) }
                }
            }
            is LiveEvent.StopAudio -> { audio?.stopPlaying(); readings.cancelChildren(); lastReading = null }
            is LiveEvent.Left -> left(event.draft, event.reason, event.failure)
        }
    }

    private fun show(state: LiveState, view: LiveView): LiveState {
        val stage = when (view.phase) {
            LivePhase.CONNECTING -> LiveStage.Connecting
            LivePhase.LISTENING -> LiveStage.Listening
            LivePhase.HOLDING -> LiveStage.Holding
            LivePhase.THINKING -> LiveStage.Thinking
            LivePhase.SPEAKING -> LiveStage.Speaking
            LivePhase.WAITING -> LiveStage.Waiting
        }
        return state.copy(stage = stage, words = view.words, line = view.line, prev = view.prev, step = view.step,
            speaking = view.speaking, voiceUnavailable = view.voiceUnavailable,
            soon = if (stage == LiveStage.Listening) state.soon else null,
            ask = if (stage == LiveStage.Waiting) state.ask else null)
    }

    private fun left(draft: String?, said: LeaveReason, failure: String?) {
        repository.liveApproval = null
        val why = leaveCause(reason, said)
        val target = mutableState.value.chatId ?: repository.state.value.current.id
        val kept = !draft.isNullOrBlank()
        if (kept) {
            val old = repository.draft(target)
            repository.setDraft(target, old.copy(text = joinDraft(old.text, draft!!)))
        }
        end()
        if (failure != null) {
            mutableState.update { it.copy(problem = LiveProblem(failure), soon = null, toast = null) }
        } else {
            mutableState.update { LiveState(launches = it.launches) }
            note(target, leaveNoteText(why, kept))
        }
    }

    private fun note(chatId: String, text: String) {
        val shown = LeaveNote(chatId, text)
        leaveNote.value = shown
        scope.launch { delay(10_000); if (leaveNote.value === shown) leaveNote.value = null }
    }

    private fun show(text: String) {
        mutableState.update { it.copy(toast = text) }
        toastJob?.cancel()
        toastJob = scope.launch { delay(1_600); mutableState.update { it.copy(toast = null) } }
    }

    /** Leaves the dock up with why live voice stopped; the close button dismisses it. */
    private fun fail(problem: LiveProblem) {
        end()
        mutableState.update { it.copy(active = true, problem = problem, soon = null) }
    }

    /** Stops the audio and lets go of the session. */
    private fun end() {
        leaveJob?.cancel()
        leaveJob = null
        readings.cancelChildren()
        lastReading = null
        audio?.stop()
        audio = null
        giveFocus()
        if (repository.liveReplies === replies) repository.liveReplies = null
        events?.close()
        events = null
        run = null
        session?.let { live ->
            session = null
            // Dropping the last reference leaves live mode, if it had not yet.
            runCatching { live.destroy() }
        }
        level.value = 0f
        LiveService.stop(context)
    }

    // Audio focus

    private val focusHandler = Handler(Looper.getMainLooper())

    private fun takeFocus(): Boolean {
        val attributes = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build()
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
            .setAudioAttributes(attributes)
            .setOnAudioFocusChangeListener({ change -> focusChanged(change) }, focusHandler)
            .build()
        focus = request
        return audioManager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    /** A call or another app took the audio: the session leaves, keeping what was said. */
    private fun interrupted(why: LeaveCause) {
        val live = session ?: return
        reason = why
        live.interrupted()
        leaveJob?.cancel()
        leaveJob = scope.launch { delay(2_000); if (session === live) { end(); mutableState.update { it.copy(active = false) } } }
    }

    private fun giveFocus() {
        focus?.let { audioManager.abandonAudioFocusRequest(it) }
        focus = null
    }

    private fun focusChanged(change: Int) {
        if (session == null) return
        val inCall = audioManager.mode == AudioManager.MODE_IN_CALL || audioManager.mode == AudioManager.MODE_RINGTONE ||
            audioManager.mode == AudioManager.MODE_CALL_SCREENING
        when {
            inCall -> interrupted(LeaveCause.Call)
            change == AudioManager.AUDIOFOCUS_LOSS -> interrupted(LeaveCause.AudioTaken)
            // Something speaks for a moment, such as directions: Beep stops talking and listens.
            change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> session?.interrupt()
        }
    }
}
