package com.flashmind.beep

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import android.util.Base64
import com.flashmind.beep.ffi.ListCache
import com.flashmind.beep.ffi.decodeListCache
import com.flashmind.beep.ffi.encodeListCache
import com.flashmind.beep.ffi.newIdentity
import com.flashmind.beep.proto.Approval
import com.flashmind.beep.proto.ApprovalField
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

data class ConnectionSettings(val nodeId: String = "") {
    val configured get() = nodeId.isNotBlank()
    companion object {
        fun fromJson(raw: String): ConnectionSettings = ConnectionSettings(JSONObject(raw).getString("node_id"))
    }
}

/** A connection profile file: the server and, when exported from a phone, its identity. */
class Profile(val settings: ConnectionSettings, val identity: ByteArray?) {
    fun toJson(): String = JSONObject().put("node_id", settings.nodeId)
        .apply { identity?.let { put("identity", it.joinToString("") { b -> "%02x".format(b) }) } }.toString()
    companion object {
        fun fromJson(raw: String): Profile {
            val hex = JSONObject(raw).optString("identity")
            val identity = hex.takeIf { it.isNotEmpty() }?.let {
                require(it.length == 64 && it.all { c -> c in "0123456789abcdefABCDEF" }) { "The profile's identity is not a key" }
                it.chunked(2).map { b -> b.toInt(16).toByte() }.toByteArray()
            }
            return Profile(ConnectionSettings.fromJson(raw), identity)
        }
    }
}

data class Card(val type: String, val data: JSONObject)
data class TaskStep(val label: String, val status: String)
data class ToolCall(
    val callId: String = "", val name: String = "", val description: String = "",
    val isDone: Boolean = false, val success: Boolean = true, val result: String = ""
)

data class ChatMessage(val id: String = UUID.randomUUID().toString(), val role: String, val text: String, val cards: List<Card> = emptyList(), val taskSteps: List<TaskStep>? = null, val images: List<String> = emptyList(), val toolCalls: List<ToolCall> = emptyList(), val memoryScore: Double? = null,
    /** The model's reasoning before this reply, folded under a "Thoughts" row. */
    val reasoning: String = "",
    /** Tool calls waiting on, or answered by, the user. */
    val approvals: List<ApprovalPrompt> = emptyList(),
    /** The model an admin reran this prompt on, as a `ModelChoice` id. Empty for the usual model. */
    val model: String = "",
    /** When the server saved this prompt, in epoch milliseconds; 0 until the chat syncs. Names the turn to delete. */
    val sentAt: Long = 0)

/** A tool call the server holds until the user allows it. `answer` is empty while it waits. */
data class ApprovalPrompt(val queryId: String, val approval: Approval, val answer: String = "") {
    val waiting get() = answer.isEmpty()
}
data class Conversation(
    val id: String = UUID.randomUUID().toString(), val title: String = NEW_CHAT_TITLE,
    val messages: List<ChatMessage> = emptyList(), val runId: String = "", val cursor: Long = 0,
    val requestId: String = "", val pendingText: String = "", val pendingImages: List<String> = emptyList(),
    val pendingModel: String = "",
    val status: String = "", val error: String = "",
    val tool: String = "", val cancelRequested: Boolean = false, val pinned: Boolean = false,
    /** The model's reasoning for the current step, shown live and moved onto the reply once it moves on. Not saved. */
    val thinking: String = "",
    /** The server's `updated_at` of the turns in [messages], 0 when they did not come from the server. */
    val version: Long = 0,
    /** When the user last opened the chat, in epoch milliseconds. */
    val openedAt: Long = System.currentTimeMillis(),
    /** The server's `updated_at` as last listed, kept even when [messages] are evicted. 0 until listed. */
    val updatedAt: Long = 0
) {
    val busy get() = requestId.isNotEmpty() || runId.isNotEmpty()
    /** When the chat last changed, as far as the phone knows, in epoch milliseconds. */
    val lastActive get() = maxOf(updatedAt, messages.maxOfOrNull { it.sentAt } ?: 0).takeIf { it > 0 } ?: openedAt
}
const val NEW_CHAT_TITLE = "New conversation"

/** A chat opened but never used. It is not kept once the user moves on, nor listed. */
val Conversation.isUnused get() =
    messages.isEmpty() && !busy && pendingText.isEmpty() && pendingImages.isEmpty() && title == NEW_CHAT_TITLE

/** A change to the chat list made on the phone, kept until the server has it. */
sealed interface ChatChange {
    data class Delete(val ids: List<String>) : ChatChange
    data class Rename(val id: String, val title: String) : ChatChange
    data class Pin(val id: String, val pinned: Boolean) : ChatChange
}

data class Draft(val text: String = "", val images: List<String> = emptyList())

data class ChatState(val conversations: List<Conversation> = listOf(Conversation()), val selected: String = "") {
    val current get() = conversations.firstOrNull { it.id == selected } ?: conversations.first()
}

private val identityLock = Any()
/**
 * Raised when cached chats need a reload from the server. 2: prompts sent from the
 * phone lacked the server time that deleting a turn needs.
 */
private const val CACHE_FORMAT = 2
/** Shared by every `LocalStorage`, so the keystore is read once per process. */
@Volatile private var cachedIdentity: ByteArray? = null

class LocalStorage(internal val context: Context) {
    private val prefs = context.getSharedPreferences("connection", Context.MODE_PRIVATE)
    private val file = AtomicFile(File(context.filesDir, "conversations.json"))
    private val changesFile = AtomicFile(File(context.filesDir, "pending-changes.json"))
    private val listsFile = AtomicFile(File(context.filesDir, "lists.bin"))
    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey("beep-identity", null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder("beep-identity", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }
    /** The server picked in Connection settings, else the one built into the app. */
    fun loadSettings(): ConnectionSettings =
        ConnectionSettings(prefs.getString("nodeId", null) ?: BuildConfig.SERVER_NODE_ID)
    fun hasSavedSettings(): Boolean = prefs.contains("nodeId")
    fun saveSettings(settings: ConnectionSettings) {
        check(prefs.edit().putString("nodeId", settings.nodeId).remove("token").commit()) { "Could not save connection" }
    }
    /** The saved iroh identity, or null if this phone has none yet. */
    fun storedIdentity(): ByteArray? = synchronized(identityLock) {
        cachedIdentity?.let { return it }
        val encrypted = prefs.getString("identity", null) ?: return null
        runCatching {
            val bytes = Base64.decode(encrypted, Base64.NO_WRAP)
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
            }.doFinal(bytes.copyOfRange(12, bytes.size))
        }.getOrNull()?.takeIf { it.size == 32 }?.also { cachedIdentity = it }
    }
    /** This phone's iroh identity, which is the user's account. Made on first use. */
    fun identity(): ByteArray = synchronized(identityLock) {
        storedIdentity() ?: newIdentity().also { saveIdentity(it) }
    }
    fun saveIdentity(identity: ByteArray) = synchronized(identityLock) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val encrypted = Base64.encodeToString(cipher.iv + cipher.doFinal(identity), Base64.NO_WRAP)
        check(prefs.edit().putString("identity", encrypted).commit()) { "Could not save identity" }
        cachedIdentity = identity
    }
    fun loadState(): ChatState = runCatching {
        val json = JSONObject(file.openRead().bufferedReader().use { it.readText() })
        val list = json.getJSONArray("conversations")
        // An older cache keeps its chats but forgets their version, so each reloads on open.
        val stale = json.optInt("cache") < CACHE_FORMAT
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
                val apArr = m.optJSONArray("approvals")
                val approvals = if (apArr != null) (0 until apArr.length()).map { ai ->
                    val a = apArr.getJSONObject(ai)
                    val fArr = a.getJSONArray("fields")
                    val fields = (0 until fArr.length()).map { fi ->
                        val f = fArr.getJSONObject(fi)
                        ApprovalField(f.getString("label"), f.getString("value"))
                    }
                    ApprovalPrompt(a.getString("queryId"), Approval(a.getString("app"), a.getString("action"), fields), a.optString("answer"))
                } else emptyList()
                ChatMessage(m.getString("id"), m.getString("role"), m.getString("text"), cards, taskSteps, readImages(m, "images", "imageBase64"), toolCalls,
                    m.optDouble("memoryScore").takeUnless { it.isNaN() }, m.optString("reasoning"), approvals, m.optString("model"), m.optLong("sentAt"))
            }, c.optString("runId"), c.optLong("cursor"), c.optString("requestId"), c.optString("pendingText"),
                pendingImages = readImages(c, "pendingImages", "pendingImage"), pendingModel = c.optString("pendingModel"),
                status = c.optString("status"), cancelRequested = c.optBoolean("cancelRequested"), pinned = c.optBoolean("pinned"),
                version = if (stale) 0 else c.optLong("version"), openedAt = c.optLong("openedAt", System.currentTimeMillis()),
                updatedAt = c.optLong("updatedAt"))
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

    fun loadDrafts(): Map<String, String> = runCatching {
        val json = JSONObject(prefs.getString("drafts", null) ?: return@runCatching null)
        json.keys().asSequence().associateWith { json.getString(it) }
    }.getOrNull().orEmpty()

    fun saveDrafts(drafts: Map<String, String>) {
        prefs.edit().putString("drafts", JSONObject(drafts).toString()).apply()
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

    /** The settings lists last fetched from the server. */
    fun loadLists(): ListCache =
        runCatching { decodeListCache(listsFile.readFully()) }.getOrElse { ListCache(null, null, null, null) }

    fun saveLists(lists: ListCache) {
        val out = listsFile.startWrite()
        try {
            out.write(encodeListCache(lists))
            listsFile.finishWrite(out)
        } catch (error: Exception) { listsFile.failWrite(out); throw error }
    }

    fun loadChanges(): List<ChatChange> = runCatching {
        val list = JSONArray(changesFile.openRead().bufferedReader().use { it.readText() })
        (0 until list.length()).mapNotNull { i ->
            val c = list.getJSONObject(i)
            when (c.getString("kind")) {
                "delete" -> c.getJSONArray("ids").let { ids -> ChatChange.Delete((0 until ids.length()).map { ids.getString(it) }) }
                "rename" -> ChatChange.Rename(c.getString("id"), c.getString("title"))
                "pin" -> ChatChange.Pin(c.getString("id"), c.getBoolean("pinned"))
                else -> null
            }
        }
    }.getOrElse { emptyList() }

    fun saveChanges(changes: List<ChatChange>) {
        val list = JSONArray()
        changes.forEach { change ->
            list.put(when (change) {
                is ChatChange.Delete -> JSONObject().put("kind", "delete").put("ids", JSONArray(change.ids))
                is ChatChange.Rename -> JSONObject().put("kind", "rename").put("id", change.id).put("title", change.title)
                is ChatChange.Pin -> JSONObject().put("kind", "pin").put("id", change.id).put("pinned", change.pinned)
            })
        }
        val out = changesFile.startWrite()
        try {
            out.write(list.toString().toByteArray())
            changesFile.finishWrite(out)
        } catch (error: Exception) { changesFile.failWrite(out); throw error }
    }

    fun saveState(state: ChatState) {
        val list = JSONArray()
        state.conversations.forEach { c ->
            val messages = JSONArray()
            c.messages.forEach { m ->
                val msg = JSONObject().put("id", m.id).put("role", m.role).put("text", m.text).apply { if (m.images.isNotEmpty()) put("images", JSONArray(m.images)) }
                    .apply { if (m.memoryScore != null) put("memoryScore", m.memoryScore) }
                    .apply { if (m.reasoning.isNotEmpty()) put("reasoning", m.reasoning) }
                    .apply { if (m.model.isNotEmpty()) put("model", m.model) }
                    .apply { if (m.sentAt != 0L) put("sentAt", m.sentAt) }
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
                if (m.approvals.isNotEmpty()) {
                    val apArr = JSONArray()
                    m.approvals.forEach { a ->
                        val fields = JSONArray()
                        a.approval.fields.forEach { f -> fields.put(JSONObject().put("label", f.label).put("value", f.value)) }
                        apArr.put(JSONObject().put("queryId", a.queryId).put("answer", a.answer).put("app", a.approval.app)
                            .put("action", a.approval.action).put("fields", fields))
                    }
                    msg.put("approvals", apArr)
                }
                messages.put(msg)
            }
            list.put(JSONObject().put("id", c.id).put("title", c.title).put("messages", messages)
                .put("runId", c.runId).put("cursor", c.cursor).put("requestId", c.requestId)
                .put("pendingText", c.pendingText).apply { if (c.pendingImages.isNotEmpty()) put("pendingImages", JSONArray(c.pendingImages)) }
                .apply { if (c.pendingModel.isNotEmpty()) put("pendingModel", c.pendingModel) }
                .put("status", c.status).put("cancelRequested", c.cancelRequested)
                .put("pinned", c.pinned).put("version", c.version).put("openedAt", c.openedAt).put("updatedAt", c.updatedAt))
        }
        val out = file.startWrite()
        try {
            out.write(JSONObject().put("cache", CACHE_FORMAT).put("selected", state.selected).put("conversations", list).toString().toByteArray())
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
