package com.flashmind.beep

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.os.IBinder
import kotlinx.coroutines.*
import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.ffi.NoticeListener
import com.flashmind.beep.proto.Notice

/**
 * Keeps the process alive with Beep's endpoint bound, so the server can dial the phone with a
 * notice while no connection is open, and tells the endpoint when the network changes.
 */
class NotificationService : Service() {
    companion object {
        // A channel keeps the importance it was created with, so the quieter one needed a new id.
        private const val CHANNEL_FOREGROUND = "beep_connection"
        private const val OLD_CHANNEL_FOREGROUND = "beep_foreground"
        private const val FOREGROUND_ID = 99
        private var nextId = 1000

        fun start(context: Context) {
            context.startForegroundService(Intent(context, NotificationService::class.java))
        }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var listenJob: Job? = null
    private var networkJob: Job? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    /** The network the phone last caught up on. */
    @Volatile private var caughtUpOn: Network? = null
    private val listener = object : NoticeListener {
        override fun onNotice(notice: Notice): Boolean {
            handleNotice(notice)
            return true
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.deleteNotificationChannel(OLD_CHANNEL_FOREGROUND)
        // Lowest importance: it sits collapsed in the silent section with no status bar icon.
        manager.createNotificationChannel(NotificationChannel(CHANNEL_FOREGROUND, "Background connection",
            NotificationManager.IMPORTANCE_MIN).apply {
            description = "Lets routines and reminders reach the phone"
            setShowBadge(false)
        })
        ChatNotifications.ensureChannel(this)
        startForeground(FOREGROUND_ID, buildForegroundNotification())
        watchNetwork()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Started again after the settings change, which may mean a new client.
        listen()
        return START_STICKY
    }

    override fun onDestroy() {
        networkCallback?.let { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
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

    /** Binds the endpoint and hands it the listener. No connection stays open. */
    private fun listen() {
        listenJob?.cancel()
        listenJob = scope.launch {
            val storage = LocalStorage(this@NotificationService)
            val config = storage.loadSettings()
            if (!config.configured) return@launch
            var attempt = 0
            while (isActive) {
                try {
                    val client = BeepClients.client(config, storage.identity())
                    client.listen(listener)
                    catchUp(client)
                    return@launch
                } catch (error: Exception) {
                    if (error is CancellationException) throw error
                    attempt++
                }
                delay(minOf(30_000L * attempt, 300_000L))
            }
        }
    }

    /**
     * Android gives iroh no way to watch the network itself. Capability updates arrive often
     * (signal strength among them), so only a change of transport or validation counts, and a
     * burst of callbacks becomes one call.
     */
    private fun watchNetwork() {
        val callback = object : ConnectivityManager.NetworkCallback() {
            private var last: NetworkCapabilities? = null
            override fun onAvailable(network: Network) = changed()
            override fun onLost(network: Network) = changed()
            override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) = changed()
            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                val before = last
                last = capabilities
                if (before == null || differs(before, capabilities)) changed()
            }
        }
        getSystemService(ConnectivityManager::class.java).registerDefaultNetworkCallback(callback)
        networkCallback = callback
    }

    private fun differs(a: NetworkCapabilities, b: NetworkCapabilities): Boolean =
        listOf(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.TRANSPORT_CELLULAR,
            NetworkCapabilities.TRANSPORT_ETHERNET, NetworkCapabilities.TRANSPORT_VPN)
            .any { a.hasTransport(it) != b.hasTransport(it) } ||
            a.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) !=
            b.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)

    /** Callbacks all run on one thread, so the job needs no lock. */
    private fun changed() {
        networkJob?.cancel()
        networkJob = scope.launch {
            delay(400)
            val client = BeepClients.current() ?: return@launch
            client.networkChanged()
            val cm = getSystemService(ConnectivityManager::class.java)
            val network = cm.activeNetwork ?: return@launch
            val online = cm.getNetworkCapabilities(network)
                ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) == true
            if (online && network != caughtUpOn) {
                caughtUpOn = network
                catchUp(client)
            }
        }
    }

    /**
     * Connects once, so the server sends what it queued while the phone was offline. The
     * connection closes itself when idle.
     */
    private suspend fun catchUp(client: BeepClient) {
        try {
            client.health()
        } catch (error: Exception) {
            if (error is CancellationException) throw error
        }
    }

    private fun handleNotice(notice: Notice) {
        // A chat changed on the server, maybe from another phone.
        if (notice.changed) { (applicationContext as BeepApplication).repository.refreshHistory(); return }
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

        notice.approval?.let { approval ->
            // On screen, the card in the chat asks instead.
            val repository = (applicationContext as BeepApplication).repository
            if (!repository.onScreen(conversationId)) ApprovalNotifications.show(this, approval, conversationId, title, body, pending)
            return
        }

        val builder = Notification.Builder(this, ChatNotifications.CHANNEL)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(title)
            .setContentText(body)
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setColor(0xFF3B9B7A.toInt())
            .setStyle(Notification.BigTextStyle().bigText(body))
        // A routine's result can be answered in place, like a chat reply.
        if (conversationId.isNotEmpty() && notice.app == null)
            builder.addAction(ChatNotifications.replyAction(this, conversationId))

        getSystemService(NotificationManager::class.java).notify(nextId++, builder.build())
    }
}
