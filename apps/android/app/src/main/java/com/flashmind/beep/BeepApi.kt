package com.flashmind.beep

import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.ffi.BeepException

/** One client per server and identity, so calls share a connection. */
object BeepClients {
    private class Cached(val config: ConnectionSettings, val identity: ByteArray, val client: BeepClient)
    private var cached: Cached? = null

    @Synchronized
    fun client(config: ConnectionSettings, identity: ByteArray): BeepClient {
        cached?.let { if (it.config == config && it.identity.contentEquals(identity)) return it.client }
        // The old client's endpoint would otherwise keep pinging its relay until collected.
        cached?.client?.destroy()
        return BeepClient(config.nodeId, identity).also {
            it.setListener(Notices)
            cached = Cached(config, identity, it)
        }
    }

    /** The client in use, without making one. */
    @Synchronized
    fun current(): BeepClient? = cached?.client
}

/** Text to show for a failed call. */
fun userMessage(error: Throwable): String = when (error) {
    is BeepException.Unauthorized -> "The server refused this phone."
    is BeepException.NotFound -> "This conversation or run is no longer available on the server."
    is BeepException.Conflict -> "This conversation is busy. Retry shortly."
    is BeepException.BadRequest -> error.reason
    is BeepException.Internal -> "The server hit an error. Try again."
    is BeepException.Connection -> "Can't reach your server: ${error.reason}"
    else -> error.message ?: "Something went wrong."
}
