package com.flashmind.beep

import android.os.SystemClock
import android.view.ActionMode
import android.view.InputDevice
import android.view.Menu
import android.view.MenuItem
import android.view.MotionEvent
import android.view.View
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.Clipboard
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalTextToolbar
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.platform.TextToolbar
import androidx.compose.ui.platform.TextToolbarStatus
import androidx.compose.ui.platform.ViewConfiguration

/**
 * Text selection in a message, past what SelectionContainer does on its own: a double tap
 * selects the word under it, and the selection toolbar gets Quote next to Copy.
 */
@Composable
fun SelectableMessage(onQuote: (String) -> Unit, content: @Composable (WordPicker) -> Unit) {
    val view = LocalView.current
    val clipboard = LocalClipboard.current
    val config = LocalViewConfiguration.current
    val picker = remember(view, config) { WordPicker(view, config) }
    val quoting = remember(clipboard) { QuotingClipboard(clipboard) }
    quoting.onQuote = onQuote
    val toolbar = remember(view, quoting) { QuoteToolbar(view, quoting) }
    CompositionLocalProvider(LocalViewConfiguration provides picker, LocalClipboard provides quoting,
        LocalTextToolbar provides toolbar) { content(picker) }
}

/**
 * Turns a double tap into the long press SelectionContainer selects a word on. There is no
 * public way to set its selection, so it gets a synthetic press with the long press timeout
 * cut to nothing for that one press.
 */
class WordPicker(private val view: View, private val base: ViewConfiguration) : ViewConfiguration by base {
    private var quick = false
    private var coordinates: LayoutCoordinates? = null
    private var lastDown = Offset.Zero
    /** True while the synthetic press and the tap it looks like are still arriving. */
    var picking = false
        private set

    override val longPressTimeoutMillis: Long get() = if (quick) 1L else base.longPressTimeoutMillis

    /** Goes on the element that detects the double tap, to know where it landed. */
    val tracking: Modifier = Modifier
        .onGloballyPositioned { coordinates = it }
        .pointerInput(Unit) {
            awaitEachGesture { lastDown = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial).position }
        }

    fun selectWord() {
        val at = coordinates?.takeIf { it.isAttached }?.localToRoot(lastDown) ?: return
        val down = SystemClock.uptimeMillis()
        quick = true; picking = true
        press(down, down, MotionEvent.ACTION_DOWN, at)
        view.postDelayed({
            press(down, SystemClock.uptimeMillis(), MotionEvent.ACTION_UP, at)
            quick = false
        }, PRESS_MS)
        view.postDelayed({ picking = false }, base.doubleTapTimeoutMillis + PRESS_MS * 2)
    }

    private fun press(down: Long, time: Long, action: Int, at: Offset) {
        val event = MotionEvent.obtain(down, time, action, at.x, at.y, 0)
        event.source = InputDevice.SOURCE_TOUCHSCREEN
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    private companion object {
        const val PRESS_MS = 40L
    }
}

/** Passes copies through, except the one Quote asks for, which goes to [onQuote] and not the clipboard. */
private class QuotingClipboard(private val base: Clipboard) : Clipboard by base {
    var onQuote: (String) -> Unit = {}
    var capture = false

    override suspend fun setClipEntry(clipEntry: ClipEntry?) {
        if (!capture) return base.setClipEntry(clipEntry)
        capture = false
        val text = clipEntry?.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
        if (!text.isNullOrBlank()) onQuote(text)
    }
}

/** The floating selection toolbar: Copy, Quote and Select all. */
private class QuoteToolbar(private val view: View, private val clipboard: QuotingClipboard) : TextToolbar {
    private var mode: ActionMode? = null
    private var rect = Rect.Zero
    private var copy: (() -> Unit)? = null
    private var selectAll: (() -> Unit)? = null

    override var status = TextToolbarStatus.Hidden
        private set

    override fun showMenu(rect: Rect, onCopyRequested: (() -> Unit)?, onPasteRequested: (() -> Unit)?,
                          onCutRequested: (() -> Unit)?, onSelectAllRequested: (() -> Unit)?) {
        this.rect = rect; copy = onCopyRequested; selectAll = onSelectAllRequested
        val current = mode
        if (current == null) {
            mode = view.startActionMode(callback, ActionMode.TYPE_FLOATING)
            status = TextToolbarStatus.Shown
        } else {
            current.invalidate()
            current.invalidateContentRect()
        }
    }

    override fun hide() {
        status = TextToolbarStatus.Hidden
        mode?.finish()
        mode = null
    }

    private val callback = object : ActionMode.Callback2() {
        override fun onCreateActionMode(mode: ActionMode, menu: Menu) = true

        override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean {
            menu.clear()
            if (copy != null) {
                menu.add(0, COPY, 0, android.R.string.copy).setShowAsAction(MenuItem.SHOW_AS_ACTION_ALWAYS)
                menu.add(0, QUOTE, 1, "Quote").setShowAsAction(MenuItem.SHOW_AS_ACTION_ALWAYS)
            }
            if (selectAll != null) menu.add(0, SELECT_ALL, 2, android.R.string.selectAll).setShowAsAction(MenuItem.SHOW_AS_ACTION_ALWAYS)
            return true
        }

        override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean {
            when (item.itemId) {
                COPY -> copy?.invoke()
                // The selection only comes out through a copy, which the clipboard catches.
                QUOTE -> { clipboard.capture = true; copy?.invoke() }
                SELECT_ALL -> { selectAll?.invoke(); return true }
            }
            mode.finish()
            return true
        }

        override fun onDestroyActionMode(mode: ActionMode) {
            this@QuoteToolbar.mode = null
            status = TextToolbarStatus.Hidden
        }

        override fun onGetContentRect(mode: ActionMode, view: View, outRect: android.graphics.Rect) {
            outRect.set(rect.left.toInt(), rect.top.toInt(), rect.right.toInt(), rect.bottom.toInt())
        }
    }

    private companion object {
        const val COPY = 1
        const val QUOTE = 2
        const val SELECT_ALL = 3
    }
}
