package com.flashmind.beep

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import android.util.Base64
import com.flashmind.beep.proto.PromptSuggestion
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

data class ConnectionSettings(val nodeId: String = "", val token: String = "") {
    val configured get() = nodeId.isNotBlank()
    companion object {
        fun fromJson(raw: String): ConnectionSettings {
            val json = JSONObject(raw)
            return ConnectionSettings(json.getString("node_id"), json.optString("token"))
        }
    }
}

data class Card(val type: String, val data: JSONObject)
data class TaskStep(val label: String, val status: String)
data class ToolCall(
    val callId: String = "", val name: String = "", val description: String = "",
    val isDone: Boolean = false, val success: Boolean = true, val result: String = ""
)

data class ChatMessage(val id: String = UUID.randomUUID().toString(), val role: String, val text: String, val cards: List<Card> = emptyList(), val taskSteps: List<TaskStep>? = null, val images: List<String> = emptyList(), val toolCalls: List<ToolCall> = emptyList(), val memoryScore: Double? = null)
data class Conversation(
    val id: String = UUID.randomUUID().toString(), val title: String = NEW_CHAT_TITLE,
    val messages: List<ChatMessage> = emptyList(), val runId: String = "", val cursor: Long = 0,
    val requestId: String = "", val pendingText: String = "", val pendingImages: List<String> = emptyList(),
    val status: String = "", val error: String = "",
    val tool: String = "", val cancelRequested: Boolean = false, val pinned: Boolean = false,
    /** The model's reasoning for the current step, shown live and dropped once it moves on. Not saved. */
    val thinking: String = ""
) { val busy get() = requestId.isNotEmpty() || runId.isNotEmpty() }
const val NEW_CHAT_TITLE = "New conversation"

/** A chat opened but never used. It is not kept once the user moves on, nor listed. */
val Conversation.isUnused get() =
    messages.isEmpty() && !busy && pendingText.isEmpty() && pendingImages.isEmpty() && title == NEW_CHAT_TITLE

data class ChatState(val conversations: List<Conversation> = listOf(Conversation()), val selected: String = "") {
    val current get() = conversations.firstOrNull { it.id == selected } ?: conversations.first()
}

class LocalStorage(internal val context: Context) {
    private val prefs = context.getSharedPreferences("connection", Context.MODE_PRIVATE)
    private val file = AtomicFile(File(context.filesDir, "conversations.json"))
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey("beep-token", null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder("beep-token", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    fun loadSettings(): ConnectionSettings {
        if (!prefs.contains("nodeId") && BuildConfig.DEBUG) {
            runCatching {
                context.assets.open("testing-profile.json").bufferedReader().use { ConnectionSettings.fromJson(it.readText()) }
            }.getOrNull()?.takeIf { it.configured }?.let { saveSettings(it); return it }
        }
        val encrypted = prefs.getString("token", "").orEmpty()
        val token = if (encrypted.isEmpty()) "" else runCatching {
            val bytes = Base64.decode(encrypted, Base64.NO_WRAP)
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
            }.doFinal(bytes.copyOfRange(12, bytes.size)).toString(Charsets.UTF_8)
        }.getOrDefault("")
        return ConnectionSettings(prefs.getString("nodeId", "").orEmpty(), token)
    }
    fun saveSettings(settings: ConnectionSettings) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val encrypted = Base64.encodeToString(cipher.iv + cipher.doFinal(settings.token.toByteArray()), Base64.NO_WRAP)
        check(prefs.edit().putString("nodeId", settings.nodeId)
            .putString("token", encrypted).commit()) { "Could not save connection" }
    }
    fun loadState(): ChatState = runCatching {
        val json = JSONObject(file.openRead().bufferedReader().use { it.readText() })
        val list = json.getJSONArray("conversations")
        val conversations = (0 until list.length()).map { index ->
            val c = list.getJSONObject(index)
            val messages = c.getJSONArray("messages")
            Conversation(c.getString("id"), c.getString("title"), (0 until messages.length()).map { i ->
                val m = messages.getJSONObject(i)
                val cardsArr = m.optJSONArray("cards")
                val cards = if (cardsArr != null) (0 until cardsArr.length()).map { ci ->
                    val card = cardsArr.getJSONObject(ci)
                    Card(card.getString("type"), card.getJSONObject("data"))
                } else emptyList()
                val stepsArr = m.optJSONArray("taskSteps")
                val taskSteps = if (stepsArr != null) (0 until stepsArr.length()).map { si ->
                    val s = stepsArr.getJSONObject(si)
                    TaskStep(s.getString("label"), s.getString("status"))
                } else null
                val tcArr = m.optJSONArray("toolCalls")
                val toolCalls = if (tcArr != null) (0 until tcArr.length()).map { tci ->
                    val tc = tcArr.getJSONObject(tci)
                    ToolCall(tc.optString("callId"), tc.optString("name"), tc.optString("description"),
                        tc.optBoolean("isDone"), tc.optBoolean("success", true), tc.optString("result"))
                } else emptyList()
                ChatMessage(m.getString("id"), m.getString("role"), m.getString("text"), cards, taskSteps, readImages(m, "images", "imageBase64"), toolCalls,
                    m.optDouble("memoryScore").takeUnless { it.isNaN() })
            }, c.optString("runId"), c.optLong("cursor"), c.optString("requestId"), c.optString("pendingText"),
                pendingImages = readImages(c, "pendingImages", "pendingImage"),
                status = c.optString("status"), cancelRequested = c.optBoolean("cancelRequested"), pinned = c.optBoolean("pinned"))
        }
        check(conversations.isNotEmpty())
        ChatState(conversations, json.optString("selected"))
    }.getOrElse { ChatState() }

    /** The last personal starters and when they were fetched, in epoch milliseconds. */
    fun loadStarters(): Pair<List<PromptSuggestion>, Long> = runCatching {
        val json = JSONObject(prefs.getString("starters", null) ?: return@runCatching null)
        val items = json.getJSONArray("items")
        val list = (0 until items.length()).map { i ->
            items.getJSONObject(i).let { PromptSuggestion(it.getString("title"), it.getString("prompt")) }
        }
        list to json.getLong("fetched_at")
    }.getOrNull() ?: (emptyList<PromptSuggestion>() to 0L)

    fun saveStarters(starters: List<PromptSuggestion>, fetchedAt: Long) {
        val items = JSONArray()
        starters.forEach { items.put(JSONObject().put("title", it.title).put("prompt", it.prompt)) }
        prefs.edit().putString("starters", JSONObject().put("items", items).put("fetched_at", fetchedAt).toString()).apply()
    }

    var onboardingDone: Boolean
        get() = prefs.getBoolean("onboarding_done", false)
        set(value) { prefs.edit().putBoolean("onboarding_done", value).apply() }

    fun deviceId(): String {
        val existing = prefs.getString("device_id", null)
        if (existing != null) return existing
        val id = UUID.randomUUID().toString()
        prefs.edit().putString("device_id", id).apply()
        return id
    }

    fun saveState(state: ChatState) {
        val list = JSONArray()
        state.conversations.forEach { c ->
            val messages = JSONArray()
            c.messages.forEach { m ->
                val msg = JSONObject().put("id", m.id).put("role", m.role).put("text", m.text).apply { if (m.images.isNotEmpty()) put("images", JSONArray(m.images)) }
                    .apply { if (m.memoryScore != null) put("memoryScore", m.memoryScore) }
                if (m.cards.isNotEmpty()) {
                    val cardsArr = JSONArray()
                    m.cards.forEach { card -> cardsArr.put(JSONObject().put("type", card.type).put("data", card.data)) }
                    msg.put("cards", cardsArr)
                }
                if (!m.taskSteps.isNullOrEmpty()) {
                    val stepsArr = JSONArray()
                    m.taskSteps.forEach { step -> stepsArr.put(JSONObject().put("label", step.label).put("status", step.status)) }
                    msg.put("taskSteps", stepsArr)
                }
                if (m.toolCalls.isNotEmpty()) {
                    val tcArr = JSONArray()
                    m.toolCalls.forEach { tc -> tcArr.put(JSONObject().put("callId", tc.callId).put("name", tc.name)
                        .put("description", tc.description).put("isDone", tc.isDone).put("success", tc.success).put("result", tc.result)) }
                    msg.put("toolCalls", tcArr)
                }
                messages.put(msg)
            }
            list.put(JSONObject().put("id", c.id).put("title", c.title).put("messages", messages)
                .put("runId", c.runId).put("cursor", c.cursor).put("requestId", c.requestId)
                .put("pendingText", c.pendingText).apply { if (c.pendingImages.isNotEmpty()) put("pendingImages", JSONArray(c.pendingImages)) }
                .put("status", c.status).put("cancelRequested", c.cancelRequested)
                .put("pinned", c.pinned))
        }
        val out = file.startWrite()
        try {
            out.write(JSONObject().put("selected", state.selected).put("conversations", list).toString().toByteArray())
            file.finishWrite(out)
        } catch (error: Exception) { file.failWrite(out); throw error }
    }
}

/** The images saved under [key], or the single image older versions saved under [legacyKey]. */
private fun readImages(json: JSONObject, key: String, legacyKey: String): List<String> {
    val list = json.optJSONArray(key)
    if (list != null) return (0 until list.length()).map { list.getString(it) }
    return listOfNotNull(json.optString(legacyKey, null))
}
