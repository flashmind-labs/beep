package com.flashmind.beep

import android.app.ForegroundServiceStartNotAllowedException
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Keeps the process alive while a reply runs, so the stream is not cut when the app leaves the
 * screen. It stops when the last run ends; the reply then shows as a chat notification and the
 * app goes idle, with Firebase bringing what the server sends meanwhile.
 *
 * A phone without Firebase, such as one without Google Play services, cannot be reached that
 * way, so there the service stays up as "Beep is connected" for the server to dial the phone.
 */
class RunService : Service() {
    companion object {
        private const val RUN_CHANNEL = "beep_run"
        private const val CONNECTION_CHANNEL = "beep_connection"
        private const val OLD_CHANNEL = "beep_foreground"
        private const val ID = 99
        private const val EXTRA_TITLES = "titles"
        private const val EXTRA_KEEP_ALIVE = "keep_alive"
        private const val PREFS = "run_service"
        private const val PREF_KEEP_ALIVE = "keep_alive"

        private data class Mode(val titles: List<String>, val keepAlive: Boolean)

        /** Starts, changes or stops the service as chats start and finish runs. */
        fun follow(context: Context, repository: ChatRepository, scope: CoroutineScope) {
            // The last answer, so a restart keeps the phone reachable before it connects.
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val titles = repository.state.map { state -> state.conversations.filter { it.busy }.map { it.title } }
            scope.launch {
                combine(titles, repository.pushMissing) { titles, missing ->
                    if (missing != null) prefs.edit().putBoolean(PREF_KEEP_ALIVE, missing).apply()
                    Mode(titles, missing ?: prefs.getBoolean(PREF_KEEP_ALIVE, false))
                }
                    .distinctUntilChanged()
                    .collect { mode -> if (mode.titles.isEmpty() && !mode.keepAlive) stop(context) else start(context, mode) }
            }
        }

        private fun start(context: Context, mode: Mode) {
            val intent = Intent(context, RunService::class.java)
                .putStringArrayListExtra(EXTRA_TITLES, ArrayList(mode.titles))
                .putExtra(EXTRA_KEEP_ALIVE, mode.keepAlive)
            try {
                context.startForegroundService(intent)
            } catch (error: ForegroundServiceStartNotAllowedException) {
                // A run resumed in the background, with no user action to allow it. It still
                // streams while the process lives.
                Log.w("RunService", "not allowed from the background")
            }
        }

        private fun stop(context: Context) {
            context.stopService(Intent(context, RunService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.deleteNotificationChannel(OLD_CHANNEL)
        // Low: in the shade without sound, since the reply itself notifies.
        manager.createNotificationChannel(NotificationChannel(RUN_CHANNEL, "Replies in progress",
            NotificationManager.IMPORTANCE_LOW).apply {
            description = "Shown while Beep is working on a reply"
            setShowBadge(false)
        })
        // Lowest: collapsed in the silent section with no status bar icon.
        manager.createNotificationChannel(NotificationChannel(CONNECTION_CHANNEL, "Background connection",
            NotificationManager.IMPORTANCE_MIN).apply {
            description = "Keeps Beep reachable on phones without Google Play services"
            setShowBadge(false)
        })
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // No intent: Android restarted the service after killing the process, which it only
        // does for the sticky keep-alive. Loading the repository connects again.
        if (intent == null) (application as BeepApplication).repository
        val titles = intent?.getStringArrayListExtra(EXTRA_TITLES).orEmpty()
        val keepAlive = intent?.getBooleanExtra(EXTRA_KEEP_ALIVE, false) ?: true
        startForeground(ID, notification(titles))
        // After a kill during a run, the repository resumes busy chats and starts this again.
        return if (keepAlive) START_STICKY else START_NOT_STICKY
    }

    private fun notification(titles: List<String>): Notification {
        val open = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java).setAction("com.flashmind.beep.RUNNING"), PendingIntent.FLAG_IMMUTABLE)
        val builder = if (titles.isEmpty()) {
            Notification.Builder(this, CONNECTION_CHANNEL).setContentTitle("Beep is connected")
        } else {
            val text = when (titles.size) {
                1 -> titles[0].takeIf { it != NEW_CHAT_TITLE }
                else -> "${titles.size} chats"
            }
            Notification.Builder(this, RUN_CHANNEL).setContentTitle("Beep is replying").setContentText(text)
        }
        return builder
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentIntent(open)
            .setColor(LINEAR_ACCENT.toInt())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()
    }
}
