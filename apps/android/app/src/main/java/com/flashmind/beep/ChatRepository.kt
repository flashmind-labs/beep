package com.flashmind.beep

import android.Manifest
import android.app.Application
import android.content.pm.PackageManager
import android.location.Location
import android.os.SystemClock
import android.provider.ContactsContract
import androidx.compose.runtime.mutableStateMapOf
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.ffi.ListCache
import com.flashmind.beep.ffi.Rerun
import com.flashmind.beep.ffi.identityName
import com.flashmind.beep.proto.AppIntegration
import com.flashmind.beep.proto.ApprovalDecision
import com.flashmind.beep.proto.BrowserCommand
import com.flashmind.beep.proto.LinkedDevice
import com.flashmind.beep.proto.DeviceAnswer
import com.flashmind.beep.proto.AppPermissions
import com.flashmind.beep.proto.Models
import com.flashmind.beep.proto.ReasoningLevel
import com.flashmind.beep.proto.DeviceQuery
import com.flashmind.beep.proto.PermissionLevel
import com.flashmind.beep.proto.Event
import com.flashmind.beep.proto.Group
import com.flashmind.beep.proto.GroupChat
import com.flashmind.beep.proto.GroupLink
import com.flashmind.beep.proto.GroupMessage
import com.flashmind.beep.proto.GroupPreview
import com.flashmind.beep.proto.GroupReminder
import com.flashmind.beep.proto.HistoryEntry
import com.flashmind.beep.proto.MemorySource
import com.flashmind.beep.proto.Cron as ProtoCron
import com.flashmind.beep.proto.Memory as ProtoMemory
import com.flashmind.beep.proto.PromptSuggestion
import com.flashmind.beep.proto.RunEvent
import com.flashmind.beep.proto.ScrollDirection
import com.flashmind.beep.proto.SessionMatch
import com.flashmind.beep.proto.SharedLink
import com.flashmind.beep.proto.SharedSession
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.channels.Channel

class BeepApplication : Application() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    val repository by lazy {
        ChatRepository(LocalStorage(this)).also {
            followFace(it, scope)
            RunService.follow(this, it, scope)
        }
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
        Avatars.apply(this)
        NetworkWatch.start(this, scope)
        // The bubble stays up across process restarts until the user drops it on the X.
        if (Bubble.wanted(this)) Bubble.show(this)
    }

    companion object {
        var instance: BeepApplication? = null
            private set
    }
}

/** How a localhost OAuth flow ended; `error` is null on success. */
data class AppAuthResult(val slug: String, val error: String?)

data class AppIntent(val app: String, val uri: String, val fallbackUrl: String)

/** An alarm for the Clock app. `days` are java.util.Calendar weekdays, empty for a one-off. */
data class AlarmRequest(val hour: Int, val minute: Int, val label: String, val days: List<Int>) {
    val time get() = "%d:%02d".format(hour, minute)

    companion object {
        private val weekdays = listOf("sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday")

        fun from(json: JSONObject): AlarmRequest? {
            val hour = json.optInt("hour", -1).takeIf { it in 0..23 } ?: return null
            val minute = json.optInt("minute", -1).takeIf { it in 0..59 } ?: return null
            val list = json.optJSONArray("days")
            val days = (0 until (list?.length() ?: 0)).mapNotNull { i -> weekdays.indexOf(list!!.optString(i)).takeIf { it >= 0 }?.plus(1) }
            return AlarmRequest(hour, minute, json.optString("label"), days)
        }
    }
}

/** How long a chat's turns stay on the phone after it was last opened. */
private const val CACHE_FOR_MS = 72 * 60 * 60 * 1000L
/** New starters the user may ask for within the window, as the server allows. */
private const val STARTER_REFRESHES = 3
private const val STARTER_REFRESH_WINDOW_MS = 10 * 60 * 1000L
/** What a chat shows while the phone has no network: an unsent message goes out once it is back. */
private fun waiting(chat: Conversation) = if (chat.runId.isEmpty()) "Sends when you’re back online" else "Waiting for a connection…"
/** Roles of the messages that start a turn. */
private val TURN_STARTS = setOf("user", "scheduled")

class ChatRepository(private val storage: LocalStorage) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val lock = Any()
    private val jobs = mutableMapOf<String, Job>()
    private val mutableState = MutableStateFlow(evictUnopened(storage.loadState()))
    val state = mutableState.asStateFlow()
    val settings = MutableStateFlow(storage.loadSettings())
    /** The server's `updated_at` of each chat, as last listed. */
    private val serverVersions = ConcurrentHashMap<String, Long>()
    /** Asks for a sync of the chat list; several asks while one runs make one more. */
    private val syncRequests = Channel<Unit>(Channel.CONFLATED).also { requests ->
        scope.launch { for (request in requests) runCatching { syncHistory(settings.value) } }
    }
    private var lists = storage.loadLists()
    val connection = MutableStateFlow(if (settings.value.configured) "Checking connection…" else "No server in this build")
    val connected = MutableStateFlow(false)
    /** Whether this phone has no Firebase, as on phones without Google Play services; null until a connection tells. */
    val pushMissing = MutableStateFlow<Boolean?>(null)
    val pendingIntent = MutableStateFlow<AppIntent?>(null)
    val pendingAlarm = MutableStateFlow<AlarmRequest?>(null)
    val browserOpen = MutableStateFlow(false)
    private var reconnectJob: Job? = null
    /** False while the phone has no working network, so runs wait instead of spinning. */
    @Volatile private var networkUp = true
    /** Renames, pins and deletes not yet on the server, oldest first. Guarded by [lock]. */
    private var changes = storage.loadChanges()
    private val pushing = Mutex()
    /** What is typed in each chat but not sent, shared by the app, the bubble and the panel. Only the text is saved. */
    private val drafts = mutableStateMapOf<String, Draft>().apply { storage.loadDrafts().forEach { (id, text) -> put(id, Draft(text)) } }
    fun draft(id: String): Draft = drafts[id] ?: Draft()
    fun setDraft(id: String, draft: Draft) {
        if (draft == Draft()) drafts.remove(id) else drafts[id] = draft
        storage.saveDrafts(drafts.mapValues { it.value.text }.filterValues { it.isNotEmpty() })
    }
    private var locationJob: Job? = null
    /** Chat screens currently started: the app, the bubble or the panel. */
    private val chatScreens = AtomicInteger(0)
    fun chatScreenShown(shown: Boolean) {
        if (!shown) { chatScreens.decrementAndGet(); return }
        chatScreens.incrementAndGet()
        // The chat on screen has been read, so its notification goes.
        ChatNotifications.cancel(storage.context, state.value.current.id)
    }
    fun onScreen(id: String) = chatScreens.get() > 0 && state.value.current.id == id
    private val cloud = CloudBackup(storage.context)
    /** The readable name of this phone's identity, such as mega-fox-trot. */
    val accountName = MutableStateFlow("")
    /** Whether the identity is in the Google account backup; null until known. */
    val backedUp = MutableStateFlow<Boolean?>(null)
    init {
        scope.launch {
            restoreIdentity()
            accountName.value = identityName(storage.identity())
            if (settings.value.configured) reconnect()
        }
    }

    private fun api(config: ConnectionSettings = settings.value, identity: ByteArray = storage.identity()): BeepClient =
        BeepClients.client(config, identity)

    /** On a new phone, takes the identity back from the Google account backup. */
    private suspend fun restoreIdentity() {
        if (storage.storedIdentity() != null) return
        val backup = cloud.load() ?: return
        storage.saveIdentity(backup.identity)
    }

    private fun backUp() {
        scope.launch { backedUp.value = cloud.save(storage.identity(), settings.value.nodeId) }
    }

    /** The server and this phone's identity, for a user to keep when the Google backup is not available. */
    fun exportProfile(): String = Profile(settings.value, storage.identity()).toJson()

    private fun update(transform: (ChatState) -> ChatState) = synchronized(lock) {
        val next = transform(mutableState.value).let { s ->
            val kept = s.conversations.filter { !it.isUnused || it.id == s.selected }
            if (kept.isEmpty()) s else s.copy(conversations = kept)
        }
        storage.saveState(next) // Cursor and text are committed atomically, before publishing either.
        mutableState.value = next
    }
    private fun updateChat(id: String, transform: (Conversation) -> Conversation) = update { s ->
        s.copy(conversations = s.conversations.map { if (it.id == id) transform(it) else it })
    }
    fun select(id: String) {
        // A routine's chat may be new to the phone; it gets its title and turns from the server.
        val known = state.value.conversations.any { it.id == id }
        val now = System.currentTimeMillis()
        update { s ->
            if (s.conversations.any { it.id == id }) s.copy(selected = id, conversations = s.conversations.map { if (it.id == id) it.copy(openedAt = now) else it })
            else s.copy(conversations = listOf(Conversation(id = id)) + s.conversations, selected = id)
        }
        ChatNotifications.cancel(storage.context, id)
        if (known) scope.launch { runCatching { loadHistory(id, settings.value) } } else refreshHistory()
    }
    /** Whether the phone holds the chat's turns as the server last listed them. */
    private fun cached(chat: Conversation) = chat.messages.isNotEmpty() && chat.version != 0L && serverVersions[chat.id] == chat.version
    private suspend fun loadHistory(id: String, config: ConnectionSettings) {
        val before = state.value.conversations.find { it.id == id } ?: return
        if (before.busy || cached(before)) return
        // Taken before the fetch: turns saved meanwhile only make the next sync fetch again.
        val version = serverVersions[id] ?: 0L
        // A turn where the model only called tools has no text, and would show as an empty
        // reply, so its tools and reasoning move onto the next reply that has text.
        var pending = emptyList<ToolCall>()
        var pendingReasoning = ""
        val messages = api(config).getSession(id).mapIndexedNotNull { i, entry ->
            when (entry) {
                is HistoryEntry.User -> { pending = emptyList(); pendingReasoning = ""; ChatMessage("history-$id-$i", "user", entry.content, sentAt = entry.timestamp,
                    autoModel = entry.auto?.model.orEmpty(), autoThinking = entry.auto?.thinking?.name?.lowercase().orEmpty()) }
                is HistoryEntry.Scheduled -> { pending = emptyList(); pendingReasoning = ""; ChatMessage("history-$id-$i", "scheduled", entry.label, sentAt = entry.timestamp) }
                is HistoryEntry.Assistant -> {
                    val tools = pending + entry.tools.map {
                        ToolCall(it.callId, it.name, toolLabel(it.name), isDone = true, success = it.success, result = it.output)
                    }
                    val reasoning = joinReasoning(pendingReasoning, entry.reasoning)
                    if (entry.content.isBlank()) { pending = tools; pendingReasoning = reasoning; null }
                    else {
                        pending = emptyList(); pendingReasoning = ""
                        ChatMessage("history-$id-$i", "assistant", entry.content, toolCalls = tools, memoryScore = entry.memoryScore, reasoning = reasoning, model = entry.model)
                    }
                }
                // The session stopped to ask before a call: the card takes the tools held back for it.
                is HistoryEntry.Approval -> {
                    val message = ChatMessage("history-$id-$i", "assistant", "", toolCalls = pending, reasoning = pendingReasoning,
                        approvals = listOf(ApprovalPrompt(entry.queryId, entry.approval)))
                    pending = emptyList(); pendingReasoning = ""
                    message
                }
            }
        }
        if (settings.value != config) return
        // Turns from before the server kept Jev scores only have the one the phone saw live.
        val scores = before.messages.filter { it.memoryScore != null }.associate { it.text to it.memoryScore }
        // Messages already on screen keep their ids, or the list drops and re-adds every row while it is drawn.
        // They keep their images too: the server swaps them for descriptions and never sends them back.
        val kept = before.messages.groupBy { it.role to it.text }.mapValues { ArrayDeque(it.value) }
        val scored = messages.map { m -> if (m.memoryScore != null) m else scores[m.text]?.let { m.copy(memoryScore = it) } ?: m }
            .map { m -> kept[m.role to m.text]?.removeFirstOrNull()?.let { m.copy(id = it.id, images = it.images) } ?: m }
        updateChat(id) { current -> if (current == before) current.copy(messages = scored, version = version) else current }
    }
    fun refreshHistory() { syncRequests.trySend(Unit) }
    /** Fetches the chat a notification is about, so tapping it opens the chat at once. */
    fun prefetch(id: String) {
        if (id.isEmpty() || !settings.value.configured) return
        scope.launch {
            runCatching {
                val config = settings.value
                // Lists the chat if it is new, and learns that it changed if not.
                syncHistory(config)
                // Counts as opened, or the next sync forgets its turns before the tap.
                updateChat(id) { it.copy(openedAt = System.currentTimeMillis()) }
                loadHistory(id, config)
            }
        }
    }
    private suspend fun syncHistory(config: ConnectionSettings) {
        val sessions = api(config).listSessions()
        if (settings.value != config) return
        sessions.forEach { serverVersions[it.id] = it.updatedAt }
        update { state ->
            val known = state.conversations.map { it.id }.toSet()
            val server = sessions.associateBy { it.id }
            val added = sessions.filter { it.id !in known }
                .map { Conversation(id = it.id, title = it.title, pinned = it.pinned, updatedAt = it.updatedAt) }
            // The server owns titles and pins of chats it knows.
            val updated = state.conversations.map { c ->
                server[c.id]?.let { c.copy(title = it.title, pinned = it.pinned, updatedAt = it.updatedAt) } ?: c
            }
            withChanges(evictUnopened(state.copy(conversations = (updated + added).sortedByDescending { it.pinned })))
        }
        dropDeleted(config, sessions.map { it.id }.toSet())
        // Chats opened in the last 72 hours stay cached, so they open at once and offline.
        val listed = sessions.map { it.id }.toSet()
        state.value.conversations
            .filter { it.id in listed && (it.id == state.value.current.id || it.messages.isNotEmpty()) && !cached(it) }
            .forEach { runCatching { loadHistory(it.id, config) } }
    }
    /**
     * Drops the chats another phone deleted. The list stops at 200 chats, so
     * the server says which of the others still exist.
     */
    private suspend fun dropDeleted(config: ConnectionSettings, listed: Set<String>) {
        val unlisted = state.value.conversations
            .filter { it.id !in listed && it.version != 0L && !it.busy && it.pendingText.isEmpty() }
            .map { it.id }
        if (unlisted.isEmpty()) return
        val existing = unlisted.chunked(1000).flatMap { api(config).existingSessions(it) }.toSet()
        if (settings.value != config) return
        val gone = unlisted.filter { it !in existing }
        if (gone.isNotEmpty()) update { applyChange(it, ChatChange.Delete(gone)) }
    }
    /**
     * Forgets the turns of chats not opened for 72 hours. They stay listed
     * and come back from the server when opened.
     */
    private fun evictUnopened(state: ChatState): ChatState {
        val cutoff = System.currentTimeMillis() - CACHE_FOR_MS
        return state.copy(conversations = state.conversations.map { c ->
            val keep = c.openedAt >= cutoff || c.id == state.selected || c.version == 0L || c.busy || c.pendingText.isNotEmpty()
            if (keep) c else c.copy(messages = emptyList(), version = 0)
        })
    }
    fun newChat() = update { state ->
        val chat = Conversation(); state.copy(conversations = listOf(chat) + state.conversations, selected = chat.id)
    }
    /** Opens a new chat with `text` typed in, for the user to finish and send. */
    fun newChatWithDraft(text: String) {
        newChat()
        setDraft(state.value.current.id, Draft(text))
    }
    /** Signs this phone in as the account in an imported key file, in place of its own key. */
    suspend fun importIdentity(identity: ByteArray) {
        if (identity.contentEquals(storage.identity())) return
        try { api(identity = identity).health() } catch (error: Exception) { throw IllegalStateException(userMessage(error), error) }
        reconnectJob?.cancel()
        synchronized(lock) { jobs.values.forEach { it.cancel() }; jobs.clear() }
        storage.saveIdentity(identity)
        accountName.value = identityName(identity)
        synchronized(lock) { changes = emptyList(); storage.saveChanges(changes) }
        serverVersions.clear()
        saveLists { ListCache(null, null, null, null) }
        modelChoices.value = null
        update { ChatState() }
        starters.value = emptyList()
        loadStarters()
        reconnect()
    }
    fun reconnect() {
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            var attempt = 0
            while (isActive) {
                try {
                    connection.value = if (attempt > 0) "Reconnecting… (attempt ${attempt + 1})" else "Checking connection…"
                    api().health()
                    networkUp = true
                    connected.value = true
                    scope.launch { reportTimezone() }
                    scope.launch {
                        // Online now, so no token means no Firebase on this phone.
                        val token = PushService.token()
                        pushMissing.value = token == null
                        token?.let { registerPushToken(it) }
                    }
                    connection.value = "Connected"
                    backUp()
                    resumePending()
                    pushChanges()
                    runCatching { syncHistory(settings.value) }
                    refreshGroups()
                    return@launch
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    attempt++
                    connected.value = false
                    connection.value = when {
                        error is com.flashmind.beep.ffi.BeepException.Unauthorized -> userMessage(error)
                        attempt < 5 -> "Reconnecting…"
                        else -> "Can’t reach your server · tap to retry"
                    }
                    if (attempt >= 10) return@launch
                    delay(minOf(1000L * attempt, 15000L))
                }
            }
        }
    }
    /**
     * The phone has no working network. Runs stop waiting on a dead connection and show that
     * they wait; each resumes from its cursor once [networkBack] reconnects.
     */
    fun networkLost() {
        networkUp = false
        reconnectJob?.cancel()
        connected.value = false
        connection.value = "No internet"
        synchronized(lock) { jobs.values.forEach { it.cancel() }; jobs.clear() }
        state.value.conversations.filter { it.busy }.forEach { chat -> updateChat(chat.id) { it.copy(status = waiting(it), error = "") } }
    }
    /** A working network is back: reconnect, which resumes waiting runs. */
    fun networkBack() {
        networkUp = true
        if (!connected.value && settings.value.configured) reconnect()
    }
    private fun resumePending() { state.value.conversations.filter { it.busy }.forEach { launchRun(it.id) } }
    fun send(text: String, images: List<String> = emptyList(), conversationId: String = state.value.current.id, model: String = "", thinking: String = "") {
        // A routine's chat may not be listed here yet.
        if (state.value.conversations.none { it.id == conversationId })
            update { it.copy(conversations = it.conversations + Conversation(id = conversationId, title = text.trim().take(44))) }
        val chat = state.value.conversations.first { it.id == conversationId }
        if (text.isBlank() || chat.busy || !settings.value.configured) return
        startTurn(chat, text, images, model, thinking)
        launchRun(chat.id)
    }
    /** Shows [text] as the user's next prompt and marks the chat as sending it. */
    private fun startTurn(chat: Conversation, text: String, images: List<String>, model: String, thinking: String = "") {
        val request = UUID.randomUUID().toString()
        // A new message instead of an answer skips the call that waits on it.
        val skipped = chat.messages.flatMap { m -> m.approvals.filter { it.waiting } }
        skipped.forEach { ApprovalNotifications.cancel(storage.context, it.queryId) }
        updateChat(chat.id) { it.copy(title = if (it.messages.isEmpty()) text.take(44) else it.title,
            messages = skipApprovals(it.messages) + ChatMessage(role = "user", text = text.trim(), images = images, model = model, modelThinking = thinking),
            requestId = request, pendingText = text.trim(), pendingImages = images, pendingModel = model, pendingThinking = thinking,
            status = "Sending…", error = "", cancelRequested = false) }
    }
    /** Holds a message typed while [chatId]'s reply runs, added to any already held, to send when it finishes. */
    fun queue(text: String, images: List<String>, chatId: String = state.value.current.id) {
        updateChat(chatId) { it.copy(queuedText = listOf(it.queuedText, text.trim()).filter { t -> t.isNotBlank() }.joinToString("\n\n"),
            queuedImages = (it.queuedImages + images).take(MAX_IMAGES)) }
    }
    /** Drops [chatId]'s queued message and returns it, so it can go back into the composer. */
    fun unqueue(chatId: String = state.value.current.id): Pair<String, List<String>> {
        val chat = state.value.conversations.find { it.id == chatId } ?: return "" to emptyList()
        updateChat(chatId) { it.copy(queuedText = "", queuedImages = emptyList()) }
        return chat.queuedText to chat.queuedImages
    }
    /** Sends [chatId]'s queued message now, after a run that ended without finishing. */
    fun sendQueued(chatId: String = state.value.current.id) {
        val chat = state.value.conversations.find { it.id == chatId } ?: return
        if (chat.busy || !chat.hasQueued) return
        val (text, images) = unqueue(chatId)
        send(text.ifBlank { if (images.size > 1) "What are these?" else "What is this?" }, images, chatId)
    }
    /**
     * Starts [chat]'s queued message once its reply finished cleanly. A run that stopped,
     * failed or waits on an approval keeps it, so nothing goes out on its own.
     */
    private fun startQueued(chat: Conversation): Boolean {
        if (!chat.hasQueued || chat.error.isNotEmpty() || !settings.value.configured) return false
        if (chat.messages.any { m -> m.approvals.any { it.waiting } }) return false
        val (text, images) = unqueue(chat.id)
        startTurn(chat, text.ifBlank { if (images.size > 1) "What are these?" else "What is this?" }, images, "")
        return true
    }
    /**
     * Sends the prompt behind [reply] again on another model, as if the first try never
     * happened: its turn and every turn after it leave the chat and the model's context.
     * Admins only.
     */
    suspend fun rerun(reply: ChatMessage, model: String, thinking: String = "") {
        val chat = state.value.current
        val prompt = chat.messages.take(chat.messages.indexOf(reply)).lastOrNull { it.role in TURN_STARTS } ?: return
        rerunPrompt(prompt, model, thinking)
    }
    /**
     * Deletes the turn [prompt] started and everything after it, then sends it again on [model],
     * thinking at [thinking] ("off" to "high") or, when empty, the admin's usual level.
     */
    suspend fun rerunPrompt(prompt: ChatMessage, model: String, thinking: String = "") {
        if (prompt.role != "user") return
        val chat = state.value.current
        deleteTurn(prompt, chat.id, onward = true)
        send(prompt.text, prompt.images, chat.id, model, thinking)
    }
    /**
     * Removes [prompt] with the reply and tool calls that followed it, here and on the server.
     * With [onward], every later turn goes too.
     */
    suspend fun deleteTurn(prompt: ChatMessage, chatId: String = state.value.current.id, onward: Boolean = false) {
        val chat = state.value.conversations.find { it.id == chatId } ?: return
        check(!chat.busy) { "Wait for the reply to finish." }
        try {
            val sentAt = withContext(Dispatchers.IO) { serverTime(prompt, chat) }
            if (sentAt != 0L) withContext(Dispatchers.IO) { api().deleteTurn(chatId, sentAt, onward) }
        } catch (error: Exception) { throw IllegalStateException(userMessage(error), error) }
        updateChat(chatId) { c ->
            val start = c.messages.indexOfFirst { it.id == prompt.id }
            if (start < 0) return@updateChat c
            val end = if (onward) c.messages.size
                else c.messages.drop(start + 1).indexOfFirst { it.role in TURN_STARTS }.let { if (it < 0) c.messages.size else start + 1 + it }
            c.copy(messages = c.messages.take(start) + c.messages.drop(end))
        }
    }
    /**
     * The server's time for [prompt], 0 when the server never saved it. A prompt sent from
     * this phone has none until the chat reloads, so it is the server turn with the same
     * text at the same count: the second "hi" on the phone is the second "hi" there.
     */
    private suspend fun serverTime(prompt: ChatMessage, chat: Conversation): Long {
        if (prompt.sentAt != 0L) return prompt.sentAt
        val nth = chat.messages.takeWhile { it.id != prompt.id }.count { it.role == prompt.role && it.text == prompt.text }
        return api().getSession(chat.id).mapNotNull { entry ->
            when (entry) {
                is HistoryEntry.User -> entry.timestamp.takeIf { prompt.role == "user" && entry.content == prompt.text }
                is HistoryEntry.Scheduled -> entry.timestamp.takeIf { prompt.role == "scheduled" && entry.label == prompt.text }
                is HistoryEntry.Assistant, is HistoryEntry.Approval -> null
            }
        }.getOrElse(nth) { 0L }
    }
    fun retry() { launchRun(state.value.current.id) }
    /**
     * Sends the last message again after its run ended without finishing.
     * The message is already on screen, so it is not added twice.
     */
    fun retryLast() {
        val chat = state.value.current
        if (chat.busy) { retry(); return }
        val last = chat.messages.lastOrNull { it.role == "user" } ?: return
        if (!settings.value.configured) return
        updateChat(chat.id) { it.copy(requestId = UUID.randomUUID().toString(), pendingText = last.text,
            pendingImages = last.images, pendingModel = last.model, pendingThinking = last.modelThinking, status = "Sending…", error = "", cancelRequested = false) }
        launchRun(chat.id)
    }
    fun cancel() {
        val chat = state.value.current
        updateChat(chat.id) { it.copy(cancelRequested = true, status = "Stopping…") }
        scope.launch {
            if (chat.runId.isEmpty()) { retry(); return@launch }
            try { api().cancelRun(chat.runId) }
            catch (error: Exception) { updateChat(chat.id) { it.copy(error = userMessage(error)) } }
        }
    }
    /** What the chat shows under a run that ended with [status], given the error it already has. */
    private fun runEndError(status: String, current: String) = when (status) {
        // Stopped on an approval card; the answer starts the next run.
        "completed", "waiting" -> ""
        "cancelled" -> "Stopped"
        "interrupted" -> "The server restarted before Beep finished."
        "timed_out" -> "That took too long, so Beep stopped. Try again."
        "stalled" -> "The model stopped responding. Try again."
        "looped" -> "Beep got stuck repeating itself and was stopped. Try again."
        else -> current.ifEmpty { "Run ended: $status. You can send another message." }
    }
    private fun launchRun(id: String) = synchronized(lock) {
        if (jobs[id]?.isActive == true) return@synchronized
        jobs[id] = scope.launch {
            var failures = 0
            while (isActive) {
                val chat = state.value.conversations.find { it.id == id } ?: break
                if (!chat.busy && !startQueued(chat)) break
                if (!chat.busy) continue
                if (!networkUp) { updateChat(id) { it.copy(status = waiting(it), error = "") }; break }
                try {
                    val api = api()
                    updateChat(id) { it.copy(error = "", status = if (it.runId.isEmpty()) "Sending…" else "Beep is thinking…") }
                    if (chat.runId.isNotEmpty() && chat.cancelRequested) api.cancelRun(chat.runId)

                    if (chat.runId.isEmpty()) { reportTimezone(); refreshLocation() }
                    // A new message streams its run from the start; a run we lost is resumed from the cursor.
                    val stream = if (chat.runId.isEmpty()) api.chat(id, chat.requestId, chat.pendingText, chat.pendingImages,
                        chat.pendingModel.takeIf { it.isNotEmpty() }?.let { Rerun(it, ReasoningLevel.entries.find { level -> level.name.equals(chat.pendingThinking, true) }) })
                        else api.events(chat.runId, chat.cursor)
                    var runId = chat.runId
                    var missed = false
                    stream.use { events ->
                        var first = true
                        while (true) {
                            val received = events.next() ?: break
                            val started = received.event as? RunEvent.RunStarted
                            if (started != null) {
                                runId = started.runId
                                updateChat(id) { it.copy(runId = runId, requestId = "", pendingText = "", pendingImages = emptyList(), pendingModel = "", pendingThinking = "", cursor = 0) }
                                if (state.value.conversations.first { it.id == id }.cancelRequested) api.cancelRun(runId)
                            }

                            // A run that finished while we were away only reports its status.
                            missed = first && received.event is RunEvent.RunFinished
                            first = false
                            if (missed && runId.isEmpty()) {
                                // A resent message whose run already finished: we never learned its id.
                                val status = (received.event as RunEvent.RunFinished).status
                                updateChat(id) { it.copy(requestId = "", pendingText = "", pendingImages = emptyList(), pendingModel = "", pendingThinking = "", status = "", tool = "", cancelRequested = false,
                                    error = runEndError(status, it.error)) }
                            } else {
                                event(id, runId, received)
                            }
                        }
                    }
                    // Its reply is in the session history instead.
                    if (missed) loadHistory(id, settings.value)
                    failures = 0; connected.value = true
                    // Bounded server subscriptions may end before the run. Reopen from the cursor.
                    delay(150)
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    if (!networkUp) { updateChat(id) { it.copy(status = waiting(it), error = "") }; break }
                    failures++
                    connected.value = false; connection.value = "Connection interrupted"
                    updateChat(id) { it.copy(status = "", error = if (failures < 4) "Reconnecting…" else userMessage(error)) }
                    if (failures >= 4) { reconnect(); break }
                    delay(1000L shl (failures - 1))
                }
            }
        }
    }
    data class Memory(
        val id: String, val content: String, val category: String, val source: MemorySource,
        val updated: String, val validFrom: String?, val validUntil: String?, val quote: String?,
    )
    private fun memory(it: ProtoMemory) =
        Memory(it.id, it.content, it.category, it.source, it.updated, it.validFrom, it.validUntil, it.quote)
    /** Chats whose messages hold `query`, searched on the server since the phone keeps few. */
    suspend fun searchChats(query: String): List<SessionMatch> = api().searchSessions(query)
    /** Shares the chat as it is now, or updates its shared copy under the same link. */
    suspend fun shareChat(id: String): SharedLink = api().shareSession(id)
    suspend fun unshareChat(id: String) = api().unshareSession(id)
    suspend fun sharedChat(token: String): SharedSession = api().getShare(token)
    /** Copies a shared chat into the user's own chats and opens it. */
    suspend fun continueShared(token: String) {
        val id = UUID.randomUUID().toString()
        api().copyShare(token, id)
        select(id)
    }
    /** The memories last fetched, null if never. */
    fun cachedMemories(): List<Memory>? = lists.memories?.map(::memory)
    /** The user's group chats, most recently active first. */
    val groups = MutableStateFlow<List<Group>>(emptyList())
    /** Ids of groups that changed on the server, for the open group to reload. */
    val groupChanges = MutableSharedFlow<String>(extraBufferCapacity = 16)
    @Volatile private var openGroup: String? = null
    /** The group on screen, or null when none is. Its notification goes. */
    fun groupShown(id: String?) {
        openGroup = id
        if (id != null) ChatNotifications.cancelGroup(storage.context, id)
    }
    fun groupOnScreen(id: String) = chatScreens.get() > 0 && openGroup == id
    fun groupChanged(id: String) {
        groupChanges.tryEmit(id)
        refreshGroups()
    }
    fun refreshGroups() {
        scope.launch { runCatching { api().listGroups() }.onSuccess { groups.value = it } }
    }
    suspend fun group(id: String): GroupChat = api().getGroup(id)
    suspend fun createGroup(name: String, memberName: String): Group =
        api().createGroup(UUID.randomUUID().toString(), name, memberName).also { refreshGroups() }
    /** Posts to a group; the same [requestId] sent twice posts once. */
    suspend fun sendToGroup(id: String, requestId: String, text: String): GroupMessage =
        api().sendToGroup(id, requestId, text)
    suspend fun groupInvite(id: String, reset: Boolean = false): GroupLink = api().groupInvite(id, reset)
    suspend fun peekGroup(token: String): GroupPreview = api().peekGroup(token)
    suspend fun joinGroup(token: String, memberName: String): Group =
        api().joinGroup(token, memberName).also { refreshGroups() }
    suspend fun leaveGroup(id: String) {
        api().leaveGroup(id)
        groups.value = groups.value.filter { it.id != id }
    }
    /** Pins a group at once; the list is fetched again if the server refuses. */
    fun pinGroup(id: String, pinned: Boolean) {
        groups.value = groups.value.map { if (it.id == id) it.copy(pinned = pinned) else it }
        scope.launch { runCatching { api().pinGroup(id, pinned) }.onFailure { refreshGroups() } }
    }
    /** Lets Beep use the user's apps in a group, or stops it; the list is fetched again if the server refuses. */
    fun shareGroupApps(id: String, shared: Boolean) {
        groups.value = groups.value.map { if (it.id == id) it.copy(appsShared = shared) else it }
        scope.launch { runCatching { api().shareGroupApps(id, shared) }.onFailure { refreshGroups() } }
    }
    suspend fun groupReminders(id: String): List<GroupReminder> = api().listGroupReminders(id)
    suspend fun deleteGroupReminder(groupId: String, id: String) = api().deleteGroupReminder(groupId, id)
    /** Answers an app action Beep asked to take for the user from a group. */
    fun answerGroupApproval(queryId: String, allow: Boolean) {
        ApprovalNotifications.cancel(storage.context, queryId)
        scope.launch { runCatching { api().answerGroupApproval(queryId, allow) } }
    }
    /** Leaves each group in the background, as bulk delete does for chats. */
    fun leaveGroups(ids: Set<String>) {
        groups.value = groups.value.filter { it.id !in ids }
        scope.launch {
            for (id in ids) runCatching { api().leaveGroup(id) }
            refreshGroups()
        }
    }

    suspend fun listMemories(): List<Memory> {
        val fresh = api().listMemories()
        saveLists { it.copy(memories = fresh) }
        return fresh.map(::memory)
    }
    suspend fun deleteMemory(id: String) {
        api().deleteMemory(id)
    }
    suspend fun updateMemory(id: String, content: String) {
        api().updateMemory(id, content)
    }
    suspend fun clearMemories() {
        api().clearMemories()
    }

    /** `nextRunAt` is an RFC 3339 time the server works out in the user's timezone. */
    data class Cron(val id: String, val label: String, val prompt: String, val schedule: String, val enabled: Boolean, val nextRunAt: String?)
    private fun cron(it: ProtoCron) = Cron(it.id, it.label, it.prompt, it.schedule, it.enabled, it.nextRunAt)
    /** The routines last fetched, null if never. */
    fun cachedCrons(): List<Cron>? = lists.crons?.map(::cron)
    suspend fun listCrons(): List<Cron> {
        val fresh = api().listCrons()
        saveLists { it.copy(crons = fresh) }
        return fresh.map(::cron)
    }
    suspend fun createCron(label: String, prompt: String, schedule: String) {
        api().createCron(label, prompt, schedule)
    }
    suspend fun deleteCron(id: String) {
        api().deleteCron(id)
    }
    /** Runs a routine now and returns the chat it ran in. */
    suspend fun runCron(id: String): String = api().runCron(id)
    suspend fun toggleCron(id: String, enabled: Boolean) {
        api().updateCron(id, enabled, null, null)
    }

    /** The apps last fetched, null if never. */
    fun cachedApps(): List<AppIntegration>? = lists.apps
    suspend fun listApps(): List<AppIntegration> = api().listApps().also { fresh -> saveLists { it.copy(apps = fresh) } }
    suspend fun connectApp(slug: String): String = api().connectApp(slug).url
    suspend fun disconnectApp(slug: String) = api().disconnectApp(slug)
    suspend fun appPermissions(slug: String): AppPermissions = api().getAppPermissions(slug)
    suspend fun setAppPermissions(slug: String, level: PermissionLevel, allowed: List<String>) =
        api().setAppPermissions(slug, level, allowed)
    /** The last model list the server sent, cached between launches. Its choices are empty unless the user is an admin. */
    val modelChoices = MutableStateFlow(lists.models)
    suspend fun listDevices(): List<LinkedDevice> = api().listDevices()
    suspend fun linkDevice(offerUri: String): LinkedDevice = api().linkDevice(offerUri)
    suspend fun removeDevice(id: String) = api().removeDevice(id)
    suspend fun models(): Models = api().getModels().also(::keepModels)
    suspend fun setModel(id: String): Models = api().setModel(id).also(::keepModels)
    suspend fun setThinking(level: ReasoningLevel): Models = api().setThinking(level).also(::keepModels)
    suspend fun setAutoModel(enabled: Boolean): Models = api().setAutoModel(enabled).also(::keepModels)
    private fun keepModels(fresh: Models) {
        modelChoices.value = fresh
        saveLists { it.copy(models = fresh) }
    }
    private fun saveLists(transform: (ListCache) -> ListCache) = synchronized(lock) {
        lists = transform(lists)
        storage.saveLists(lists)
    }

    /** Set when a localhost OAuth flow ends, so the app list reloads. */
    val appAuthResults = MutableStateFlow<AppAuthResult?>(null)

    /**
     * Connects an app whose provider only redirects to localhost. Runs in the repository's
     * scope so it survives the screen while the user is in the browser.
     */
    fun authorizeApp(slug: String, open: (String) -> Unit) {
        scope.launch {
            val error = try {
                LoopbackAuth.run(
                    start = { redirect -> api().startAppAuth(slug, redirect).url },
                    open = open,
                    finish = { api().finishAppAuth(slug, it.code, it.state, it.issuer) },
                )
                null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                e.message ?: "Could not connect"
            }
            appAuthResults.value = AppAuthResult(slug, error)
        }
    }

    /** Conversation starters for an empty chat, kept on disk so the home screen never waits for them. */
    val starters = MutableStateFlow(storage.loadStarters().first)
    private var startersJob: Job? = null
    /**
     * Asks the server for starters, showing the saved ones meanwhile. The server
     * answers from its cache and decides when they are stale, like after a
     * flight lands; `refresh` makes new ones.
     */
    fun loadStarters(refresh: Boolean = false) {
        if (!refresh && startersJob?.isActive == true) return
        startersJob = scope.launch {
            if (refresh) startersRefreshing.value = true
            try {
                runCatching { api().suggestPrompts(refresh) }.onSuccess { result ->
                    if (result.personal) {
                        storage.saveStarters(result.suggestions, System.currentTimeMillis())
                        starters.value = result.suggestions
                    } else if (starters.value.isEmpty()) {
                        // The fixed fallback is shown but not kept, so the next visit asks again.
                        starters.value = result.suggestions
                    }
                }
            } finally {
                if (refresh) startersRefreshing.value = false
            }
        }
    }

    /** True while a set the user asked for with New ideas is loading. */
    val startersRefreshing = MutableStateFlow(false)

    /** When the user asked for new starters, oldest first, as elapsed-realtime millis. */
    val starterRefreshes = MutableStateFlow<List<Long>>(emptyList())

    /** Millis until another New ideas may run, 0 now; the server keeps the same limit. */
    fun starterRefreshWait(refreshes: List<Long>, now: Long): Long {
        val recent = refreshes.filter { now - it < STARTER_REFRESH_WINDOW_MS }
        if (recent.size < STARTER_REFRESHES) return 0
        return (recent.first() + STARTER_REFRESH_WINDOW_MS - now).coerceAtLeast(1)
    }

    /** New starters on request, within the refresh limit. */
    fun newStarters() {
        val now = SystemClock.elapsedRealtime()
        if (startersRefreshing.value || starterRefreshWait(starterRefreshes.value, now) > 0) return
        starterRefreshes.value = starterRefreshes.value.filter { now - it < STARTER_REFRESH_WINDOW_MS } + now
        loadStarters(refresh = true)
    }

    /** Takes a used starter off the saved set until the next one loads. */
    fun dropStarter(prompt: String) {
        val left = starters.value.filter { it.prompt != prompt }
        if (left.size == starters.value.size) return
        starters.value = left
        storage.saveStarters(left, System.currentTimeMillis())
    }

    fun renameSession(id: String, title: String) = queue(ChatChange.Rename(id, title))
    fun deleteSession(id: String) = queue(ChatChange.Delete(listOf(id)))
    fun deleteSessions(ids: Set<String>) { if (ids.isNotEmpty()) queue(ChatChange.Delete(ids.toList())) }
    fun pinSession(id: String, pinned: Boolean) = queue(ChatChange.Pin(id, pinned))

    /** Shows [change] at once and sends it to the server now, or once the app is back online. */
    private fun queue(change: ChatChange) {
        synchronized(lock) {
            // A later change to the same chat makes the earlier one moot.
            val kept = changes.mapNotNull { old ->
                when {
                    change is ChatChange.Delete && old is ChatChange.Delete -> old
                    change is ChatChange.Delete -> old.takeUnless { changedChat(it) in change.ids }
                    old::class == change::class && changedChat(old) == changedChat(change) -> null
                    else -> old
                }
            }
            changes = kept + change
            storage.saveChanges(changes)
            update { applyChange(it, change) }
        }
        if (change is ChatChange.Delete) change.ids.forEach { setDraft(it, Draft()) }
        scope.launch { pushChanges() }
    }
    private fun changedChat(change: ChatChange): String? = when (change) {
        is ChatChange.Delete -> null
        is ChatChange.Rename -> change.id
        is ChatChange.Pin -> change.id
    }

    /** Sends queued changes in order, stopping at the first that fails to reach the server. */
    private suspend fun pushChanges() = pushing.withLock {
        while (true) {
            val change = synchronized(lock) { changes.firstOrNull() } ?: return@withLock
            try {
                when (change) {
                    is ChatChange.Delete -> change.ids.chunked(200).forEach { api().deleteSessions(it) }
                    is ChatChange.Rename -> api().renameSession(change.id, change.title)
                    is ChatChange.Pin -> api().pinSession(change.id, change.pinned)
                }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                // A chat the server never had, or a change it refuses, will not go through on a retry either.
                if (error !is BeepException.NotFound && error !is BeepException.BadRequest) return@withLock
            }
            synchronized(lock) {
                changes = changes.filter { it !== change }
                storage.saveChanges(changes)
            }
        }
    }

    /** [state] with the changes the server does not have yet laid over it. */
    private fun withChanges(state: ChatState): ChatState = synchronized(lock) { changes.fold(state, ::applyChange) }

    private fun applyChange(state: ChatState, change: ChatChange): ChatState = when (change) {
        is ChatChange.Delete -> {
            val remaining = state.conversations.filter { it.id !in change.ids }
            val safe = remaining.ifEmpty { listOf(Conversation()) }
            state.copy(conversations = safe, selected = if (state.selected in change.ids) safe.first().id else state.selected)
        }
        is ChatChange.Rename -> state.copy(conversations = state.conversations.map { if (it.id == change.id) it.copy(title = change.title) else it })
        is ChatChange.Pin -> state.copy(conversations = state.conversations
            .map { if (it.id == change.id) it.copy(pinned = change.pinned) else it }
            .sortedByDescending { it.pinned })
    }

    /** Titles are written by the server after the first message and once a chat goes quiet. */
    private fun refreshTitles() {
        val config = settings.value
        scope.launch {
            runCatching {
                val titles = api(config).listSessions().associate { it.id to it.title }
                if (settings.value != config) return@runCatching
                update { state -> withChanges(state.copy(conversations = state.conversations.map { c -> titles[c.id]?.let { c.copy(title = it) } ?: c })) }
            }
        }
    }

    /** Lets the server reach this phone through Firebase when it cannot over iroh. */
    fun registerPushToken(token: String) {
        if (!settings.value.configured) return
        scope.launch { runCatching { api().registerDevice(storage.deviceId(), token) } }
    }

    /** One-shot translation: creates a temporary session, sends the prompt, streams the response, and returns the full text. */
    suspend fun translate(text: String, language: String): String {
        val config = settings.value
        require(config.configured) { "Server not configured" }
        val api = api(config)
        val sessionId = UUID.randomUUID().toString()
        val prompt = if (language == "Auto") "Translate the following text to the most likely target language (if it's English, translate to Spanish; otherwise translate to English). Reply with ONLY the translation, no explanation:\n\n$text"
            else "Translate the following text to $language. Reply with ONLY the translation, no explanation:\n\n$text"
        val requestId = UUID.randomUUID().toString()
        val result = StringBuilder()
        var runId = ""
        var cursor = 0L
        var finished = false
        while (!finished) {
            val stream = if (runId.isEmpty()) api.chat(sessionId, requestId, prompt, emptyList(), null) else api.events(runId, cursor)
            stream.use { events ->
                var first = true
                while (true) {
                    val event = events.next() ?: break
                    cursor = event.seq
                    when (val e = event.event) {
                        is RunEvent.RunStarted -> runId = e.runId
                        is RunEvent.TextDelta -> result.append(e.delta)
                        is RunEvent.RunFinished -> {
                            finished = true
                            // Finished while we were away: the full reply is in the session.
                            if (first) {
                                val reply = api.getSession(sessionId).lastOrNull() as? HistoryEntry.Assistant
                                result.setLength(0)
                                result.append(reply?.content.orEmpty())
                            }
                        }
                        is RunEvent.Error -> throw Exception(e.message.ifEmpty { "Translation failed" })
                        else -> {}
                    }
                    first = false
                }
            }
            if (!finished) delay(150)
        }
        return result.toString().trim()
    }

    /** A position to report, with the phone's name for the area. */
    data class Fix(val latitude: Double, val longitude: Double, val accuracy: Float, val place: String?)

    private var locate: (suspend () -> Fix?)? = null
    @Volatile private var locatedAt = 0L

    private suspend fun updateLocation(fix: Fix) {
        val tz = java.util.TimeZone.getDefault().id
        api().updateLocation(fix.latitude, fix.longitude, fix.accuracy.toDouble(), tz, fix.place)
        locatedAt = System.currentTimeMillis()
        reported = fix
    }
    private var reported: Fix? = null

    /** True unless [fix] is within 50 m of the last reported one. */
    private fun moved(fix: Fix): Boolean {
        val last = reported ?: return true
        val distance = FloatArray(1)
        Location.distanceBetween(last.latitude, last.longitude, fix.latitude, fix.longitude, distance)
        return distance[0] >= 50f
    }

    fun startLocationUpdates(getLocation: suspend () -> Fix?) {
        locate = getLocation
        locationJob?.cancel()
        locationJob = scope.launch {
            while (isActive) {
                if (connected.value) {
                    try { getLocation()?.takeIf { moved(it) }?.let { updateLocation(it) } } catch (_: Exception) {}
                }
                // Each report opens a connection, so it stays rare and skips a phone that hasn't moved.
                delay(15 * 60 * 1000L)
            }
        }
    }

    @Volatile private var reportedTimezone: String? = null

    /** Tells the server the phone's timezone when it differs from the last one sent, so routines run in local time without a location. */
    private suspend fun reportTimezone() {
        val tz = java.util.TimeZone.getDefault().id
        if (tz == reportedTimezone) return
        try { api().setTimezone(tz); reportedTimezone = tz } catch (_: Exception) {}
    }

    /** Reports the position before a message when the last report is over a minute old, so "nearby" means here. */
    private suspend fun refreshLocation() {
        val getLocation = locate ?: return
        if (System.currentTimeMillis() - locatedAt < 60_000L) return
        withTimeoutOrNull(2_000L) {
            try { getLocation()?.let { updateLocation(it) } } catch (_: Exception) {}
        }
    }

    private suspend fun executeDeviceQuery(query: DeviceQuery): JSONObject = when (query) {
        is DeviceQuery.Contacts -> queryContacts(query.name)
        is DeviceQuery.Browser -> browser(query.command)
        // Never runs on its own: the user answers it through [answerApproval].
        is DeviceQuery.Approve -> JSONObject().put("decision", "deny")
    }

    /**
     * Answers a tool approval with "allow", "always" or "deny", from the card or the
     * notification. The card locks on the answer; the server acks with the run that
     * carries the chat on from that call, and the reply streams into the same message.
     */
    fun answerApproval(conversationId: String, queryId: String, decision: String) {
        var answered = false
        updateChat(conversationId) { chat ->
            chat.copy(messages = chat.messages.map { message ->
                message.copy(approvals = message.approvals.map {
                    if (it.queryId != queryId) it
                    else { answered = !it.waiting; if (it.waiting) it.copy(answer = decision) else it }
                })
            })
        }
        ApprovalNotifications.cancel(storage.context, queryId)
        if (answered) return
        val wire = when (decision) {
            "always" -> ApprovalDecision.ALWAYS
            "allow" -> ApprovalDecision.ALLOW
            else -> ApprovalDecision.DENY
        }
        scope.launch {
            try {
                val run = api().answerApproval(conversationId, queryId, wire)
                updateChat(conversationId) { chat ->
                    val holder = chat.messages.lastOrNull { m -> m.approvals.any { it.queryId == queryId } }
                    chat.copy(messages = chat.messages.map { if (it === holder) it.copy(id = "assistant-${run.id}") else it },
                        runId = run.id, cursor = 0, status = "Beep is thinking…", error = "", cancelRequested = false)
                }
                launchRun(conversationId)
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                // A conflict means the server already has an answer for this call.
                if (error is BeepException.Conflict) return@launch
                updateChat(conversationId) { chat ->
                    chat.copy(error = userMessage(error), messages = chat.messages.map { message ->
                        message.copy(approvals = message.approvals.map { if (it.queryId == queryId) it.copy(answer = "") else it })
                    })
                }
            }
        }
    }

    private fun skipApprovals(messages: List<ChatMessage>) = messages.map { message ->
        if (message.approvals.none { it.waiting }) message
        else message.copy(approvals = message.approvals.map { if (it.waiting) it.copy(answer = "skipped") else it })
    }

    private suspend fun browser(command: BrowserCommand): JSONObject {
        val ctx = storage.context
        val (action, params) = when (command) {
            is BrowserCommand.Navigate -> {
                browserOpen.value = true
                return BrowserBridge.navigate(ctx, command.url)
            }
            BrowserCommand.ReadPage -> "read_page" to JSONObject()
            is BrowserCommand.Click -> "click" to JSONObject().put("selector", command.selector)
            is BrowserCommand.ClickElement -> "click" to JSONObject().put("index", command.index.toInt())
            is BrowserCommand.Type -> "type" to JSONObject().put("selector", command.selector).put("text", command.text)
            is BrowserCommand.TypeElement -> "type" to JSONObject().put("index", command.index.toInt())
                .put("text", command.text).put("submit", command.submit)
            is BrowserCommand.Scroll -> "scroll" to
                JSONObject().put("direction", if (command.direction == ScrollDirection.UP) "up" else "down")
            is BrowserCommand.GetText -> "get_text" to JSONObject().put("selector", command.selector)
            is BrowserCommand.WaitFor -> "wait_for" to JSONObject().put("selector", command.selector)
            BrowserCommand.Back -> "back" to JSONObject()
            BrowserCommand.Forward -> "forward" to JSONObject()
            BrowserCommand.Reload -> "reload" to JSONObject()
            BrowserCommand.GetPageInfo -> "get_page_info" to JSONObject()
        }
        return BrowserBridge.executeCommand(ctx, action, params)
    }

    /** Show the in-app browser on [url]. */
    fun openBrowser(url: String) {
        if (url.isEmpty()) return
        browserOpen.value = true
        scope.launch { BrowserBridge.open(storage.context, url) }
    }

    fun closeBrowser() { browserOpen.value = false }

    private fun queryContacts(query: String): JSONObject {
        val ctx = storage.context
        if (ContextCompat.checkSelfPermission(ctx, Manifest.permission.READ_CONTACTS) != PackageManager.PERMISSION_GRANTED) {
            return JSONObject().put("error", "permission_denied")
                .put("message", "READ_CONTACTS permission not granted. The user needs to allow contacts access in the app settings.")
        }
        val results = JSONArray()
        val projection = arrayOf(
            ContactsContract.Contacts._ID,
            ContactsContract.Contacts.DISPLAY_NAME_PRIMARY,
            ContactsContract.Contacts.HAS_PHONE_NUMBER
        )
        val selection = if (query.isNotEmpty()) "${ContactsContract.Contacts.DISPLAY_NAME_PRIMARY} LIKE ?" else null
        val selectionArgs = if (query.isNotEmpty()) arrayOf("%$query%") else null

        ctx.contentResolver.query(ContactsContract.Contacts.CONTENT_URI, projection, selection, selectionArgs, null)?.use { cursor ->
            val idIdx = cursor.getColumnIndex(ContactsContract.Contacts._ID)
            val nameIdx = cursor.getColumnIndex(ContactsContract.Contacts.DISPLAY_NAME_PRIMARY)
            val hasPhoneIdx = cursor.getColumnIndex(ContactsContract.Contacts.HAS_PHONE_NUMBER)
            while (cursor.moveToNext() && results.length() < 10) {
                val id = cursor.getString(idIdx)
                val name = cursor.getString(nameIdx) ?: continue
                val contact = JSONObject().put("name", name)

                if (cursor.getInt(hasPhoneIdx) > 0) {
                    val phones = getPhoneNumbers(ctx, id)
                    if (phones.length() > 0) contact.put("phones", phones)
                }

                val emails = getEmails(ctx, id)
                if (emails.length() > 0) contact.put("emails", emails)

                results.put(contact)
            }
        }
        return JSONObject().put("contacts", results).put("count", results.length())
    }

    private fun getPhoneNumbers(ctx: android.content.Context, contactId: String): JSONArray {
        val phones = JSONArray()
        ctx.contentResolver.query(
            ContactsContract.CommonDataKinds.Phone.CONTENT_URI,
            arrayOf(ContactsContract.CommonDataKinds.Phone.NUMBER, ContactsContract.CommonDataKinds.Phone.TYPE),
            "${ContactsContract.CommonDataKinds.Phone.CONTACT_ID} = ?",
            arrayOf(contactId), null
        )?.use { cursor ->
            val numIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.NUMBER)
            val typeIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Phone.TYPE)
            while (cursor.moveToNext()) {
                val number = cursor.getString(numIdx) ?: continue
                val typeInt = cursor.getInt(typeIdx)
                val label = ContactsContract.CommonDataKinds.Phone.getTypeLabel(ctx.resources, typeInt, "Other").toString()
                phones.put(JSONObject().put("number", number).put("type", label))
            }
        }
        return phones
    }

    private fun getEmails(ctx: android.content.Context, contactId: String): JSONArray {
        val emails = JSONArray()
        ctx.contentResolver.query(
            ContactsContract.CommonDataKinds.Email.CONTENT_URI,
            arrayOf(ContactsContract.CommonDataKinds.Email.ADDRESS, ContactsContract.CommonDataKinds.Email.TYPE),
            "${ContactsContract.CommonDataKinds.Email.CONTACT_ID} = ?",
            arrayOf(contactId), null
        )?.use { cursor ->
            val addrIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Email.ADDRESS)
            val typeIdx = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Email.TYPE)
            while (cursor.moveToNext()) {
                val address = cursor.getString(addrIdx) ?: continue
                val typeInt = cursor.getInt(typeIdx)
                val label = ContactsContract.CommonDataKinds.Email.getTypeLabel(ctx.resources, typeInt, "Other").toString()
                emails.put(JSONObject().put("address", address).put("type", label))
            }
        }
        return emails
    }

    private suspend fun sendDeviceResult(runId: String, queryId: String, result: JSONObject) {
        val answer = if (result.has("error")) DeviceAnswer.Err(result.optString("message", result.optString("error")))
            else DeviceAnswer.Ok(result.toString())
        api().deviceResult(runId, queryId, answer)
    }

    private fun extractCardsFromText(message: ChatMessage): ChatMessage {
        val cardPattern = Regex("""\{[^{}]*"type"\s*:\s*"card"[^{}]*\}""")
        val matches = cardPattern.findAll(message.text)
        val cards = mutableListOf<Card>()
        var cleanText = message.text
        for (match in matches) {
            try {
                val json = JSONObject(match.value)
                if (json.optString("type") == "card") {
                    cards.add(Card(json.getString("card_type"), json.getJSONObject("card_data")))
                    cleanText = cleanText.replace(match.value, "")
                }
            } catch (_: Exception) {}
        }
        if (cards.isEmpty()) return message
        return message.copy(text = cleanText.trim(), cards = message.cards + cards)
    }

    private fun event(id: String, runId: String, received: Event) {
        val sequence = received.seq
        var finished = false
        updateChat(id) { chat ->
            if (runId != chat.runId || sequence <= chat.cursor) return@updateChat chat
            finished = received.event is RunEvent.RunFinished
            check(sequence == chat.cursor + 1) { "Missing server event; reconnecting" }
            var next = chat.copy(cursor = sequence)
            when (val event = received.event) {
                is RunEvent.TextDelta -> {
                    val messageId = "assistant-${chat.runId}"
                    val existing = next.messages.find { it.id == messageId }
                    val newText = existing?.text.orEmpty() + event.delta
                    val message = if (existing != null) existing.copy(text = newText, reasoning = joinReasoning(existing.reasoning, chat.thinking))
                        else ChatMessage(messageId, "assistant", newText, reasoning = chat.thinking.trim())
                    next = next.copy(messages = if (existing == null) next.messages + message else next.messages.map { if (it.id == messageId) message else it }, status = "Replying...", tool = "", thinking = "")
                }
                is RunEvent.ReasoningDelta -> next = next.copy(thinking = (chat.thinking + event.delta).takeLast(MAX_REASONING))
                is RunEvent.ToolStarted -> {
                    val description = toolLabel(event.name).ifEmpty { "Using a tool..." }
                    val messageId = "assistant-${chat.runId}"
                    val existing = next.messages.find { it.id == messageId }
                    val tc = ToolCall(callId = event.callId, name = event.name, description = description)
                    // An answered approval starts the call it already shows.
                    val known = existing != null && event.callId.isNotEmpty() && existing.toolCalls.any { it.callId == event.callId }
                    val msg = if (existing != null) existing.copy(toolCalls = if (known) existing.toolCalls else existing.toolCalls + tc,
                            reasoning = joinReasoning(existing.reasoning, chat.thinking))
                        else ChatMessage(messageId, "assistant", "", toolCalls = listOf(tc), reasoning = chat.thinking.trim())
                    next = next.copy(
                        messages = if (existing == null) next.messages + msg else next.messages.map { if (it.id == messageId) msg else it },
                        tool = description, status = "Working...", thinking = "")
                }
                is RunEvent.ToolResult -> {
                    val callId = event.callId
                    val toolSuccess = event.success
                    val output = event.output
                    val messageId = "assistant-${chat.runId}"
                    val curMsg = next.messages.find { it.id == messageId }
                    if (curMsg != null && curMsg.toolCalls.isNotEmpty()) {
                        val updated = curMsg.toolCalls.map { tc ->
                            if ((callId.isNotEmpty() && tc.callId == callId) || (callId.isEmpty() && !tc.isDone))
                                tc.copy(isDone = true, success = toolSuccess, result = output) else tc
                        }
                        next = next.copy(messages = next.messages.map { if (it.id == messageId) curMsg.copy(toolCalls = updated) else it })
                    }
                    next = next.copy(tool = if (toolSuccess) "Done" else "Tool didn’t complete")
                    if (output.contains("\"action\"") || output.contains("\"type\"")) {
                        try {
                            val outputJson = JSONObject(output)
                            when (outputJson.optString("action")) {
                                "open_app" -> pendingIntent.value = AppIntent(
                                    app = outputJson.optString("app", ""),
                                    uri = outputJson.optString("uri", ""),
                                    fallbackUrl = outputJson.optString("fallback_url", ""))
                                "set_alarm" -> pendingAlarm.value = AlarmRequest.from(outputJson)
                                "open_browser" -> openBrowser(outputJson.optString("url", ""))
                                // Places show as a map card in the chat; Maps opens only when the user taps it.
                            }
                            if (outputJson.optString("type") == "task_progress") {
                                val stepsArr = outputJson.optJSONArray("steps")
                                if (stepsArr != null) {
                                    val steps = (0 until stepsArr.length()).map { si ->
                                        val s = stepsArr.getJSONObject(si)
                                        TaskStep(s.getString("label"), s.getString("status"))
                                    }
                                    val messageId = "assistant-${chat.runId}"
                                    val existing = next.messages.find { it.id == messageId }
                                    if (existing != null) {
                                        next = next.copy(messages = next.messages.map {
                                            if (it.id == messageId) it.copy(taskSteps = steps) else it
                                        })
                                    } else {
                                        next = next.copy(messages = next.messages + ChatMessage(messageId, "assistant", "", taskSteps = steps))
                                    }
                                }
                            }
                            if (outputJson.optString("type") == "card") {
                                val card = Card(outputJson.getString("card_type"), outputJson.getJSONObject("card_data"))
                                val messageId = "assistant-${chat.runId}"
                                val existing = next.messages.find { it.id == messageId }
                                if (existing != null) {
                                    next = next.copy(messages = next.messages.map {
                                        if (it.id == messageId) it.copy(cards = it.cards + card) else it
                                    })
                                } else {
                                    next = next.copy(messages = next.messages + ChatMessage(messageId, "assistant", "", listOf(card)))
                                }
                            }
                        } catch (_: Exception) {}
                    }
                }
                is RunEvent.DeviceQuery -> {
                    val query = event.query
                    if (query is DeviceQuery.Approve) {
                        // An approval waits for the user's tap on the card or the notification.
                        val messageId = "assistant-${chat.runId}"
                        val prompt = ApprovalPrompt(event.queryId, query.approval)
                        next = if (next.messages.any { it.id == messageId })
                            next.copy(messages = next.messages.map { if (it.id == messageId) it.copy(approvals = it.approvals + prompt) else it })
                        else next.copy(messages = next.messages + ChatMessage(messageId, "assistant", "", approvals = listOf(prompt)))
                    } else scope.launch {
                        runCatching {
                            val result = runCatching { executeDeviceQuery(query) }.getOrElse {
                                JSONObject().put("error", "device_error").put("message", it.message ?: "The phone could not do that.")
                            }
                            sendDeviceResult(chat.runId, event.queryId, result)
                        }
                    }
                }
                is RunEvent.Error -> next = next.copy(error = event.message.ifEmpty { "The assistant couldn’t finish." })
                is RunEvent.RunFinished -> {
                    val status = event.status
                    refreshTitles()
                    // The phone streamed the turn the run saved, so it already holds that version.
                    event.updatedAt?.toEpochMilli()?.let { saved ->
                        serverVersions.merge(id, saved, ::maxOf)
                        next = next.copy(version = saved)
                    }
                    // Extract any card JSON embedded in assistant text
                    val messageId = "assistant-${chat.runId}"
                    next = next.copy(
                        messages = next.messages.map {
                            if (it.id == messageId) extractCardsFromText(it).copy(reasoning = joinReasoning(it.reasoning, chat.thinking)) else it
                        },
                        runId = "", status = "", tool = "", thinking = "", cancelRequested = false, error = runEndError(status, chat.error))
                }
                is RunEvent.MemoryScore -> {
                    val messageId = "assistant-${chat.runId}"
                    next = next.copy(messages = next.messages.map { if (it.id == messageId) it.copy(memoryScore = event.probability) else it })
                }
                is RunEvent.AutoPicked -> {
                    val prompt = next.messages.lastOrNull { it.role == "user" }
                    val picked = prompt?.copy(autoModel = event.pick.model, autoThinking = event.pick.thinking.name.lowercase())
                    if (picked != null) next = next.copy(messages = next.messages.map { if (it.id == prompt.id) picked else it })
                }
                is RunEvent.Usage, is RunEvent.RunStarted -> {}
            }
            next
        }
        if (finished && !onScreen(id))
            state.value.conversations.find { it.id == id }?.let {
                ChatNotifications.show(storage.context, it)
                Bubble.callout(storage.context, it)
            }
    }
}
