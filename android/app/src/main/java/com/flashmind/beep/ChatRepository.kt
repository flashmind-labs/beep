package com.flashmind.beep

import android.Manifest
import android.app.Application
import android.content.pm.PackageManager
import android.provider.ContactsContract
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.proto.BrowserCommand
import com.flashmind.beep.proto.DeviceAnswer
import com.flashmind.beep.proto.DeviceQuery
import com.flashmind.beep.proto.Event
import com.flashmind.beep.proto.HistoryEntry
import com.flashmind.beep.proto.RunEvent
import com.flashmind.beep.proto.ScrollDirection
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

class BeepApplication : Application() {
    val repository by lazy { ChatRepository(LocalStorage(this)) }

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    companion object {
        var instance: BeepApplication? = null
            private set
    }
}

data class AppIntent(val app: String, val uri: String, val fallbackUrl: String)

class ChatRepository(private val storage: LocalStorage) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val lock = Any()
    private val jobs = mutableMapOf<String, Job>()
    private val mutableState = MutableStateFlow(storage.loadState())
    val state = mutableState.asStateFlow()
    val settings = MutableStateFlow(storage.loadSettings())
    val connection = MutableStateFlow(if (settings.value.configured) "Checking connection…" else "Connect your assistant")
    val connected = MutableStateFlow(false)
    val pendingIntent = MutableStateFlow<AppIntent?>(null)
    val browserUrl = MutableStateFlow<String?>(null)
    private var reconnectJob: Job? = null
    private var locationJob: Job? = null
    init { if (settings.value.configured) reconnect() }

    private fun api(config: ConnectionSettings = settings.value): BeepClient = BeepClients.client(config)

    private fun update(transform: (ChatState) -> ChatState) = synchronized(lock) {
        val next = transform(mutableState.value)
        storage.saveState(next) // Cursor and text are committed atomically, before publishing either.
        mutableState.value = next
    }
    private fun updateChat(id: String, transform: (Conversation) -> Conversation) = update { s ->
        s.copy(conversations = s.conversations.map { if (it.id == id) transform(it) else it })
    }
    fun select(id: String) {
        update { it.copy(selected = id) }
        scope.launch { runCatching { loadHistory(id, settings.value) } }
    }
    private suspend fun loadHistory(id: String, config: ConnectionSettings) {
        val before = state.value.conversations.find { it.id == id } ?: return
        if (before.busy) return
        val messages = api(config).getSession(id).mapIndexed { i, entry ->
            when (entry) {
                is HistoryEntry.User -> ChatMessage("history-$id-$i", "user", entry.content)
                is HistoryEntry.Assistant -> ChatMessage("history-$id-$i", "assistant", entry.content)
            }
        }
        if (settings.value != config) return
        updateChat(id) { current -> if (current == before) current.copy(messages = messages) else current }
    }
    fun refreshHistory() { scope.launch { runCatching { syncHistory(settings.value) } } }
    private suspend fun syncHistory(config: ConnectionSettings) {
        val sessions = api(config).listSessions()
        if (settings.value != config) return
        update { state ->
            val known = state.conversations.map { it.id }.toSet()
            val serverPinned = sessions.associate { it.id to it.pinned }
            val added = sessions.filter { it.id !in known }
                .map { Conversation(id = it.id, title = it.title, pinned = it.pinned) }
            // Update pinned status from server for existing conversations
            val updated = state.conversations.map { c ->
                val pin = serverPinned[c.id]
                if (pin != null) c.copy(pinned = pin) else c
            }
            state.copy(conversations = (updated + added).sortedByDescending { it.pinned })
        }
        if (sessions.any { it.id == state.value.current.id })
            loadHistory(state.value.current.id, config)
    }
    fun newChat() = update { state ->
        val chat = Conversation(); state.copy(conversations = listOf(chat) + state.conversations, selected = chat.id)
    }
    suspend fun connect(config: ConnectionSettings) {
        require(config.configured) { "Enter your server's node ID." }
        connection.value = "Connecting…"
        try { api(config).health() } catch (error: Exception) { throw IllegalStateException(userMessage(error), error) }
        val effective = config
        val changedAccount = settings.value.nodeId != effective.nodeId || settings.value.token != effective.token
        synchronized(lock) { jobs.values.forEach { it.cancel() }; jobs.clear() }
        storage.saveSettings(effective)
        settings.value = effective
        if (changedAccount) update { ChatState() }
        connected.value = true
        scope.launch { runCatching { registerDevice(storage.deviceId(), "pending-fcm-setup") } }
        NotificationService.start(storage.context)
        connection.value = "Connected"
        resumePending()
        runCatching { syncHistory(effective) }
    }
    fun reconnect() {
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            var attempt = 0
            while (isActive) {
                try {
                    connection.value = if (attempt > 0) "Reconnecting… (attempt ${attempt + 1})" else "Checking connection…"
                    api().health()
                    connected.value = true
                    scope.launch { runCatching { registerDevice(storage.deviceId(), "pending-fcm-setup") } }
                    NotificationService.start(storage.context)
                    connection.value = "Connected"
                    resumePending()
                    runCatching { syncHistory(settings.value) }
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
    private fun resumePending() { state.value.conversations.filter { it.busy }.forEach { launchRun(it.id) } }
    fun send(text: String, imageBase64: String? = null) {
        val chat = state.value.current
        if (text.isBlank() || chat.busy || !settings.value.configured) return
        val request = UUID.randomUUID().toString()
        updateChat(chat.id) { it.copy(title = if (it.messages.isEmpty()) text.take(44) else it.title,
            messages = it.messages + ChatMessage(role = "user", text = text.trim(), imageBase64 = imageBase64),
            requestId = request, pendingText = text.trim(), pendingImage = imageBase64,
            status = "Sending…", error = "", cancelRequested = false) }
        launchRun(chat.id)
    }
    fun retry() { launchRun(state.value.current.id) }
    fun cancel() {
        val chat = state.value.current
        updateChat(chat.id) { it.copy(cancelRequested = true, status = "Stopping…") }
        scope.launch {
            if (chat.runId.isEmpty()) { retry(); return@launch }
            try { api().cancelRun(chat.runId) }
            catch (error: Exception) { updateChat(chat.id) { it.copy(error = userMessage(error)) } }
        }
    }
    private fun launchRun(id: String) = synchronized(lock) {
        if (jobs[id]?.isActive == true) return@synchronized
        jobs[id] = scope.launch {
            var failures = 0
            while (isActive) {
                val chat = state.value.conversations.find { it.id == id } ?: break
                if (!chat.busy) break
                try {
                    val api = api()
                    updateChat(id) { it.copy(error = "", status = if (it.runId.isEmpty()) "Sending…" else "Beep is thinking…") }
                    if (chat.runId.isNotEmpty() && chat.cancelRequested) api.cancelRun(chat.runId)

                    // A new message streams its run from the start; a run we lost is resumed from the cursor.
                    val stream = if (chat.runId.isEmpty()) api.chat(id, chat.requestId, chat.pendingText, chat.pendingImage)
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
                                updateChat(id) { it.copy(runId = runId, requestId = "", pendingText = "", pendingImage = null, cursor = 0) }
                                if (state.value.conversations.first { it.id == id }.cancelRequested) api.cancelRun(runId)
                            }

                            // A run that finished while we were away only reports its status.
                            missed = first && received.event is RunEvent.RunFinished
                            first = false
                            if (missed && runId.isEmpty()) {
                                // A resent message whose run already finished: we never learned its id.
                                updateChat(id) { it.copy(requestId = "", pendingText = "", pendingImage = null, status = "", tool = "", cancelRequested = false) }
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
                    failures++
                    connected.value = false; connection.value = "Connection interrupted"
                    updateChat(id) { it.copy(status = "", error = if (failures < 4) "Reconnecting…" else userMessage(error)) }
                    if (failures >= 4) { reconnect(); break }
                    delay(1000L shl (failures - 1))
                }
            }
        }
    }
    suspend fun listMemories(): List<Pair<String, String>> {
        return api().listMemories().map { it.key to it.content }
    }
    suspend fun deleteMemory(key: String) {
        api().deleteMemory(key)
    }

    data class Cron(val id: String, val label: String, val prompt: String, val schedule: String, val enabled: Boolean)
    suspend fun listCrons(): List<Cron> {
        return api().listCrons().map { Cron(it.id, it.label, it.prompt, it.schedule, it.enabled) }
    }
    suspend fun createCron(label: String, prompt: String, schedule: String) {
        api().createCron(label, prompt, schedule)
    }
    suspend fun deleteCron(id: String) {
        api().deleteCron(id)
    }
    suspend fun toggleCron(id: String, enabled: Boolean) {
        api().updateCron(id, enabled, null, null)
    }

    data class McpConnection(val id: String, val provider: String, val label: String, val status: String)
    suspend fun listIntegrations(): List<McpConnection> {
        return api().listIntegrations().map { McpConnection(it.id, it.provider, it.label, it.status) }
    }
    suspend fun createIntegration(provider: String, label: String, config: String = "{}") {
        api().createIntegration(provider, label, config)
    }
    suspend fun uploadCredentials(id: String, credentials: String) {
        api().updateCredentials(id, credentials, "connected")
    }
    suspend fun deleteIntegration(id: String) {
        api().deleteIntegration(id)
    }

    suspend fun renameSession(id: String, title: String) {
        api().renameSession(id, title)
        updateChat(id) { it.copy(title = title) }
    }

    suspend fun deleteSession(id: String) {
        api().deleteSession(id)
        update { state ->
            val remaining = state.conversations.filter { it.id != id }
            val safe = remaining.ifEmpty { listOf(Conversation()) }
            state.copy(conversations = safe, selected = if (state.selected == id) safe.first().id else state.selected)
        }
    }

    suspend fun pinSession(id: String, pinned: Boolean) {
        api().pinSession(id, pinned)
        updateChat(id) { it.copy(pinned = pinned) }
        update { state ->
            state.copy(conversations = state.conversations.sortedByDescending { it.pinned })
        }
    }

    suspend fun registerDevice(deviceId: String, fcmToken: String) {
        api().registerDevice(deviceId, fcmToken)
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
            val stream = if (runId.isEmpty()) api.chat(sessionId, requestId, prompt, null) else api.events(runId, cursor)
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

    suspend fun updateLocation(latitude: Double, longitude: Double, accuracy: Float) {
        val tz = java.util.TimeZone.getDefault().id
        api().updateLocation(latitude, longitude, accuracy.toDouble(), tz)
    }

    fun startLocationUpdates(getLocation: suspend () -> Triple<Double, Double, Float>?) {
        locationJob?.cancel()
        locationJob = scope.launch {
            while (isActive) {
                if (connected.value) {
                    try {
                        val loc = getLocation()
                        if (loc != null) updateLocation(loc.first, loc.second, loc.third)
                    } catch (_: Exception) {}
                }
                delay(5 * 60 * 1000L)
            }
        }
    }

    private suspend fun executeDeviceQuery(query: DeviceQuery): JSONObject = when (query) {
        is DeviceQuery.Contacts -> queryContacts(query.name)
        is DeviceQuery.Browser -> browser(query.command)
    }

    private suspend fun browser(command: BrowserCommand): JSONObject = when (command) {
        is BrowserCommand.Navigate -> if (command.url.isNotEmpty()) {
            browserUrl.value = command.url
            // Give the WebView a moment to open and load
            delay(2000)
            JSONObject().put("navigated", true).put("url", command.url)
        } else {
            JSONObject().put("error", "missing_url").put("message", "URL is required for navigate action")
        }
        is BrowserCommand.Click ->
            BrowserBridge.executeCommand("click", JSONObject().put("selector", command.selector))
        is BrowserCommand.Type -> BrowserBridge.executeCommand("type",
            JSONObject().put("selector", command.selector).put("text", command.text))
        is BrowserCommand.Scroll -> BrowserBridge.executeCommand("scroll",
            JSONObject().put("direction", if (command.direction == ScrollDirection.UP) "up" else "down"))
        is BrowserCommand.GetText ->
            BrowserBridge.executeCommand("get_text", JSONObject().put("selector", command.selector))
        BrowserCommand.GetPageInfo -> BrowserBridge.executeCommand("get_page_info", JSONObject())
    }

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
        api().deviceResult(runId, queryId, DeviceAnswer.Ok(result.toString()))
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
        updateChat(id) { chat ->
            if (runId != chat.runId || sequence <= chat.cursor) return@updateChat chat
            check(sequence == chat.cursor + 1) { "Missing server event; reconnecting" }
            var next = chat.copy(cursor = sequence)
            when (val event = received.event) {
                is RunEvent.TextDelta -> {
                    val messageId = "assistant-${chat.runId}"
                    val existing = next.messages.find { it.id == messageId }
                    val newText = existing?.text.orEmpty() + event.delta
                    val message = if (existing != null) existing.copy(text = newText)
                        else ChatMessage(messageId, "assistant", newText)
                    next = next.copy(messages = if (existing == null) next.messages + message else next.messages.map { if (it.id == messageId) message else it }, status = "Replying...", tool = "")
                }
                is RunEvent.ToolStarted -> {
                    val description = event.description.ifEmpty { "Using a tool..." }
                    val messageId = "assistant-${chat.runId}"
                    val existing = next.messages.find { it.id == messageId }
                    val tc = ToolCall(callId = event.callId, name = event.name, description = description)
                    val msg = if (existing != null) existing.copy(toolCalls = existing.toolCalls + tc)
                        else ChatMessage(messageId, "assistant", "", toolCalls = listOf(tc))
                    next = next.copy(
                        messages = if (existing == null) next.messages + msg else next.messages.map { if (it.id == messageId) msg else it },
                        tool = description, status = "Working...")
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
                                "open_browser" -> browserUrl.value = outputJson.optString("url", "")
                                "open_maps" -> pendingIntent.value = AppIntent(
                                    app = "maps",
                                    uri = outputJson.optString("uri", ""),
                                    fallbackUrl = "https://maps.google.com/?q=" +
                                        java.net.URLEncoder.encode(outputJson.optString("uri", ""), "UTF-8"))
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
                    scope.launch {
                        runCatching {
                            val result = executeDeviceQuery(event.query)
                            sendDeviceResult(chat.runId, event.queryId, result)
                        }
                    }
                }
                is RunEvent.Error -> next = next.copy(error = event.message.ifEmpty { "The assistant couldn’t finish." })
                is RunEvent.RunFinished -> {
                    val status = event.status
                    // Extract any card JSON embedded in assistant text
                    val messageId = "assistant-${chat.runId}"
                    next = next.copy(
                        messages = next.messages.map { if (it.id == messageId) extractCardsFromText(it) else it },
                        runId = "", status = "", tool = "", cancelRequested = false, error = when (status) {
                        "completed" -> ""
                        "cancelled" -> "Stopped"
                        "interrupted" -> "The server restarted. Send a new message to continue."
                        else -> chat.error.ifEmpty { "Run ended: $status. You can send another message." }
                    })
                }
                is RunEvent.Usage, is RunEvent.RunStarted -> {}
            }
            next
        }
    }
}
