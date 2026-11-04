package com.flashmind.beep

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Person
import android.app.RemoteInput
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.drawable.Icon
import android.net.Uri
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.StyleSpan
import com.flashmind.beep.proto.PendingApproval
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/** A chat a notification belongs to: a chat with Beep (routines included) or a group. */
data class ChatKey(val id: String, val group: Boolean) {
    /**
     * Names the notification, its shortcut and its bubble. Every chat with Beep shares one,
     * so only one bubble ever floats; it shows whichever chat spoke last.
     */
    val tag get() = if (group) "group:$id" else "beep"
    val notificationId get() = tag.hashCode()
    fun extras(intent: Intent): Intent = intent.putExtra(if (group) "group_id" else "conversation_id", id)

    companion object {
        fun from(intent: Intent): ChatKey? =
            intent.getStringExtra("group_id")?.let { ChatKey(it, true) }
                ?: intent.getStringExtra("conversation_id")?.let { ChatKey(it, false) }
    }
}

/** One message in a notification. [sender] is null for the user, "Beep" for Beep, else a member's name. */
data class ChatLine(val sender: String?, val text: String, val time: Long = System.currentTimeMillis())

/**
 * Every chat as a conversation notification: who said what, a shortcut of its own so it
 * can float as a bubble, and Reply and Mark as read in the shade.
 */
object ChatNotifications {
    const val CHANNEL = "beep_messages"
    private const val REPLY_KEY = "reply"
    private const val SHOWN_MESSAGES = 6
    const val BEEP = "Beep"

    /** What each chat's notification shows, so a new notice adds to it rather than replacing it. */
    private val shown = ConcurrentHashMap<String, List<ChatLine>>()
    /** The chat with Beep that the shared notification is showing. */
    @Volatile private var beepChat: String? = null
    /** A chat whose reply the bubble announced, for the bubble to open on next time it expands. */
    @Volatile var announced: String? = null

    fun ensureChannel(context: Context) {
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Incoming notifications from your assistant"
                enableLights(true)
                lightColor = LINEAR_ACCENT.toInt()
                setAllowBubbles(true)
            })
    }

    fun beepPerson(context: Context): Person {
        Avatars.load(context)
        return Person.Builder().setName(BEEP).setKey(BEEP).setBot(true).setImportant(true)
            .setIcon(Icon.createWithResource(context, Avatars.current.bubbleIcon)).build()
    }

    private fun you() = Person.Builder().setName("You").setKey("self").build()

    private fun member(name: String) = Person.Builder().setName(name).setKey("member:$name")
        .setIcon(Icon.createWithAdaptiveBitmap(initials(name))).build()

    private val tints = intArrayOf(0xFFCDE8DA.toInt(), 0xFFF9D9D2.toInt(), 0xFFE6DDF7.toInt(), 0xFFF6E7C1.toInt())

    /** A round avatar with the name's first letter, sized for an adaptive icon. */
    private fun initials(name: String): Bitmap {
        val size = 108
        val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.drawColor(tints[name.hashCode().mod(tints.size)])
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF10201A.toInt(); textSize = 40f; textAlign = Paint.Align.CENTER; typeface = Typeface.DEFAULT_BOLD
        }
        val letter = initial(name).ifEmpty { "?" }
        canvas.drawText(letter, size / 2f, size / 2f - (paint.descent() + paint.ascent()) / 2, paint)
        return bitmap
    }

    private fun chatIntent(context: Context, key: ChatKey) = key.extras(Intent(context, MainActivity::class.java)
        .setAction(Intent.ACTION_VIEW).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))

    fun open(context: Context, key: ChatKey): PendingIntent = PendingIntent.getActivity(context, key.notificationId,
        chatIntent(context, key), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

    /** The inline reply field. Mutable because Android writes the typed text into it. */
    private fun replyAction(context: Context, key: ChatKey): Notification.Action {
        val intent = key.extras(Intent(context, ReplyReceiver::class.java))
        val pending = PendingIntent.getBroadcast(context, key.notificationId, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)
        return Notification.Action.Builder(Icon.createWithResource(context, android.R.drawable.ic_menu_send), "Reply", pending)
            .addRemoteInput(RemoteInput.Builder(REPLY_KEY).setLabel(if (key.group) "Message" else "Reply to Beep").build())
            .setSemanticAction(Notification.Action.SEMANTIC_ACTION_REPLY)
            .setAllowGeneratedReplies(true)
            .build()
    }

    private fun markReadAction(context: Context, key: ChatKey): Notification.Action {
        val intent = key.extras(Intent(context, MarkReadReceiver::class.java))
        val pending = PendingIntent.getBroadcast(context, key.notificationId, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        return Notification.Action.Builder(null as Icon?, "Mark as read", pending)
            .setSemanticAction(Notification.Action.SEMANTIC_ACTION_MARK_AS_READ)
            .build()
    }

    /** The chat's long-lived shortcut, which conversation notifications and bubbles hang off. */
    private fun pushShortcut(context: Context, key: ChatKey, title: String, icon: Icon, people: List<Person>) {
        // Android does not count a shortcut whose people are all bots as a conversation,
        // and then drops the bubble, so Beep stays off it. A chat with only Beep has none.
        val humans = people.filterNot { it.isBot }
        val shortcut = ShortcutInfo.Builder(context, key.tag)
            .setShortLabel(title.take(24).ifBlank { BEEP })
            .setLongLabel(title.ifBlank { BEEP })
            .setIcon(icon)
            .apply { if (humans.isNotEmpty()) setPersons(humans.toTypedArray()) }
            .setLongLived(true)
            .setCategories(setOf("com.flashmind.beep.category.CHAT"))
            .setIntent(chatIntent(context, key))
            .build()
        runCatching { context.getSystemService(ShortcutManager::class.java).pushDynamicShortcut(shortcut) }
    }

    /** Opens the chat in a bubble. Unique data keeps each chat's bubble apart. */
    private fun bubble(context: Context, key: ChatKey, icon: Icon, expand: Boolean): Notification.BubbleMetadata {
        val intent = key.extras(Intent(context, BubbleActivity::class.java).setAction(Intent.ACTION_VIEW)
            .setData(Uri.parse("beep://chat/${Uri.encode(key.tag)}")))
        val pending = PendingIntent.getActivity(context, key.notificationId, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)
        return Notification.BubbleMetadata.Builder(pending, icon).setDesiredHeight(440)
            .setAutoExpandBubble(expand).setSuppressNotification(expand).build()
    }

    /** How a post should behave beyond showing the messages. */
    data class Post(val quiet: Boolean = false, val expand: Boolean = false)

    /**
     * Posts the chat's conversation notification with [lines], the latest last. A group's
     * icon is its first letter; a chat with Beep shows Beep.
     */
    fun post(context: Context, key: ChatKey, title: String, lines: List<ChatLine>, options: Post = Post()) {
        ensureChannel(context)
        val beep = beepPerson(context)
        val members = lines.mapNotNull { it.sender }.filter { it != BEEP }.distinct().associateWith(::member)
        val style = Notification.MessagingStyle(you())
            .setConversationTitle(title.takeIf { key.group || it.isNotBlank() && it != NEW_CHAT_TITLE })
            .setGroupConversation(key.group)
        // A chat with Beep shows only its latest line, so a long reply is not buried under
        // the turns before it. A group keeps who said what.
        val kept = lines.takeLast(if (key.group) SHOWN_MESSAGES else 1)
        kept.forEach { line ->
            val sender = when (line.sender) { null -> null; BEEP -> beep; else -> members.getValue(line.sender) }
            style.addMessage(line.text, line.time, sender)
        }
        shown[key.tag] = kept
        if (!key.group) {
            beepChat = key.id
            if (!options.quiet) announced = key.id
        }
        val icon = if (key.group) Icon.createWithAdaptiveBitmap(initials(title)) else Icon.createWithResource(context, Avatars.current.bubbleIcon)
        pushShortcut(context, key, if (key.group) title else BEEP, icon, listOf(beep) + members.values)
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setStyle(style)
            .setShortcutId(key.tag)
            .setContentIntent(open(context, key))
            .addAction(replyAction(context, key))
            .addAction(markReadAction(context, key))
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setColor(LINEAR_ACCENT.toInt())
            .setAutoCancel(true)
            .setOnlyAlertOnce(options.quiet)
            // Only Beep floats; a group stays in the shade so there is never a second bubble.
            .apply { if (!key.group) setBubbleMetadata(bubble(context, key, icon, options.expand)) }
        context.getSystemService(NotificationManager::class.java).notify(key.notificationId, builder.build())
    }

    /** Adds [line] under what the chat's notification already shows. */
    fun append(context: Context, key: ChatKey, title: String, line: ChatLine, options: Post = Post()) =
        post(context, key, title, (if (isShown(context, key)) shown[key.tag].orEmpty() else emptyList()) + line, options)

    private fun isShown(context: Context, key: ChatKey) = (key.group || beepChat == key.id) &&
        context.getSystemService(NotificationManager::class.java).activeNotifications.any { it.id == key.notificationId }

    /** The lines of a chat with Beep, from its saved turns. */
    private fun lines(chat: Conversation): List<ChatLine> {
        val now = System.currentTimeMillis()
        val recent = chat.messages.filter { it.role != "scheduled" && (it.text.isNotBlank() || it.role == "assistant") }.takeLast(SHOWN_MESSAGES)
        val lines = recent.mapIndexed { i, message ->
            ChatLine(if (message.role == "user") null else BEEP, message.text.ifBlank { "Done" }, now - (recent.size - i))
        }.toMutableList()
        if (chat.error.isNotEmpty() && !chat.busy) lines += ChatLine(BEEP, chat.error, now)
        if (chat.busy) lines += ChatLine(BEEP, "Thinking\u2026", now)
        return lines
    }

    /** Posts or refreshes the chat's notification with its latest turns. */
    fun show(context: Context, chat: Conversation, expand: Boolean = false) =
        // The echo of the user's own reply should not buzz again.
        post(context, ChatKey(chat.id, false), chat.title, lines(chat), Post(quiet = chat.busy || expand, expand = expand))

    /** Takes the chat's notification down. A chat floating as a bubble stays unless [bubble] says to drop it too. */
    fun cancel(context: Context, key: ChatKey, bubble: Boolean = false) {
        if (announced == key.id) announced = null
        val manager = context.getSystemService(NotificationManager::class.java)
        val floating = manager.activeNotifications.any { it.id == key.notificationId && it.notification.flags and Notification.FLAG_BUBBLE != 0 }
        if (floating && !bubble) return
        // The shared notification may be showing another chat with Beep.
        if (!key.group && beepChat != null && beepChat != key.id) return
        shown.remove(key.tag)
        manager.cancel(key.notificationId)
    }

    fun cancel(context: Context, conversationId: String) = cancel(context, ChatKey(conversationId, false))

    fun cancelGroup(context: Context, groupId: String) = cancel(context, ChatKey(groupId, true))

    /** A group's notice body is "Name: text"; the name is who said it. */
    fun groupLine(body: String): ChatLine {
        val name = body.substringBefore(": ", "")
        return if (name.isNotBlank() && name.length <= 40 && '\n' !in name) ChatLine(name, body.substringAfter(": "))
        else ChatLine(BEEP, body)
    }

    fun replyText(intent: Intent): String? =
        RemoteInput.getResultsFromIntent(intent)?.getCharSequence(REPLY_KEY)?.toString()?.trim()?.takeIf { it.isNotEmpty() }
}

/** Sends what the user typed into a notification's reply field to that chat or group. */
class ReplyReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val key = ChatKey.from(intent) ?: return
        val text = ChatNotifications.replyText(intent) ?: return
        val repository = (context.applicationContext as BeepApplication).repository
        if (!key.group) {
            // While Beep is still replying, hold it like the composer does instead of dropping it.
            if (repository.state.value.conversations.find { it.id == key.id }?.busy == true) repository.queue(text, emptyList(), key.id)
            else repository.send(text, conversationId = key.id)
            // Reposting stops the reply spinner and shows the message that was sent.
            repository.state.value.conversations.find { it.id == key.id }?.let { ChatNotifications.show(context, it) }
            return
        }
        val title = repository.groups.value.find { it.id == key.id }?.name ?: "Group"
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                withTimeoutOrNull(25_000L) {
                    val sent = runCatching { repository.sendToGroup(key.id, UUID.randomUUID().toString(), text) }.isSuccess
                    val line = if (sent) ChatLine(null, text) else ChatLine(null, "Not sent: $text")
                    ChatNotifications.append(context, key, title, line, ChatNotifications.Post(quiet = true))
                }
            } finally {
                pending.finish()
            }
        }
    }
}

/** Takes a chat's notification down without opening it. */
class MarkReadReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        ChatKey.from(intent)?.let { ChatNotifications.cancel(context, it, bubble = true) }
    }
}

/** A tool call waiting for the user, with Allow and Deny right in the notification. */
object ApprovalNotifications {
    fun show(context: Context, pending: PendingApproval, conversationId: String, title: String, body: String, open: PendingIntent, group: Boolean = false) {
        ChatNotifications.ensureChannel(context)
        fun action(label: String, decision: String): Notification.Action {
            val intent = Intent(context, ApprovalReceiver::class.java)
                .putExtra("conversation_id", conversationId).putExtra("query_id", pending.queryId).putExtra("decision", decision).putExtra("group", group)
            val request = (pending.queryId + decision).hashCode()
            val broadcast = PendingIntent.getBroadcast(context, request, intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
            return Notification.Action.Builder(null as Icon?, label, broadcast).build()
        }
        // A group asks on behalf of someone, so say who before the arguments.
        val text = if (group) SpannableStringBuilder(body).append('\n').append(arguments(pending)) else arguments(pending).ifEmpty { body }
        val notification = Notification.Builder(context, ChatNotifications.CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text))
            .setContentIntent(open)
            .addAction(action("Deny", "deny"))
            .addAction(action("Allow", "allow"))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setColor(LINEAR_ACCENT.toInt())
            .setAutoCancel(true)
            .build()
        context.getSystemService(NotificationManager::class.java).notify(pending.queryId.hashCode(), notification)
    }

    /** Each argument on its own line, the label in bold. */
    private fun arguments(pending: PendingApproval): CharSequence {
        val text = SpannableStringBuilder()
        pending.approval.fields.forEach { field ->
            if (text.isNotEmpty()) text.append('\n')
            val start = text.length
            text.append(field.label).append(':')
            text.setSpan(StyleSpan(Typeface.BOLD), start, text.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            text.append(' ').append(field.value)
        }
        return text
    }

    fun cancel(context: Context, queryId: String) =
        context.getSystemService(NotificationManager::class.java).cancel(queryId.hashCode())
}

/** Sends the user's Allow or Deny from an approval notification. */
class ApprovalReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val queryId = intent.getStringExtra("query_id") ?: return
        val decision = intent.getStringExtra("decision") ?: return
        val repository = (context.applicationContext as BeepApplication).repository
        if (intent.getBooleanExtra("group", false)) return repository.answerGroupApproval(queryId, decision == "allow")
        val conversationId = intent.getStringExtra("conversation_id") ?: return
        repository.answerApproval(conversationId, queryId, decision)
    }
}
