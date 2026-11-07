package com.flashmind.beep.center

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.util.TypedValue
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.inputmethod.InputMethodManager
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.termux.terminal.TerminalSession
import com.termux.terminal.TerminalSessionClient
import com.termux.terminal.appendRemote
import com.termux.terminal.closeRemote
import com.termux.terminal.remoteSession
import com.termux.terminal.resetRemote
import com.termux.view.TerminalView
import com.termux.view.TerminalViewClient
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

/**
 * Owns the Termux view, the sticky modifiers and the font size. The screen feeds it output and reads
 * the grid it settles on; input and size changes go out through the callbacks of [connect].
 * Used on the main thread only.
 */
class TerminalEmulatorModel(private val context: Context) {
    companion object {
        val fontRange = 9..20
        const val BACKGROUND = 0xFF0B0C0E.toInt()
        const val FOREGROUND = 0xFFD0D3D8.toInt()
    }

    var cols by mutableIntStateOf(0)
        private set
    var rows by mutableIntStateOf(0)
        private set
    /** True until [connect] or [takeControl]. Nothing is sent while it is set. */
    var readOnly by mutableStateOf(true)
        private set
    var mods by mutableStateOf(emptySet<TermMod>())
        private set

    private val keys = TermKeyState()
    private var fontSize = 11
    private var onInput: ((ByteArray) -> Unit)? = null
    private var onResize: ((Int, Int) -> Unit)? = null
    private var sizeWaiter: CancellableContinuation<Pair<Int, Int>?>? = null

    private val session: TerminalSession = remoteSession(SessionClient(), FOREGROUND, BACKGROUND, ::typed)
    val view = TerminalView(context, null).apply {
        isFocusable = true
        isFocusableInTouchMode = true
        setTerminalViewClient(ViewClient())
        setTextSize(pixels(fontSize))
        attachSession(session)
    }

    private fun pixels(sp: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, sp.toFloat(), context.resources.displayMetrics).toInt()

    /** The grid size once the view has been laid out. A newer call replaces an older waiter, which gets null. */
    suspend fun size(): Pair<Int, Int>? {
        if (cols > 0) return cols to rows
        return suspendCancellableCoroutine { waiter ->
            sizeWaiter?.resume(null)
            sizeWaiter = waiter
            waiter.invokeOnCancellation { if (sizeWaiter === waiter) sizeWaiter = null }
        }
    }

    /** Starts sending input and size changes and opens the keyboard. A grid different from the attach size is sent now. */
    fun connect(attachedAt: Pair<Int, Int>, input: (ByteArray) -> Unit, resize: (Int, Int) -> Unit, controlling: Boolean = true) {
        onInput = input
        onResize = resize
        if (!controlling) return
        lockInput(false)
        if (cols > 0 && (cols to rows) != attachedAt) resize(cols, rows)
    }

    fun disconnect() { onInput = null; onResize = null }

    /** Leaves view only: input comes back and the current grid is sent once. */
    fun takeControl() {
        lockInput(false)
        if (cols > 0) onResize?.invoke(cols, rows)
    }

    /** Hides the key bar and the keyboard and sends nothing, no input and no resize. Output keeps coming. */
    fun viewOnly() = lockInput(true)

    fun feed(data: ByteArray) {
        session.appendRemote(data)
        view.onScreenUpdated()
    }

    fun reset() {
        session.resetRemote()
        view.onScreenUpdated()
    }

    /** Stops the input thread. The model is not used afterwards. */
    fun close() {
        disconnect()
        session.closeRemote()
    }

    fun press(key: TermKey) {
        if (readOnly) return
        val bytes = keys.press(key)
        mods = keys.mods
        onInput?.invoke(bytes)
    }

    fun toggle(mod: TermMod) {
        if (readOnly) return
        keys.toggle(mod)
        mods = keys.mods
    }

    private fun lockInput(value: Boolean) {
        if (value == readOnly) return
        readOnly = value
        val input = context.getSystemService(InputMethodManager::class.java)
        if (value) {
            input.hideSoftInputFromWindow(view.windowToken, 0)
            view.clearFocus()
        } else {
            view.requestFocus()
            input.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
        }
    }

    /** What the view and the emulator write: soft keyboard text, pastes and replies. */
    private fun typed(bytes: ByteArray) {
        if (readOnly) return
        val out = keys.applyTyped(bytes)
        mods = keys.mods
        onInput?.invoke(out)
    }

    private fun setFont(size: Int) {
        val next = size.coerceIn(fontRange)
        if (next == fontSize) return
        fontSize = next
        view.setTextSize(pixels(next))
    }

    private fun gridChanged() {
        val emulator = view.mEmulator ?: return
        val newCols = emulator.mColumns
        val newRows = emulator.mRows
        if (newCols <= 0 || newRows <= 0 || (newCols == cols && newRows == rows)) return
        cols = newCols
        rows = newRows
        val waiter = sizeWaiter
        if (waiter != null) { sizeWaiter = null; waiter.resume(newCols to newRows) }
        else if (!readOnly) onResize?.invoke(newCols, newRows)
    }

    private inner class ViewClient : TerminalViewClient {
        override fun onScale(scale: Float): Float {
            if (scale in 0.9f..1.1f) return scale
            setFont(fontSize + if (scale > 1f) 1 else -1)
            return 1f
        }
        override fun onSingleTapUp(e: MotionEvent) {
            if (readOnly) return
            view.requestFocus()
            context.getSystemService(InputMethodManager::class.java).showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
        }
        override fun shouldBackButtonBeMappedToEscape() = false
        override fun shouldEnforceCharBasedInput() = true
        override fun shouldUseCtrlSpaceWorkaround() = false
        override fun isTerminalViewSelected() = true
        override fun copyModeChanged(copyMode: Boolean) {}
        override fun onKeyDown(keyCode: Int, e: KeyEvent, session: TerminalSession) = false
        override fun onKeyUp(keyCode: Int, e: KeyEvent) = false
        override fun onLongPress(event: MotionEvent) = false
        override fun readControlKey() = false
        override fun readAltKey() = false
        override fun readShiftKey() = false
        override fun readFnKey() = false
        override fun onCodePoint(codePoint: Int, ctrlDown: Boolean, session: TerminalSession) = false
        override fun onEmulatorSet() = gridChanged()
        override fun logError(tag: String?, message: String?) {}
        override fun logWarn(tag: String?, message: String?) {}
        override fun logInfo(tag: String?, message: String?) {}
        override fun logDebug(tag: String?, message: String?) {}
        override fun logVerbose(tag: String?, message: String?) {}
        override fun logStackTraceWithMessage(tag: String?, message: String?, e: Exception?) {}
        override fun logStackTrace(tag: String?, e: Exception?) {}
    }

    private inner class SessionClient : TerminalSessionClient {
        override fun onTextChanged(changedSession: TerminalSession) { view.onScreenUpdated() }
        override fun onTitleChanged(changedSession: TerminalSession) {}
        override fun onSessionFinished(finishedSession: TerminalSession) {}
        override fun onCopyTextToClipboard(session: TerminalSession, text: String?) {
            context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("terminal", text.orEmpty()))
        }
        override fun onPasteTextFromClipboard(session: TerminalSession?) {
            val text = context.getSystemService(ClipboardManager::class.java).primaryClip?.getItemAt(0)?.coerceToText(context)
            if (!text.isNullOrEmpty()) view.mEmulator?.paste(text.toString())
        }
        override fun onBell(session: TerminalSession) {}
        override fun onColorsChanged(session: TerminalSession) { view.onScreenUpdated() }
        override fun onTerminalCursorStateChange(state: Boolean) {}
        override fun getTerminalCursorStyle(): Int? = null
        override fun logError(tag: String?, message: String?) {}
        override fun logWarn(tag: String?, message: String?) {}
        override fun logInfo(tag: String?, message: String?) {}
        override fun logDebug(tag: String?, message: String?) {}
        override fun logVerbose(tag: String?, message: String?) {}
        override fun logStackTraceWithMessage(tag: String?, message: String?, e: Exception?) {}
        override fun logStackTrace(tag: String?, e: Exception?) {}
    }
}

@Composable
internal fun TerminalEmulator(model: TerminalEmulatorModel, modifier: Modifier = Modifier) {
    AndroidView(factory = { model.view }, modifier = modifier)
}
