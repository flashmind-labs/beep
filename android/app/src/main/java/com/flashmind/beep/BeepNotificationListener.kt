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
 * Buffers captured notifications and sends them in batches, since each send wakes the radio:
 * at 20 items, 10 minutes after the first one, or at once while a connection is open anyway.
 */
object NotificationBuffer {
    private const val BATCH = 20
    private const val MAX_WAIT = 10 * 60 * 1000L
    private val buffer = CopyOnWriteArrayList<PhoneNotification>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var flushJob: Job? = null

    fun add(notification: PhoneNotification) {
        buffer.add(notification)
        if (buffer.size >= BATCH) {
            flush()
            return
        }
        scope.launch { if (BeepClients.current()?.isConnected() == true) flush() }
        scheduleFlush()
    }

    @Synchronized
    private fun scheduleFlush() {
        if (flushJob?.isActive == true) return
        flushJob = scope.launch {
            delay(MAX_WAIT)
            flush()
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
                scheduleFlush()
            }
        }
    }

    private suspend fun syncToServer(notifications: List<PhoneNotification>) {
        val app = BeepApplication.instance ?: return
        val config = app.repository.settings.value
        if (!config.configured) return
        BeepClients.client(config, LocalStorage(app).identity()).saveNotifications(notifications)
    }
}
