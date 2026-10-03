package com.flashmind.beep

import android.net.Uri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketTimeoutException
import kotlin.coroutines.coroutineContext

/** What the provider sent back to the localhost redirect. */
data class AuthRedirect(val code: String, val state: String, val issuer: String?)

/**
 * Catches an OAuth redirect for providers that only allow `http://localhost`, such as
 * Fastmail. It listens on a random loopback port, gives the redirect URI to [start],
 * opens the returned sign-in page, and hands the code to [finish] once the browser comes
 * back. Tokens stay on the server; this only relays the one-time code.
 */
object LoopbackAuth {
    private const val TIMEOUT_MS = 5 * 60 * 1000L

    suspend fun run(
        start: suspend (redirectUri: String) -> String,
        open: (url: String) -> Unit,
        finish: suspend (AuthRedirect) -> Unit,
    ) = withContext(Dispatchers.IO) {
        val v4 = ServerSocket(0, 4, InetAddress.getByName("127.0.0.1"))
        // The browser may resolve localhost to ::1, so listen there too when the port is free.
        val v6 = try { ServerSocket(v4.localPort, 4, InetAddress.getByName("::1")) } catch (_: IOException) { null }
        val sockets = listOfNotNull(v4, v6)
        // accept() ignores coroutine cancellation, so wake up every second to check it.
        sockets.forEach { it.soTimeout = 1000 }
        try {
            val url = start("http://localhost:${v4.localPort}/callback")
            open(url)
            withTimeout(TIMEOUT_MS) {
                coroutineScope {
                    val waits = sockets.map { socket -> async { awaitRedirect(socket, finish) } }
                    select { waits.forEach { wait -> wait.onAwait { it } } }.also { waits.forEach { it.cancel() } }
                }
            }
        } finally {
            sockets.forEach { runCatching { it.close() } }
        }
    }

    /** Serves requests until one carries the OAuth result, then answers it with how it went. */
    private suspend fun awaitRedirect(server: ServerSocket, finish: suspend (AuthRedirect) -> Unit) {
        while (true) {
            coroutineContext.ensureActive()
            val socket = try { server.accept() } catch (_: SocketTimeoutException) { continue }
            socket.use {
                it.soTimeout = 10_000
                val target = readTarget(it) ?: return@use
                val uri = Uri.parse("http://localhost$target")
                if (uri.path != "/callback") { reply(it, 404, "Not found"); return@use }
                val code = uri.getQueryParameter("code")
                val state = uri.getQueryParameter("state")
                if (code == null || state == null) {
                    reply(it, 400, "Sign-in was cancelled. You can close this tab and try again in Beep.")
                    throw IOException(uri.getQueryParameter("error_description") ?: uri.getQueryParameter("error") ?: "Sign-in was cancelled")
                }
                try {
                    finish(AuthRedirect(code, state, uri.getQueryParameter("iss")))
                } catch (e: Exception) {
                    reply(it, 500, "Beep could not finish connecting. Go back to Beep and try again.")
                    throw e
                }
                reply(it, 200, "Connected. You can go back to Beep.")
                return
            }
        }
    }

    /** The request target from `GET /callback?... HTTP/1.1`, or null for anything else. */
    private fun readTarget(socket: Socket): String? {
        val line = try { socket.getInputStream().bufferedReader().readLine() } catch (_: IOException) { null } ?: return null
        val parts = line.split(' ')
        return if (parts.size == 3 && parts[0] == "GET" && parts[1].startsWith("/")) parts[1] else null
    }

    private fun reply(socket: Socket, status: Int, message: String) {
        val body = """<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1">
            |<title>Beep</title><body style="font-family:sans-serif;padding:2em;text-align:center">
            |<p style="font-size:1.2em">$message</p></body>""".trimMargin()
        val bytes = body.toByteArray()
        val reason = when (status) { 200 -> "OK"; 400 -> "Bad Request"; 404 -> "Not Found"; else -> "Error" }
        val head = "HTTP/1.1 $status $reason\r\nContent-Type: text/html; charset=utf-8\r\n" +
            "Content-Length: ${bytes.size}\r\nConnection: close\r\n\r\n"
        runCatching {
            socket.getOutputStream().apply { write(head.toByteArray()); write(bytes); flush() }
        }
    }
}
