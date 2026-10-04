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
import android.graphics.drawable.Icon

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

    fun replyText(intent: Intent): String? =
        RemoteInput.getResultsFromIntent(intent)?.getCharSequence(REPLY_KEY)?.toString()?.trim()?.takeIf { it.isNotEmpty() }
}

/** Sends what the user typed into a notification's reply field to that chat. */
class ReplyReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra("conversation_id") ?: return
        val text = ChatNotifications.replyText(intent) ?: return
        val repository = (context.applicationContext as BeepApplication).repository
        // Keeps the process alive while the run streams; allowed since the user acted on a notification.
        NotificationService.start(context)
        repository.send(text, conversationId = id)
        // Reposting stops the reply spinner and shows the message that was sent.
        repository.state.value.conversations.find { it.id == id }?.let { ChatNotifications.show(context, it) }
    }
}
