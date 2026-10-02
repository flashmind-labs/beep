package com.flashmind.neobeep

import android.app.Application
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import java.util.UUID

class BeepApplication : Application() {
    val repository by lazy { ChatRepository(LocalStorage(this)) }
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
        val entries = JSONObject(QuicApi(config).json("GET", "/v1/sessions/$id")).getJSONArray("entries")
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
        val sessions = JSONObject(QuicApi(config).json("GET", "/v1/sessions")).getJSONArray("sessions")
        if (settings.value != config) return
        update { state ->
            val known = state.conversations.map { it.id }.toSet()
            val added = (0 until sessions.length()).mapNotNull { i ->
                val item = sessions.getJSONObject(i)
                if (item.getString("id") in known) null else Conversation(id = item.getString("id"), title = item.getString("title"))
            }
            state.copy(conversations = state.conversations + added)
        }
        if ((0 until sessions.length()).any { sessions.getJSONObject(it).getString("id") == state.value.current.id })
            loadHistory(state.value.current.id, config)
    }
    private suspend fun connectionLabel(config: ConnectionSettings): String {
        val mode = JSONObject(QuicApi(config).json("GET", "/v1/auth")).optString("mode")
        return if (mode == "testing") "Testing · shared account · QUIC" else "Connected · QUIC"
    }
    fun newChat() = update { state ->
        val chat = Conversation(); state.copy(conversations = listOf(chat) + state.conversations, selected = chat.id)
    }
    suspend fun connect(config: ConnectionSettings) {
        require(config.configured) { "Enter your server and TLS hostname." }
        connection.value = "Connecting…"
        val health = JSONObject(QuicApi(config).json("GET", "/v1/health"))
        check(health.optString("protocol") == "h3") { "This server does not report HTTP/3." }
        val changedAccount = settings.value.address != config.address || settings.value.token != config.token
        synchronized(lock) { jobs.values.forEach { it.cancel() }; jobs.clear() }
        storage.saveSettings(config)
        settings.value = config
        if (changedAccount) update { ChatState() }
        connected.value = true
        scope.launch { runCatching { registerDevice(storage.deviceId(), "pending-fcm-setup") } }
        NotificationService.start(storage.context)
        connection.value = connectionLabel(config)
        resumePending()
        runCatching { syncHistory(config) }
    }
    fun reconnect() {
        reconnectJob?.cancel()
        reconnectJob = scope.launch {
            var attempt = 0
            while (isActive) {
                try {
                    connection.value = if (attempt > 0) "Reconnecting… (attempt ${attempt + 1})" else "Checking connection…"
                    QuicApi(settings.value).json("GET", "/v1/health")
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
    fun send(text: String) {
        val chat = state.value.current
        if (text.isBlank() || chat.busy || !settings.value.configured) return
        val request = UUID.randomUUID().toString()
        updateChat(chat.id) { it.copy(title = if (it.messages.isEmpty()) text.take(44) else it.title,
            messages = it.messages + ChatMessage(role = "user", text = text.trim()),
            requestId = request, pendingText = text.trim(), status = "Sending…", error = "", cancelRequested = false) }
        launchRun(chat.id)
    }
    fun retry() { launchRun(state.value.current.id) }
    fun cancel() {
        val chat = state.value.current
        updateChat(chat.id) { it.copy(cancelRequested = true, status = "Stopping…") }
        scope.launch {
            if (chat.runId.isEmpty()) { retry(); return@launch }
            try { QuicApi(settings.value).json("POST", "/v1/runs/${chat.runId}/cancel") }
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
                val api = QuicApi(settings.value)
                try {
                    updateChat(id) { it.copy(error = "", status = if (it.runId.isEmpty()) "Sending…" else "Beep is thinking…") }
                    if (chat.runId.isEmpty()) {
                        val body = JSONObject().put("session_id", id).put("request_id", chat.requestId).put("message", chat.pendingText)
                        val run = JSONObject(api.json("POST", "/v1/runs", body.toString()))
                        updateChat(id) { it.copy(runId = run.getString("id"), requestId = "", pendingText = "", cursor = 0) }
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
        val result = JSONObject(QuicApi(settings.value).json("GET", "/v1/memories"))
        val arr = result.getJSONArray("memories")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { m -> m.getString("key") to m.getString("content") } }
    }
    suspend fun deleteMemory(key: String) {
        QuicApi(settings.value).json("DELETE", "/v1/memories", JSONObject().put("key", key).toString())
    }

    data class Cron(val id: String, val label: String, val prompt: String, val schedule: String, val enabled: Boolean)
    suspend fun listCrons(): List<Cron> {
        val result = JSONObject(QuicApi(settings.value).json("GET", "/v1/crons"))
        val arr = result.getJSONArray("crons")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { c ->
            Cron(c.getString("id"), c.getString("label"), c.getString("prompt"), c.getString("schedule"), c.optBoolean("enabled", true))
        } }
    }
    suspend fun createCron(label: String, prompt: String, schedule: String) {
        QuicApi(settings.value).json("POST", "/v1/crons", JSONObject().put("label", label).put("prompt", prompt).put("schedule", schedule).toString())
    }
    suspend fun deleteCron(id: String) {
        QuicApi(settings.value).json("DELETE", "/v1/crons/$id")
    }
    suspend fun toggleCron(id: String, enabled: Boolean) {
        QuicApi(settings.value).json("PATCH", "/v1/crons/$id", JSONObject().put("enabled", enabled).toString())
    }

    data class McpConnection(val id: String, val provider: String, val label: String, val status: String)
    suspend fun listIntegrations(): List<McpConnection> {
        val result = JSONObject(QuicApi(settings.value).json("GET", "/v1/integrations"))
        val arr = result.getJSONArray("integrations")
        return (0 until arr.length()).map { arr.getJSONObject(it).let { c ->
            McpConnection(c.getString("id"), c.getString("provider"), c.getString("label"), c.getString("status"))
        } }
    }
    suspend fun createIntegration(provider: String, label: String, config: String = "{}") {
        QuicApi(settings.value).json("POST", "/v1/integrations",
            JSONObject().put("provider", provider).put("label", label).put("config", config).toString())
    }
    suspend fun uploadCredentials(id: String, credentials: String) {
        QuicApi(settings.value).json("PUT", "/v1/integrations/$id/credentials",
            JSONObject().put("credentials", JSONObject(credentials)).put("status", "connected").toString())
    }
    suspend fun deleteIntegration(id: String) {
        QuicApi(settings.value).json("DELETE", "/v1/integrations/$id")
    }

    suspend fun registerDevice(deviceId: String, fcmToken: String) {
        QuicApi(settings.value).json("POST", "/v1/devices",
            JSONObject().put("device_id", deviceId).put("fcm_token", fcmToken).toString())
    }

    suspend fun updateLocation(latitude: Double, longitude: Double, accuracy: Float) {
        val tz = java.util.TimeZone.getDefault().id
        QuicApi(settings.value).json("POST", "/v1/location",
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
                    if (output.contains("\"action\"")) {
                        try {
                            val outputJson = JSONObject(output)
                            when (outputJson.optString("action")) {
                                "open_app" -> pendingIntent.value = AppIntent(
                                    app = outputJson.optString("app", ""),
                                    uri = outputJson.optString("uri", ""),
                                    fallbackUrl = outputJson.optString("fallback_url", ""))
                                "open_browser" -> browserUrl.value = outputJson.optString("url", "")
                            }
                        } catch (_: Exception) {}
                    }
                }
                "error" -> next = next.copy(error = event.optString("message", "The assistant couldn’t finish."))
                "run_finished" -> {
                    val status = event.getString("status")
                    next = next.copy(runId = "", status = "", tool = "", cancelRequested = false, error = when (status) {
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
