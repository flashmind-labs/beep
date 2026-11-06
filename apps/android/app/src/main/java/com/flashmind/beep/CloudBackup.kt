package com.flashmind.beep

import android.content.Context
import android.util.Log
import com.google.android.gms.auth.blockstore.Blockstore
import com.google.android.gms.auth.blockstore.RetrieveBytesRequest
import com.google.android.gms.auth.blockstore.StoreBytesData
import kotlinx.coroutines.tasks.await

/** An identity found in the user's Google account, with the server it used. */
class CloudIdentity(val identity: ByteArray, val serverUrl: String)

/**
 * Keeps the iroh identity and the server's address in Google Block Store,
 * which encrypts them end to end with the screen lock and restores them on
 * a new phone signed into the same account. That key is the whole account,
 * so restoring it signs the user back in.
 */
class CloudBackup(context: Context) {
    private val client = Blockstore.getClient(context.applicationContext)

    /** False without a screen lock or Google Play services. Then nothing is saved, because Google could read it. */
    private suspend fun available(): Boolean = try {
        client.isEndToEndEncryptionAvailable.await()
    } catch (failure: Exception) {
        Log.w(TAG, "End-to-end encryption check failed", failure)
        false
    }

    /** Returns whether the identity is now in the backup. */
    suspend fun save(identity: ByteArray, serverUrl: String): Boolean {
        if (!available()) return false
        return try {
            store(IDENTITY, identity)
            store(SERVER, serverUrl.toByteArray())
            true
        } catch (failure: Exception) {
            Log.w(TAG, "Could not save the cloud backup", failure)
            false
        }
    }

    suspend fun load(): CloudIdentity? {
        val request = RetrieveBytesRequest.Builder().setKeys(listOf(IDENTITY, SERVER)).build()
        val entries = try {
            client.retrieveBytes(request).await().blockstoreDataMap
        } catch (failure: Exception) {
            Log.w(TAG, "Could not read the cloud backup", failure)
            return null
        }
        val identity = entries[IDENTITY]?.bytes?.takeIf { it.size == 32 } ?: return null
        return CloudIdentity(identity, entries[SERVER]?.bytes?.toString(Charsets.UTF_8).orEmpty())
    }

    private suspend fun store(key: String, bytes: ByteArray) {
        val data = StoreBytesData.Builder().setKey(key).setBytes(bytes).setShouldBackupToCloud(true).build()
        client.storeBytes(data).await()
    }

    private companion object {
        const val IDENTITY = "identity"
        const val SERVER = "server"
        const val TAG = "CloudBackup"
    }
}
