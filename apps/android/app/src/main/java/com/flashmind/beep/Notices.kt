package com.flashmind.beep

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import com.flashmind.beep.ffi.NoticeListener
import com.flashmind.beep.proto.Notice
import java.util.concurrent.atomic.AtomicInteger

/** Shows what the server sends to the phone, whichever client or process state it arrives in. */
object Notices : NoticeListener {
    private val nextId = AtomicInteger(1000)

    /** True acks the notice; false leaves it queued on the server for the next connection. */
    override fun onNotice(notice: Notice): Boolean {
        val app = BeepApplication.instance ?: return false
        show(app, notice)
        return true
    }

    private fun show(app: BeepApplication, notice: Notice) {
        // A chat changed on the server, maybe from another phone.
        if (notice.changed) { app.repository.refreshHistory(); return }
        val title = notice.title.ifEmpty { return }
        val body = notice.body
        val conversationId = notice.sessionId
        ChatNotifications.ensureChannel(app)

        val intent = Intent(app, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            if (conversationId.isNotEmpty()) putExtra("conversation_id", conversationId)
            if (notice.app != null) putExtra("open_apps", true)
        }
        val requestCode = (notice.app ?: conversationId).hashCode()
        val pending = PendingIntent.getActivity(app, requestCode, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        notice.approval?.let { approval ->
            // On screen, the card in the chat asks instead.
            if (!app.repository.onScreen(conversationId)) ApprovalNotifications.show(app, approval, conversationId, title, body, pending)
            return
        }

        val builder = Notification.Builder(app, ChatNotifications.CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(title)
            .setContentText(body)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setColor(0xFF3B9B7A.toInt())
            .setStyle(Notification.BigTextStyle().bigText(body))
        // A routine's result can be answered in place, like a chat reply.
        if (conversationId.isNotEmpty() && notice.app == null)
            builder.addAction(ChatNotifications.replyAction(app, conversationId))

        app.getSystemService(NotificationManager::class.java).notify(nextId.getAndIncrement(), builder.build())
    }
}
