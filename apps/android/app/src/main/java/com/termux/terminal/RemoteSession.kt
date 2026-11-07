package com.termux.terminal

import android.os.Handler
import android.os.Looper

// TerminalSession is final and starts a local shell on its first resize. A session whose emulator is
// set here never starts one. This file sits in the library's package to reach what the library keeps
// package-private: the emulator field and the queue that holds what the view writes.

/** The size the emulator has until the view lays out and resizes it. */
private const val START_COLUMNS = 80
private const val START_ROWS = 24
private const val SCROLLBACK_ROWS = 2000

/**
 * A session with no process. Output goes in through [appendRemote]; whatever the view or the
 * emulator writes (keys, pastes, replies) is passed to [onInput] on the main thread, in order.
 */
fun remoteSession(client: TerminalSessionClient, foreground: Int, background: Int, onInput: (ByteArray) -> Unit): TerminalSession {
    // The defaults are shared by every emulator and survive a reset.
    TerminalColors.COLOR_SCHEME.mDefaultColors[TextStyle.COLOR_INDEX_FOREGROUND] = foreground
    TerminalColors.COLOR_SCHEME.mDefaultColors[TextStyle.COLOR_INDEX_BACKGROUND] = background
    val session = TerminalSession("", "", emptyArray(), emptyArray(), SCROLLBACK_ROWS, client)
    session.mEmulator = TerminalEmulator(session, START_COLUMNS, START_ROWS, 0, 0, SCROLLBACK_ROWS, client)
    // write() drops input while mShellPid is not positive. Nothing signals it: finishIfRunning is never called.
    session.mShellPid = 1
    val main = Handler(Looper.getMainLooper())
    val queue = session.mTerminalToProcessIOQueue
    Thread({
        val buffer = ByteArray(4096)
        while (true) {
            val read = queue.read(buffer, true)
            if (read < 0) return@Thread
            val chunk = buffer.copyOf(read)
            main.post { onInput(chunk) }
        }
    }, "remote-terminal-input").start()
    return session
}

/** Stops the input thread. Call on the main thread; later writes are dropped. */
fun TerminalSession.closeRemote() { mTerminalToProcessIOQueue.close() }

fun TerminalSession.appendRemote(data: ByteArray) { mEmulator.append(data, data.size) }

fun TerminalSession.resetRemote() { mEmulator.reset() }
