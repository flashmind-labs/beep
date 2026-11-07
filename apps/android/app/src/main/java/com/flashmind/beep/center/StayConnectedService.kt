package com.flashmind.beep.center

import android.app.ForegroundServiceStartNotAllowedException
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import com.flashmind.beep.LINEAR_ACCENT
import com.flashmind.beep.MainActivity
import com.flashmind.beep.R

/**
 * Holds the asks feed while Beep is closed, so questions arrive as notifications. Android shows
 * a lasting notification for it. It is off until the user turns on Stay connected.
 */
class StayConnectedService : Service() {
    companion object {
        private const val CHANNEL = "beep_stay"
        private const val ID = 98

        fun start(context: Context) {
            try {
                context.startForegroundService(Intent(context, StayConnectedService::class.java))
            } catch (error: ForegroundServiceStartNotAllowedException) {
                Log.w("StayConnected", "not allowed from the background")
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, StayConnectedService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Stay connected", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shown while Beep listens for questions from your agents"
                setShowBadge(false)
            })
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!NotifyPrefs.on(this, NotifyPrefs.STAY)) {
            // Android restarted it after the user turned Stay connected off.
            stopSelf()
            return START_NOT_STICKY
        }
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome).setColor(LINEAR_ACCENT.toInt())
            .setContentTitle("Connected to your machines")
            .setContentIntent(open).setOngoing(true).setOnlyAlertOnce(true).build()
        startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        AskCenter.start(this, AskCenter.Owner.Service)
        return START_STICKY
    }

    /** Android 15 ends a data sync service after six hours. The app starts it again when it opens. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    override fun onDestroy() {
        AskCenter.stop(AskCenter.Owner.Service)
        super.onDestroy()
    }
}
