package com.flashmind.neobeep

import kotlinx.coroutines.suspendCancellableCoroutine
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

object NativeQuic {
    init { System.loadLibrary("neobeep_android") }
    interface Callback { fun onChunk(bytes: ByteArray) }
    @JvmStatic external fun createCall(): Long
    @JvmStatic external fun cancel(id: Long)
    @JvmStatic external fun request(id: Long, address: String, hostname: String, ca: String,
        token: String, method: String, path: String, body: String, callback: Callback): Int
}

class QuicApi(private val config: ConnectionSettings) {
    companion object { private val workers = Executors.newFixedThreadPool(4) }
    suspend fun request(method: String, path: String, body: String = "", onChunk: (ByteArray) -> Unit): Int =
        suspendCancellableCoroutine { continuation ->
            val id = NativeQuic.createCall()
            continuation.invokeOnCancellation { NativeQuic.cancel(id) }
            workers.execute {
                try {
                    val status = NativeQuic.request(id, config.address.trim(), config.hostname.trim(), config.ca,
                        config.token.trim(), method, path, body, object : NativeQuic.Callback {
                            override fun onChunk(bytes: ByteArray) { onChunk(bytes) }
                        })
                    if (continuation.isActive) continuation.resume(status)
                } catch (error: Exception) {
                    if (continuation.isActive) continuation.resumeWithException(error)
                }
            }
        }
    suspend fun json(method: String, path: String, body: String = ""): String {
        val output = ByteArrayOutputStream()
        val status = request(method, path, body) { output.write(it) }
        checkStatus(status)
        return output.toString(Charsets.UTF_8.name())
    }
    fun checkStatus(status: Int) {
        when (status) {
            in 200..299 -> Unit
            401 -> error("Your app token was rejected. Check Connection settings.")
            404 -> error("This conversation or run is no longer available on the server.")
            409 -> error("This conversation is busy or the request conflicts. Retry shortly.")
            else -> error("Server returned HTTP $status.")
        }
    }
}

/** Buffer bytes until newline: UTF-8 characters may span network chunks. */
class NdjsonDecoder(private val consume: (String) -> Unit) {
    private val pending = ByteArrayOutputStream()
    fun feed(bytes: ByteArray) {
        for (byte in bytes) {
            if (byte == 10.toByte()) {
                val line = pending.toString(Charsets.UTF_8.name()).trim()
                pending.reset()
                if (line.isNotEmpty()) consume(line)
            } else {
                check(pending.size() < 1_048_576) { "Server event exceeds size limit" }
                pending.write(byte.toInt())
            }
        }
    }
    fun finish() { check(pending.size() == 0) { "Incomplete event; reconnecting" } }
}
