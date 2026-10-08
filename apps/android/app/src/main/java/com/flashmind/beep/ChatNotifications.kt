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
import android.graphics.Typeface
import android.graphics.drawable.Icon
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.StyleSpan
import com.flashmind.beep.proto.PendingApproval

/** A notification per chat with Beep's reply, answerable from the shade. */
object ChatNotifications {
    const val CHANNEL = "beep_messages"
    private const val REPLY_KEY = "reply"
    private const val SHOWN_MESSAGES = 4

    fun ensureChannel(context: Context) {
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Incoming notifications from your assistant"
                enableLights(true)
                lightColor = 0xFF3B9B7A.toInt()
            })
    }

    fun beepPerson(context: Context): Person {
        Avatars.load(context)
        return Person.Builder().setName("Beep").setBot(true).setImportant(true)
            .setIcon(Icon.createWithResource(context, Avatars.current.bubbleIcon)).build()
    }

    fun open(context: Context, conversationId: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            if (conversationId.isNotEmpty()) putExtra("conversation_id", conversationId)
        }
        return PendingIntent.getActivity(context, conversationId.hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    /** The inline reply field. Mutable because Android writes the typed text into it. */
    fun replyAction(context: Context, conversationId: String): Notification.Action {
        val intent = Intent(context, ReplyReceiver::class.java).putExtra("conversation_id", conversationId)
        val pending = PendingIntent.getBroadcast(context, conversationId.hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)
        return Notification.Action.Builder(Icon.createWithResource(context, android.R.drawable.ic_menu_send), "Reply", pending)
            .addRemoteInput(RemoteInput.Builder(REPLY_KEY).setLabel("Reply to Beep").build())
            .setSemanticAction(Notification.Action.SEMANTIC_ACTION_REPLY)
            .setAllowGeneratedReplies(true)
            .build()
    }

    /** Posts or refreshes the chat's notification with its latest turns. */
    fun show(context: Context, chat: Conversation) {
        ensureChannel(context)
        val beep = beepPerson(context)
        val style = Notification.MessagingStyle(Person.Builder().setName("You").setKey("self").build())
            .setConversationTitle(chat.title.takeIf { it != NEW_CHAT_TITLE })
        val now = System.currentTimeMillis()
        val recent = chat.messages.filter { it.role != "scheduled" && (it.text.isNotBlank() || it.role == "assistant") }.takeLast(SHOWN_MESSAGES)
        recent.forEachIndexed { i, message ->
            val time = now - (recent.size - i)
            if (message.role == "user") style.addMessage(message.text, time, null as Person?)
            else style.addMessage(message.text.ifBlank { "Done" }, time, beep)
        }
        if (chat.error.isNotEmpty() && !chat.busy) style.addMessage(chat.error, now, beep)
        if (chat.busy) style.addMessage("Thinking…", now, beep)

        val notification = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setStyle(style)
            .setContentIntent(open(context, chat.id))
            .addAction(replyAction(context, chat.id))
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setColor(0xFF3B9B7A.toInt())
            .setAutoCancel(true)
            // The echo of the user's own reply should not buzz again.
            .setOnlyAlertOnce(chat.busy)
            .build()
        context.getSystemService(NotificationManager::class.java).notify(chat.id.hashCode(), notification)
    }

    fun cancel(context: Context, conversationId: String) =
        context.getSystemService(NotificationManager::class.java).cancel(conversationId.hashCode())

    /** The id of a group's notification: one per group, replaced by each new message. */
    fun groupId(groupId: String) = "group:$groupId".hashCode()

    fun cancelGroup(context: Context, groupId: String) =
        context.getSystemService(NotificationManager::class.java).cancel(groupId(groupId))

    fun replyText(intent: Intent): String? =
        RemoteInput.getResultsFromIntent(intent)?.getCharSequence(REPLY_KEY)?.toString()?.trim()?.takeIf { it.isNotEmpty() }
}

/** Sends what the user typed into a notification's reply field to that chat. */
class ReplyReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra("conversation_id") ?: return
        val text = ChatNotifications.replyText(intent) ?: return
        val repository = (context.applicationContext as BeepApplication).repository
        repository.send(text, conversationId = id)
        // Reposting stops the reply spinner and shows the message that was sent.
        repository.state.value.conversations.find { it.id == id }?.let { ChatNotifications.show(context, it) }
    }
}

/** A tool call waiting for the user, with Allow and Deny right in the notification. */
object ApprovalNotifications {
    fun show(context: Context, pending: PendingApproval, conversationId: String, title: String, body: String, open: PendingIntent) {
        ChatNotifications.ensureChannel(context)
        fun action(label: String, decision: String): Notification.Action {
            val intent = Intent(context, ApprovalReceiver::class.java)
                .putExtra("conversation_id", conversationId).putExtra("query_id", pending.queryId).putExtra("decision", decision)
            val request = (pending.queryId + decision).hashCode()
            val broadcast = PendingIntent.getBroadcast(context, request, intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
            return Notification.Action.Builder(null as Icon?, label, broadcast).build()
        }
        val text = arguments(pending).ifEmpty { body }
        val notification = Notification.Builder(context, ChatNotifications.CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text))
            .setContentIntent(open)
            .addAction(action("Deny", "deny"))
            .addAction(action("Allow", "allow"))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setColor(0xFF3B9B7A.toInt())
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
        val conversationId = intent.getStringExtra("conversation_id") ?: return
        repository.answerApproval(conversationId, queryId, decision)
    }
}
