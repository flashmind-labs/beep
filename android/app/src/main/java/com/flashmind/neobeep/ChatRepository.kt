package com.flashmind.neobeep

import android.Manifest
import android.app.Application
import android.content.pm.PackageManager
import android.provider.ContactsContract
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
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
        val entries = JSONObject(serverApi(config).json("GET", "/v1/sessions/$id")).getJSONArray("entries")
        val messages = (0 until entries.length()).mapNotNull { i ->
            val entry = entries.getJSONObject(i)
            val role = entry.optString("role")
            if (role !in listOf("user", "assistant")) null
            else ChatMessage("history-$id-$i", role, entry.getString("content"))
        }
        if (settings.value != config) return
        updateChat(id) { current -> if (current == before) current.copy(messages = messages) else current }
    }
    fun refreshHistory() { scope.launch { runCatching { syncHistory(settings.value) } } }
    private suspend fun syncHistory(config: ConnectionSettings) {
        val sessions = JSONObject(serverApi(config).json("GET", "/v1/sessions")).getJSONArray("sessions")
        if (settings.value != config) return
        update { state ->
            val known = state.conversations.map { it.id }.toSet()
            val serverPinned = (0 until sessions.length()).associate { i ->
                val item = sessions.getJSONObject(i)
                item.getString("id") to item.optBoolean("pinned", false)
            }
            val added = (0 until sessions.length()).mapNotNull { i ->
                val item = sessions.getJSONObject(i)
                if (item.getString("id") in known) null else Conversation(id = item.getString("id"), title = item.getString("title"), pinned = item.optBoolean("pinned", false))
            }
            // Update pinned status from server for existing conversations
            val updated = state.conversations.map { c ->
                val pin = serverPinned[c.id]
                if (pin != null) c.copy(pinned = pin) else c
            }
            state.copy(conversations = (updated + added).sortedByDescending { it.pinned })
        }
        if ((0 until sessions.length()).any { sessions.getJSONObject(it).getString("id") == state.value.current.id })
            loadHistory(state.value.current.id, config)
    }
    private suspend fun connectionLabel(config: ConnectionSettings): String {
        val proto = if (config.transport == "http") "HTTP" else "QUIC"
        val mode = JSONObject(serverApi(config).json("GET", "/v1/auth")).optString("mode")
        return if (mode == "testing") "Testing · shared account · $proto" else "Connected · $proto"
    }
    fun newChat() = update { state ->
        val chat = Conversation(); state.copy(conversations = listOf(chat) + state.conversations, selected = chat.id)
    }
    suspend fun connect(config: ConnectionSettings) {
        require(config.configured) { "Enter your server and TLS hostname." }
        connection.value = "Connecting…"
        // Try the requested transport, fall back to HTTP if QUIC fails.
        val effective = if (config.transport != "http") {
            try {
                val health = JSONObject(serverApi(config).json("GET", "/v1/health"))
                check(health.optString("status") == "ok")
                config
            } catch (_: Exception) {
                val httpConfig = config.copy(transport = "http")
                val health = JSONObject(serverApi(httpConfig).json("GET", "/v1/health"))
                check(health.optString("status") == "ok") { "Server not reachable over QUIC or HTTP." }
                httpConfig
            }
        } else {
            val health = JSONObject(serverApi(config).json("GET", "/v1/health"))
            check(health.optString("status") == "ok") { "Server not reachable." }
            config
        }
        val changedAccount = settings.value.address != effective.address || settings.value.token != effective.token
        synchronized(lock) { jobs.values.forEach { it.cancel() }; jobs.clear() }
        storage.saveSettings(effective)
        settings.value = effective
        if (changedAccount) update { ChatState() }
        connected.value = true
        scope.launch { runCatching { registerDevice(storage.deviceId(), "pending-fcm-setup") } }
        NotificationService.start(storage.context)
        connection.value = connectionLabel(effective)
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
                    // Try current transport, fall back to HTTP if QUIC fails on first attempt.
                    val current = settings.value
                    try {
                        serverApi(current).json("GET", "/v1/health")
                    } catch (_: Exception) {
                        if (current.transport != "http") {
                            val httpConfig = current.copy(transport = "http")
                            serverApi(httpConfig).json("GET", "/v1/health")
                            storage.saveSettings(httpConfig)
                            settings.value = httpConfig
                        } else throw Exception("Server not reachable")
                    }
                    connected.value = true
                    scope.launch { runCatching { registerDevice(storage.deviceId(), "pending-fcm-setup") } }
                    NotificationService.start(storage.context)
                    connection.value = connectionLabel(settings.value)
                    resumePending()
                    runCatching { syncHistory(settings.value) }
                    return@launch
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    attempt++
                    connected.value = false
                    connection.value = if (attempt < 5) "Reconnecting…" else "Can’t reach your server · tap to retry"
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
            try { serverApi(settings.value).json("POST", "/v1/runs/${chat.runId}/cancel") }
            catch (error: Exception) { updateChat(chat.id) { it.copy(error = error.message.orEmpty()) } }
        }
    }
    private fun launchRun(id: String) = synchronized(lock) {
        if (jobs[id]?.isActive == true) return@synchronized
        jobs[id] = scope.launch {
            var failures = 0
            while (isActive) {
                val chat = state.value.conversations.find { it.id == id } ?: break
                if (!chat.busy) break
                val api = serverApi(settings.value)
                try {
                    updateChat(id) { it.copy(error = "", status = if (it.runId.isEmpty()) "Sending…" else "Beep is thinking…") }
                    if (chat.runId.isEmpty()) {
                        val body = JSONObject().put("session_id", id).put("request_id", chat.requestId).put("message", chat.pendingText)
                        if (chat.pendingImage != null) body.put("image", chat.pendingImage)
                        val run = JSONObject(api.json("POST", "/v1/runs", body.toString()))
                        updateChat(id) { it.copy(runId = run.getString("id"), requestId = "", pendingText = "", pendingImage = null, cursor = 0) }
                    }
                    val current = state.value.conversations.first { it.id == id }
                    if (current.cancelRequested) api.json("POST", "/v1/runs/${current.runId}/cancel")
                    val decoder = NdjsonDecoder { line -> event(id, JSONObject(line)) }
                    val status = api.request("GET", "/v1/runs/${current.runId}/events?after=${current.cursor}") { decoder.feed(it) }
                    api.checkStatus(status)
                    decoder.finish()
                    failures = 0; connected.value = true
                    // Bounded server subscriptions may end before the run. Reopen from the cursor.
                    delay(150)
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    failures++
                    connected.value = false; connection.value = "Connection interrupted"
                    updateChat(id) { it.copy(status = "", error = if (failures < 4) "Reconnecting…" else (error.message ?: "Connection failed. Tap Retry.")) }
                    if (failures >= 4) { reconnect(); break }
                    delay(1000L shl (failures - 1))
                }
            }
        }
    }
    suspend fun listMemories(): List<Pair<String, String>> {
        val result = JSONObject(serverApi(settings.value).json("GET", "/v1/memories"))
        val arr = result.getJSONArray("memories")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { m -> m.getString("key") to m.getString("content") } }
    }
    suspend fun deleteMemory(key: String) {
        serverApi(settings.value).json("DELETE", "/v1/memories", JSONObject().put("key", key).toString())
    }

    data class Cron(val id: String, val label: String, val prompt: String, val schedule: String, val enabled: Boolean)
    suspend fun listCrons(): List<Cron> {
        val result = JSONObject(serverApi(settings.value).json("GET", "/v1/crons"))
        val arr = result.getJSONArray("crons")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { c ->
            Cron(c.getString("id"), c.getString("label"), c.getString("prompt"), c.getString("schedule"), c.optBoolean("enabled", true))
        } }
    }
    suspend fun createCron(label: String, prompt: String, schedule: String) {
        serverApi(settings.value).json("POST", "/v1/crons", JSONObject().put("label", label).put("prompt", prompt).put("schedule", schedule).toString())
    }
    suspend fun deleteCron(id: String) {
        serverApi(settings.value).json("DELETE", "/v1/crons/$id")
    }
    suspend fun toggleCron(id: String, enabled: Boolean) {
        serverApi(settings.value).json("PATCH", "/v1/crons/$id", JSONObject().put("enabled", enabled).toString())
    }

    data class McpConnection(val id: String, val provider: String, val label: String, val status: String)
    suspend fun listIntegrations(): List<McpConnection> {
        val result = JSONObject(serverApi(settings.value).json("GET", "/v1/integrations"))
        val arr = result.getJSONArray("integrations")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { c ->
            McpConnection(c.getString("id"), c.getString("provider"), c.getString("label"), c.getString("status"))
        } }
    }
    suspend fun createIntegration(provider: String, label: String, config: String = "{}") {
        serverApi(settings.value).json("POST", "/v1/integrations",
            JSONObject().put("provider", provider).put("label", label).put("config", config).toString())
    }
    suspend fun uploadCredentials(id: String, credentials: String) {
        serverApi(settings.value).json("PUT", "/v1/integrations/$id/credentials",
            JSONObject().put("credentials", JSONObject(credentials)).put("status", "connected").toString())
    }
    suspend fun deleteIntegration(id: String) {
        serverApi(settings.value).json("DELETE", "/v1/integrations/$id")
    }

    suspend fun renameSession(id: String, title: String) {
        serverApi(settings.value).json("PUT", "/v1/sessions/$id", JSONObject().put("title", title).toString())
        updateChat(id) { it.copy(title = title) }
    }

    suspend fun deleteSession(id: String) {
        serverApi(settings.value).json("DELETE", "/v1/sessions/$id")
        update { state ->
            val remaining = state.conversations.filter { it.id != id }
            val safe = remaining.ifEmpty { listOf(Conversation()) }
            state.copy(conversations = safe, selected = if (state.selected == id) safe.first().id else state.selected)
        }
    }

    suspend fun pinSession(id: String, pinned: Boolean) {
        serverApi(settings.value).json("PUT", "/v1/sessions/$id/pin", JSONObject().put("pinned", pinned).toString())
        updateChat(id) { it.copy(pinned = pinned) }
        update { state ->
            state.copy(conversations = state.conversations.sortedByDescending { it.pinned })
        }
    }

    suspend fun registerDevice(deviceId: String, fcmToken: String) {
        serverApi(settings.value).json("POST", "/v1/devices",
            JSONObject().put("device_id", deviceId).put("fcm_token", fcmToken).toString())
    }

    /** One-shot translation: creates a temporary session, sends the prompt, streams the response, and returns the full text. */
    suspend fun translate(text: String, language: String): String {
        val config = settings.value
        require(config.configured) { "Server not configured" }
        val api = serverApi(config)
        val sessionId = UUID.randomUUID().toString()
        val prompt = if (language == "Auto") "Translate the following text to the most likely target language (if it's English, translate to Spanish; otherwise translate to English). Reply with ONLY the translation, no explanation:\n\n$text"
            else "Translate the following text to $language. Reply with ONLY the translation, no explanation:\n\n$text"
        val body = JSONObject().put("session_id", sessionId).put("request_id", UUID.randomUUID().toString()).put("message", prompt)
        val run = JSONObject(api.json("POST", "/v1/runs", body.toString()))
        val runId = run.getString("id")
        val result = StringBuilder()
        var cursor = 0L
        var finished = false
        while (!finished) {
            val decoder = NdjsonDecoder { line ->
                val event = JSONObject(line)
                val seq = event.optLong("seq", -1)
                if (seq > cursor) cursor = seq
                when (event.optString("type")) {
                    "text_delta" -> result.append(event.getString("delta"))
                    "run_finished" -> finished = true
                    "error" -> throw Exception(event.optString("message", "Translation failed"))
                }
            }
            val status = api.request("GET", "/v1/runs/$runId/events?after=$cursor") { decoder.feed(it) }
            api.checkStatus(status)
            decoder.finish()
            if (!finished) kotlinx.coroutines.delay(150)
        }
        return result.toString().trim()
    }

    suspend fun updateLocation(latitude: Double, longitude: Double, accuracy: Float) {
        val tz = java.util.TimeZone.getDefault().id
        serverApi(settings.value).json("POST", "/v1/location",
            JSONObject().put("latitude", latitude).put("longitude", longitude).put("accuracy", accuracy.toDouble()).put("timezone", tz).toString())
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

    private suspend fun executeDeviceQuery(query: JSONObject): JSONObject {
        val action = query.optString("action", "unknown")
        return when (action) {
            "query_contacts" -> {
                val name = query.optString("name", "")
                queryContacts(name)
            }
            "browser_command" -> {
                val browserAction = query.optString("browser_action", "")
                if (browserAction == "navigate") {
                    val url = query.optString("url", "")
                    if (url.isNotEmpty()) {
                        browserUrl.value = url
                        // Give the WebView a moment to open and load
                        kotlinx.coroutines.delay(2000)
                        JSONObject().put("navigated", true).put("url", url)
                    } else {
                        JSONObject().put("error", "missing_url")
                            .put("message", "URL is required for navigate action")
                    }
                } else {
                    BrowserBridge.executeCommand(browserAction, query)
                }
            }
            else -> JSONObject().put("error", "not_implemented").put("action", action)
        }
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
        val body = JSONObject().put("query_id", queryId).put("result", result)
        serverApi(settings.value).json("POST", "/v1/runs/$runId/device-result", body.toString())
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

    private fun event(id: String, event: JSONObject) {
        val sequence = event.optLong("seq", -1)
        updateChat(id) { chat ->
            if (event.optString("run_id") != chat.runId || sequence <= chat.cursor) return@updateChat chat
            check(sequence == chat.cursor + 1) { "Missing server event; reconnecting" }
            var next = chat.copy(cursor = sequence)
            when (event.optString("type")) {
                "text_delta" -> {
                    val messageId = "assistant-${chat.runId}"
                    val existing = chat.messages.find { it.id == messageId }
                    val message = ChatMessage(messageId, "assistant", existing?.text.orEmpty() + event.getString("delta"))
                    next = next.copy(messages = if (existing == null) chat.messages + message else chat.messages.map { if (it.id == messageId) message else it }, status = "Replying…", tool = "")
                }
                "tool_started" -> next = next.copy(tool = event.optString("description", "Using a tool…"), status = "Working…")
                "tool_result" -> {
                    next = next.copy(tool = if (event.optBoolean("success")) "Done" else "Tool didn’t complete")
                    val output = event.optString("output", "")
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
                "device_query" -> {
                    val queryId = event.getString("query_id")
                    val query = event.getJSONObject("query")
                    scope.launch {
                        runCatching {
                            val result = executeDeviceQuery(query)
                            sendDeviceResult(chat.runId, queryId, result)
                        }
                    }
                }
                "error" -> next = next.copy(error = event.optString("message", "The assistant couldn’t finish."))
                "run_finished" -> {
                    val status = event.getString("status")
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
            }
            next
        }
    }
}
