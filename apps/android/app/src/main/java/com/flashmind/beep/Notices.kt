package com.flashmind.beep

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import com.flashmind.beep.ffi.NoticeListener
import com.flashmind.beep.ffi.resolvedQuestion
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
        if (notice.group) { showGroup(app, notice); return }
        // A background question stopped waiting; such a notice also says the chat changed.
        resolvedQuestion(notice)?.let { app.repository.resolveQuestion(notice.sessionId, it); return }
        if (notice.changed) { app.repository.refreshHistory(); return }
        val title = notice.title.ifEmpty { return }
        val body = notice.body
        val conversationId = notice.sessionId
        if (notice.app == null) app.repository.prefetch(conversationId)
        ChatNotifications.ensureChannel(app)

        // Its own action keeps this apart from the other PendingIntents that open the app, which
        // Android would otherwise treat as the same one and hand this notice's extras.
        val intent = Intent(app, MainActivity::class.java).setAction("com.flashmind.beep.NOTICE").apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            if (conversationId.isNotEmpty()) putExtra("conversation_id", conversationId)
            if (notice.app != null) putExtra("open_apps", true)
            // No chat, app or approval: a read of mail or a calendar finished.
            if (conversationId.isEmpty() && notice.app == null && notice.approval == null) putExtra("open_read", true)
        }
        val requestCode = (notice.app ?: conversationId).hashCode()
        val pending = PendingIntent.getActivity(app, requestCode, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        notice.approval?.let { approval ->
            // Background work has no run to stream its card, so the notice puts one in the chat.
            if (approval.runId.isEmpty()) app.repository.showBackgroundApproval(conversationId, approval)
            // On screen, the card in the chat asks instead.
            if (!app.repository.onScreen(conversationId)) ApprovalNotifications.show(app, approval, conversationId, title, body, pending)
            return
        }

        // A routine's result comes as Beep in the routine's chat, answerable in place.
        if (conversationId.isNotEmpty() && notice.app == null) {
            ChatNotifications.append(app, ChatKey(conversationId, false), title, ChatLine(ChatNotifications.BEEP, body))
            return
        }
        val builder = Notification.Builder(app, ChatNotifications.CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(title)
            .setContentText(body)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setColor(LINEAR_ACCENT.toInt())
            .setStyle(Notification.BigTextStyle().bigText(body))
        app.getSystemService(NotificationManager::class.java).notify(nextId.getAndIncrement(), builder.build())
    }

    /** A group message, or a change in a group such as Beep starting to type. */
    private fun showGroup(app: BeepApplication, notice: Notice) {
        val groupId = notice.sessionId
        app.repository.groupChanged(groupId)
        val key = ChatKey(groupId, true)
        // Beep asks the owner of an app before acting in it, even with the group open.
        notice.approval?.let { approval ->
            ApprovalNotifications.show(app, approval, groupId, notice.title, notice.body, ChatNotifications.open(app, key), group = true)
            return
        }
        if (notice.changed || notice.title.isEmpty() || app.repository.groupOnScreen(groupId)) return
        ChatNotifications.append(app, key, notice.title, ChatNotifications.groupLine(notice.body))
    }
}
