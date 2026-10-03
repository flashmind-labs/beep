package com.flashmind.beep

import android.app.Notification
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import kotlinx.coroutines.*
import com.flashmind.beep.proto.PhoneNotification
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Captures notifications from all apps and buffers them for periodic sync
 * to the server. The user must grant Notification Access in system settings.
 */
class BeepNotificationListener : NotificationListenerService() {

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        // Skip own notifications
        if (sbn.packageName == packageName) return
        // Skip ongoing (e.g. music players, navigation) and group summaries
        if (sbn.isOngoing) return
        if (sbn.notification.flags and Notification.FLAG_GROUP_SUMMARY != 0) return

        val extras = sbn.notification.extras
        NotificationBuffer.add(PhoneNotification(
            app = sbn.packageName,
            title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: "",
            text = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString() ?: "",
            timestamp = sbn.postTime,
        ))
    }
}

/**
 * In-memory buffer that accumulates captured notifications and flushes them
 * to the server every 30 seconds or when 20 items have been buffered.
 */
object NotificationBuffer {
    private val buffer = CopyOnWriteArrayList<PhoneNotification>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var flushJob: Job? = null

    fun add(notification: PhoneNotification) {
        buffer.add(notification)
        if (buffer.size >= 20) {
            flush()
        }
        ensureFlushLoop()
    }

    private fun ensureFlushLoop() {
        if (flushJob?.isActive == true) return
        flushJob = scope.launch {
            while (isActive) {
                delay(30_000)
                flush()
            }
        }
    }

    private fun flush() {
        if (buffer.isEmpty()) return
        val batch = ArrayList(buffer)
        buffer.removeAll(batch.toSet())
        scope.launch {
            try {
                syncToServer(batch)
            } catch (_: Exception) {
                // Put them back for the next attempt
                buffer.addAll(batch)
            }
        }
    }

    private suspend fun syncToServer(notifications: List<PhoneNotification>) {
        val app = BeepApplication.instance ?: return
        val config = app.repository.settings.value
        if (!config.configured) return
        BeepClients.client(config).saveNotifications(notifications)
    }
}
