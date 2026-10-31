package com.flashmind.beep

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.view.KeyEvent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Keeps live voice going with the screen off or another app in front: a foreground service of
 * the microphone and media playback types, with an ongoing notification that says what Beep is
 * doing and has Stop. While it runs, a media session takes a headset's button as a tap on the orb.
 */
class LiveService : Service() {
    companion object {
        private const val CHANNEL = "beep_live"
        private const val ID = 98
        private const val ACTION_STOP = "com.flashmind.beep.LIVE_STOP"

        fun start(context: Context) {
            try {
                context.startForegroundService(Intent(context, LiveService::class.java))
            } catch (error: IllegalStateException) {
                // Live mode was opened without the app in front. It still runs while the screen is on.
                Log.w("LiveService", "not allowed from the background")
            } catch (error: SecurityException) {
                Log.w("LiveService", "could not start", error)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, LiveService::class.java))
        }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var media: MediaSession? = null
    private val live get() = (application as BeepApplication).live

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Live voice", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shown while you talk with Beep live"
                setShowBadge(false)
            })
        media = MediaSession(this, "BeepLive").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onMediaButtonEvent(intent: Intent): Boolean {
                    @Suppress("DEPRECATION")
                    val key = intent.getParcelableExtra<KeyEvent>(Intent.EXTRA_KEY_EVENT) ?: return false
                    if (key.action != KeyEvent.ACTION_UP) return true
                    return when (key.keyCode) {
                        KeyEvent.KEYCODE_HEADSETHOOK, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE, KeyEvent.KEYCODE_MEDIA_PLAY,
                        KeyEvent.KEYCODE_MEDIA_PAUSE -> { live.tap(); true }
                        KeyEvent.KEYCODE_MEDIA_STOP -> { live.close(LeaveCause.Notification); true }
                        else -> false
                    }
                }
                override fun onPlay() = live.tap()
                override fun onPause() = live.tap()
                override fun onStop() = live.close(LeaveCause.Notification)
            })
            // Playing, so the headset's button comes here rather than to the last music app.
            setPlaybackState(PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_STOP)
                .setState(PlaybackState.STATE_PLAYING, 0, 1f).build())
            isActive = true
        }
        scope.launch {
            live.state.map { it.stage to (it.step ?: it.line ?: it.words) }.distinctUntilChanged().collect { (stage, line) ->
                getSystemService(NotificationManager::class.java).notify(ID, notification(stage, line))
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            live.close(LeaveCause.Notification)
            return START_NOT_STICKY
        }
        val state = live.state.value
        try {
            startForeground(ID, notification(state.stage, state.words),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        } catch (error: Exception) {
            Log.w("LiveService", "could not go to the foreground", error)
            stopSelf()
        }
        // Not restarted after a kill: the session died with the process.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        media?.release()
        media = null
        super.onDestroy()
    }

    private fun notification(stage: LiveStage, line: String): Notification {
        val open = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java).setAction(LIVE_ACTION).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(this, 1, Intent(this, LiveService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE)
        val title = when (stage) {
            LiveStage.Connecting -> "Beep is starting live mode"
            LiveStage.Listening, LiveStage.Holding -> "Beep is listening"
            LiveStage.Thinking -> "Beep is thinking"
            LiveStage.Speaking -> "Beep is speaking"
            LiveStage.Waiting -> "Beep is waiting for you"
        }
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .apply { if (line.isNotBlank()) setContentText(line.takeLast(120)) }
            .setContentIntent(open)
            .addAction(Notification.Action.Builder(null as Icon?, "Stop", stop).build())
            .setCategory(Notification.CATEGORY_SERVICE)
            .setColor(0xFF3B9B7A.toInt())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE) }
            .build()
    }
}

/** The intent action that opens Beep in live voice: the shortcut, the tile, the widget and the notification use it. */
const val LIVE_ACTION = "com.flashmind.beep.LIVE"
