package com.flashmind.beep

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.IBinder
import kotlinx.coroutines.*
import com.flashmind.beep.proto.Notice

class NotificationService : Service() {
    companion object {
        // A channel keeps the importance it was created with, so the quieter one needed a new id.
        private const val CHANNEL_FOREGROUND = "beep_connection"
        private const val OLD_CHANNEL_FOREGROUND = "beep_foreground"
        private const val CHANNEL_MESSAGES = "beep_messages"
        private const val FOREGROUND_ID = 99
        private var nextId = 1000

        fun start(context: Context) {
            context.startForegroundService(Intent(context, NotificationService::class.java))
        }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var streamJob: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.deleteNotificationChannel(OLD_CHANNEL_FOREGROUND)
        // Lowest importance: it sits collapsed in the silent section with no status bar icon.
        manager.createNotificationChannel(NotificationChannel(CHANNEL_FOREGROUND, "Background connection",
            NotificationManager.IMPORTANCE_MIN).apply {
            description = "Keeps Beep connected so routines and reminders arrive"
            setShowBadge(false)
        })
        manager.createNotificationChannel(NotificationChannel(CHANNEL_MESSAGES, "Messages",
            NotificationManager.IMPORTANCE_HIGH).apply {
            description = "Incoming notifications from your assistant"
            enableLights(true)
            lightColor = 0xFF3B9B7A.toInt()
        })
        startForeground(FOREGROUND_ID, buildForegroundNotification())
        startStreaming()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Restart streaming if a new start is requested (e.g. after settings change).
        streamJob?.cancel()
        startStreaming()
        return START_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun buildForegroundNotification(): Notification {
        val open = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL_FOREGROUND)
            .setSmallIcon(android.R.drawable.stat_notify_sync_noanim)
            .setContentTitle("Beep is connected")
            .setContentIntent(open)
            .setColor(0xFF3B9B7A.toInt())
            .setOngoing(true)
            .build()
    }

    private fun startStreaming() {
        streamJob = scope.launch {
            val storage = LocalStorage(this@NotificationService)
            var attempt = 0
            while (isActive) {
                val config = storage.loadSettings()
                if (!config.configured) { delay(5000); continue }
                try {
                    BeepClients.client(config).notifications().use { notices ->
                        attempt = 0
                        while (true) handleNotice(notices.next() ?: break)
                    }
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    attempt++
                }
                delay(minOf(1000L * (attempt + 1), 15000L))
            }
        }
    }

    private fun handleNotice(notice: Notice) {
        val title = notice.title.ifEmpty { return }
        val body = notice.body
        val conversationId = notice.sessionId

        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            if (conversationId.isNotEmpty()) putExtra("conversation_id", conversationId)
            if (notice.app != null) putExtra("open_apps", true)
        }
        val requestCode = (notice.app ?: conversationId).hashCode()
        val pending = PendingIntent.getActivity(this, requestCode, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

        val notification = Notification.Builder(this, CHANNEL_MESSAGES)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(title)
            .setContentText(body)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setColor(0xFF3B9B7A.toInt())
            .setStyle(Notification.BigTextStyle().bigText(body))
            .build()

        getSystemService(NotificationManager::class.java).notify(nextId++, notification)
    }
}
