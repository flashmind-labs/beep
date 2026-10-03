package com.flashmind.beep

import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.ffi.BeepException

/** One client per connection settings, so calls share a connection. */
object Beep {
    private var cached: Pair<ConnectionSettings, BeepClient>? = null

    @Synchronized
    fun client(config: ConnectionSettings): BeepClient {
        cached?.let { (settings, client) -> if (settings == config) return client }
        return BeepClient(config.nodeId, config.token).also { cached = config to it }
    }
}

/** Text to show for a failed call. */
fun userMessage(error: Throwable): String = when (error) {
    is BeepException.Unauthorized -> "Your app token was rejected. Check Connection settings."
    is BeepException.NotFound -> "This conversation or run is no longer available on the server."
    is BeepException.Conflict -> "This conversation is busy. Retry shortly."
    is BeepException.BadRequest -> error.reason
    is BeepException.Internal -> "The server hit an error. Try again."
    is BeepException.Connection -> "Can't reach your server: ${error.reason}"
    else -> error.message ?: "Something went wrong."
}
