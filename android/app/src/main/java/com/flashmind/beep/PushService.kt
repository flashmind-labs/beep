package com.flashmind.beep

import android.util.Log
import com.flashmind.beep.ffi.openNotice
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Firebase brings what the server could not deliver over iroh. A `notice` push carries a notice
 * sealed to this phone's identity, shown without connecting. A `wake` push carries nothing: the
 * phone connects and the server sends its queued notices over that connection.
 */
class PushService : FirebaseMessagingService() {
    companion object {
        /** How long a wake keeps the process up after connecting, for the queued notices to land. */
        private const val SETTLE_MS = 3_000L
        private const val WAKE_TIMEOUT_MS = 15_000L

        suspend fun token(): String? = runCatching { FirebaseMessaging.getInstance().token.await() }.getOrNull()
    }

    override fun onNewToken(token: String) {
        val repository = (application as BeepApplication).repository
        repository.registerPushToken(token)
    }

    /** Runs on a background thread that Android keeps alive until it returns. */
    override fun onMessageReceived(message: RemoteMessage) {
        val storage = LocalStorage(this)
        val config = storage.loadSettings()
        if (!config.configured) return
        message.data["notice"]?.let { sealed ->
            runCatching { Notices.onNotice(openNotice(storage.identity(), sealed)) }
                .onFailure { Log.w("PushService", "could not open a pushed notice", it) }
            return
        }
        runBlocking {
            withTimeoutOrNull(WAKE_TIMEOUT_MS) {
                val client = BeepClients.client(config, storage.identity())
                runCatching { client.health() }
                delay(SETTLE_MS)
            }
        }
    }
}
