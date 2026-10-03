package com.flashmind.beep

import android.animation.ValueAnimator
import android.app.Activity
import android.content.ComponentCallbacks
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.graphics.Point
import android.graphics.Rect
import android.net.Uri
import android.provider.Settings
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.WindowInsets
import android.view.WindowManager
import android.view.WindowManager.LayoutParams
import android.view.animation.DecelerateInterpolator
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.FastOutLinearInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.lifecycle.setViewTreeViewModelStoreOwner
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.launch
import java.lang.ref.WeakReference
import kotlin.math.hypot

/** The two floating chats: Beep's own overlay with a live face, or Android's built-in bubble. */
enum class BubbleKind(val label: String, val hint: String) {
    System("Android bubble", "The system's chat bubble, no extra permission"),
    Overlay("Beep bubble", "Animated face, floats over every app"),
}

/**
 * Beep's bubble: an overlay window drawn in Compose. Drag it around, drop it on the X to
 * dismiss it, tap it to open the chat panel ([PanelActivity]). Needs "Display over other apps".
 */
object Bubble {
    private const val LEAVE_MS = 280L
    private const val PREFS = "bubble"
    private const val SIZE_DP = 64
    private const val MARGIN_DP = 8
    private const val TARGET_DP = 72

    val face = MutableStateFlow(BeepFace.Idle)
    private val look = mutableStateOf(Offset.Zero)
    private val nearTarget = mutableStateOf(false)
    private val targetShown = mutableStateOf(false)
    /** Set while the bubble is being thrown away, for its exit animation. */
    private val leaving = mutableStateOf(false)
    /** Whether the full app is on screen, where the bubble would only be in the way. */
    private var appInFront = false
    private var bubbleView: ComposeView? = null
    private var target: ComposeView? = null
    private var params: LayoutParams? = null
    private var targetParams: LayoutParams? = null
    private var owner: OverlayOwner? = null
    private var callbacks: ComponentCallbacks? = null
    private var motion: ValueAnimator? = null
    private var panel: WeakReference<Activity>? = null
    /** Where the bubble sat before it moved aside for the open panel. */
    private var rest: Point? = null

    fun allowed(context: Context) = Settings.canDrawOverlays(context)
    fun settings(context: Context) {
        context.startActivity(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:${context.packageName}"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
    /** Whether the user left the bubble floating, so it comes back when the app process restarts. */
    fun wanted(context: Context) = kind(context) == BubbleKind.Overlay && prefs(context).getBoolean("shown", false)

    /** Which floating chat the user picked in settings. */
    val kind = mutableStateOf<BubbleKind?>(null)
    fun kind(context: Context): BubbleKind = kind.value ?: (prefs(context).getString("kind", null)
        ?.let { saved -> BubbleKind.entries.find { it.name == saved } } ?: BubbleKind.System).also { kind.value = it }
    /** Switch kinds, moving a chat that is already floating over to the new one. */
    fun pick(context: Context, picked: BubbleKind) {
        if (picked == kind(context)) return
        prefs(context).edit().putString("kind", picked.name).apply()
        kind.value = picked
        val floating = bubbleView != null || SystemBubble.shown(context)
        hide(context)
        SystemBubble.hide(context)
        if (!floating) return
        if (picked == BubbleKind.Overlay) { if (!show(context)) settings(context) } else SystemBubble.show(context)
    }
    /** Whether the user has been sent to grant the permission once already, so it is not asked for every time. */
    fun asked(context: Context) = prefs(context).getBoolean("asked", false)
    fun markAsked(context: Context) = prefs(context).edit().putBoolean("asked", true).apply()

    /** Float the bubble. Returns false when the overlay permission is missing. */
    fun show(context: Context): Boolean {
        if (!allowed(context)) return false
        val app = context.applicationContext
        prefs(app).edit().putBoolean("shown", true).apply()
        if (bubbleView != null) return true
        // The face follows the repository, which only starts its watch once it exists.
        (app as? BeepApplication)?.repository
        Avatars.load(app)
        val windows = app.getSystemService(WindowManager::class.java)
        val owner = OverlayOwner().also { this.owner = it }
        val size = px(app, SIZE_DP)
        val bounds = screen(app)
        // Back where the user last left it: which side, and how far down.
        val saved = prefs(app)
        val bubbleParams = overlay(size, size, touchable = true).apply {
            x = if (saved.getBoolean("left", false)) bounds.left else bounds.right - size
            y = (bounds.top + saved.getFloat("down", 1 / 3f) * bounds.height()).toInt().coerceIn(bounds.top, bounds.bottom - size)
        }
        val view = ComposeView(app).apply {
            owner.attach(this)
            setContent {
                val current by face.collectAsState()
                // Thrown away: Beep closes its eyes and spins down into the X.
                val gone by animateFloatAsState(if (leaving.value) 0f else 1f, tween(LEAVE_MS.toInt(), easing = FastOutLinearInEasing), label = "gone")
                // A light disc with a shadow, so the bubble reads as floating over any background.
                Box(Modifier.fillMaxSize().graphicsLayer { scaleX = gone; scaleY = gone; alpha = gone; rotationZ = (1 - gone) * 120f }
                    .padding(5.dp).shadow(6.dp, CircleShape).background(Color(0xFFF4F8F5), CircleShape)) {
                    BeepHead(if (leaving.value) BeepFace.Sleepy else current, look, Modifier.fillMaxSize().padding(4.dp))
                }
            }
            setOnTouchListener(Drag(app))
            if (appInFront) visibility = View.GONE
        }
        val targetSize = px(app, TARGET_DP)
        val dismiss = overlay(targetSize, targetSize, touchable = false).apply {
            x = bounds.centerX() - targetSize / 2; y = bounds.bottom - targetSize - px(app, 40)
        }
        val targetView = ComposeView(app).apply {
            owner.attach(this)
            setContent { DismissTarget() }
        }
        windows.addView(targetView, dismiss)
        windows.addView(view, bubbleParams)
        bubbleView = view; target = targetView; params = bubbleParams; targetParams = dismiss
        owner.resume()
        callbacks = object : ComponentCallbacks {
            // A fold, unfold or rotation changes the screen under the bubble.
            override fun onConfigurationChanged(newConfig: Configuration) { view.post { relayout(app) } }
            @Deprecated("Deprecated in Java") override fun onLowMemory() {}
        }.also { app.registerComponentCallbacks(it) }
        return true
    }

    fun hide(context: Context) {
        val app = context.applicationContext
        prefs(app).edit().putBoolean("shown", false).apply()
        panel?.get()?.finish()
        motion?.cancel()
        val windows = app.getSystemService(WindowManager::class.java)
        bubbleView?.let { windows.removeView(it) }
        target?.let { windows.removeView(it) }
        callbacks?.let { app.unregisterComponentCallbacks(it) }
        owner?.destroy()
        bubbleView = null; target = null; params = null; targetParams = null; owner = null; callbacks = null; rest = null
        leaving.value = false; targetShown.value = false; nearTarget.value = false
    }

    /** Called by the full app as it comes and goes: the bubble steps aside while the app is on screen. */
    fun appShown(shown: Boolean) {
        appInFront = shown
        bubbleView?.visibility = if (shown) View.GONE else View.VISIBLE
    }

    /** The panel's top edge, just under the bubble once it has moved to the corner. */
    fun panelTop(context: Context) = screen(context).top + 2 * px(context, MARGIN_DP) + px(context, SIZE_DP)

    /** Called by the panel when it opens: the bubble moves to the top corner, out of its way. */
    fun panelShown(activity: Activity) {
        panel = WeakReference(activity)
        val p = params ?: return
        if (rest == null) rest = Point(p.x, p.y)
        val bounds = screen(activity)
        moveTo(activity, bounds.right - px(activity, SIZE_DP) - px(activity, MARGIN_DP), bounds.top + px(activity, MARGIN_DP))
    }

    /** Called by the panel when it closes: the bubble goes back to where it was. */
    fun panelHidden(activity: Activity) {
        if (panel?.get() === activity) panel = null
        rest?.let { moveTo(activity, it.x, it.y) }
        rest = null
    }

    private fun toggle(context: Context) {
        val open = panel?.get()
        if (open != null) open.finish()
        else context.startActivity(Intent(context, PanelActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** Keep the bubble on screen and against an edge after the screen changes size. */
    private fun relayout(context: Context) {
        val p = params ?: return
        val bounds = screen(context)
        targetParams?.let { t ->
            t.x = bounds.centerX() - t.width / 2; t.y = bounds.bottom - t.height - px(context, 40)
            target?.let { context.getSystemService(WindowManager::class.java).updateViewLayout(it, t) }
        }
        rest = null
        snapToEdge(context, p.x + p.width / 2 < bounds.centerX())
    }

    private fun snapToEdge(context: Context, left: Boolean) {
        val p = params ?: return
        val bounds = screen(context)
        val size = px(context, SIZE_DP)
        val y = p.y.coerceIn(bounds.top, bounds.bottom - size)
        prefs(context).edit().putBoolean("left", left).putFloat("down", (y - bounds.top).toFloat() / bounds.height()).apply()
        moveTo(context, if (left) bounds.left else bounds.right - size, y)
    }

    private fun moveTo(context: Context, x: Int, y: Int) {
        val p = params ?: return
        val view = bubbleView ?: return
        val windows = context.applicationContext.getSystemService(WindowManager::class.java)
        motion?.cancel()
        val fromX = p.x; val fromY = p.y
        motion = ValueAnimator.ofFloat(0f, 1f).apply {
            duration = 260
            interpolator = DecelerateInterpolator()
            addUpdateListener {
                val t = it.animatedValue as Float
                p.x = (fromX + (x - fromX) * t).toInt(); p.y = (fromY + (y - fromY) * t).toInt()
                if (view.isAttachedToWindow) windows.updateViewLayout(view, p)
            }
            start()
        }
    }

    /** Tap to open or close the panel; drag to move, and drop on the X to dismiss. */
    private class Drag(private val context: Context) : View.OnTouchListener {
        private val slop = ViewConfiguration.get(context).scaledTouchSlop
        private val windows = context.getSystemService(WindowManager::class.java)
        private var downX = 0f; private var downY = 0f; private var lastX = 0f; private var lastY = 0f
        private var startX = 0; private var startY = 0
        private var dragging = false

        override fun onTouch(view: View, event: MotionEvent): Boolean {
            val p = params ?: return false
            if (leaving.value) return true
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    motion?.cancel()
                    downX = event.rawX; downY = event.rawY; lastX = downX; lastY = downY
                    startX = p.x; startY = p.y; dragging = false
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.rawX - downX; val dy = event.rawY - downY
                    if (!dragging && hypot(dx, dy) > slop) {
                        dragging = true
                        // A drag puts the bubble down wherever it is dropped, so it does not go back after the panel.
                        rest = null
                        panel?.get()?.finish()
                        targetShown.value = true
                        view.performHapticFeedback(HapticFeedbackConstants.GESTURE_START)
                    }
                    if (!dragging) return true
                    // Eyes follow the drag.
                    look.value = Offset(((event.rawX - lastX) / 12f).coerceIn(-1f, 1f), ((event.rawY - lastY) / 12f).coerceIn(-1f, 1f))
                    lastX = event.rawX; lastY = event.rawY
                    val t = targetParams ?: return true
                    val bubbleCenterX = startX + dx + p.width / 2; val bubbleCenterY = startY + dy + p.height / 2
                    val targetCenterX = t.x + t.width / 2f; val targetCenterY = t.y + t.height / 2f
                    val near = hypot(bubbleCenterX - targetCenterX, bubbleCenterY - targetCenterY) < px(context, 90)
                    if (near && !nearTarget.value) view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                    nearTarget.value = near
                    if (nearTarget.value) {
                        // Pull the bubble onto the X so it is clear what letting go will do.
                        p.x = (targetCenterX - p.width / 2).toInt(); p.y = (targetCenterY - p.height / 2).toInt()
                    } else {
                        p.x = (startX + dx).toInt(); p.y = (startY + dy).toInt()
                    }
                    windows.updateViewLayout(view, p)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    look.value = Offset.Zero
                    if (!(dragging && nearTarget.value)) targetShown.value = false
                    if (!dragging) {
                        if (event.actionMasked == MotionEvent.ACTION_UP) { view.performClick(); toggle(context) }
                    } else if (nearTarget.value) {
                        view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                        leaving.value = true
                        // The X pops as it swallows the bubble, then both go.
                        view.postDelayed({ targetShown.value = false }, LEAVE_MS)
                        view.postDelayed({ hide(context) }, LEAVE_MS + 200)
                    } else {
                        snapToEdge(context, p.x + p.width / 2 < screen(context).centerX())
                    }
                }
            }
            return true
        }
    }

    @androidx.compose.runtime.Composable
    private fun DismissTarget() {
        val grow by animateFloatAsState(if (leaving.value) 1.12f else if (nearTarget.value) 1f else 0.8f, label = "grow")
        // Fades and rises in when a drag starts, and drops away when it ends.
        val shown by animateFloatAsState(if (targetShown.value) 1f else 0f, tween(180), label = "shown")
        if (shown == 0f) return
        Canvas(Modifier.fillMaxSize().graphicsLayer { alpha = shown; translationY = (1 - shown) * size.height * 0.6f }) {
            scale(grow) {
                drawCircle(Color(0xCC10201A))
                val arm = size.minDimension * 0.16f
                val stroke = size.minDimension * 0.06f
                drawLine(Color.White, center - Offset(arm, arm), center + Offset(arm, arm), stroke, StrokeCap.Round)
                drawLine(Color.White, center - Offset(arm, -arm), center + Offset(arm, -arm), stroke, StrokeCap.Round)
            }
        }
    }

    private fun overlay(width: Int, height: Int, touchable: Boolean) = LayoutParams(
        width, height, LayoutParams.TYPE_APPLICATION_OVERLAY,
        LayoutParams.FLAG_NOT_FOCUSABLE or LayoutParams.FLAG_LAYOUT_IN_SCREEN or LayoutParams.FLAG_LAYOUT_NO_LIMITS or
            (if (touchable) 0 else LayoutParams.FLAG_NOT_TOUCHABLE),
        PixelFormat.TRANSLUCENT,
    ).apply {
        gravity = Gravity.TOP or Gravity.START
        layoutInDisplayCutoutMode = LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
        fitInsetsTypes = 0
    }

    /** The screen minus the status bar, navigation bar and cutout, in the overlay's coordinates. */
    private fun screen(context: Context): Rect {
        val metrics = context.getSystemService(WindowManager::class.java).currentWindowMetrics
        val insets = metrics.windowInsets.getInsetsIgnoringVisibility(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
        val bounds = metrics.bounds
        return Rect(insets.left, insets.top, bounds.width() - insets.right, bounds.height() - insets.bottom)
    }

    private fun px(context: Context, dp: Int) = (dp * context.resources.displayMetrics.density).toInt()
    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

/** Lifecycle and saved state for Compose views that live in overlay windows rather than an activity. */
private class OverlayOwner : SavedStateRegistryOwner, ViewModelStoreOwner {
    private val registry = LifecycleRegistry(this)
    private val savedState = SavedStateRegistryController.create(this)
    override val lifecycle: Lifecycle get() = registry
    override val savedStateRegistry get() = savedState.savedStateRegistry
    override val viewModelStore = ViewModelStore()
    init {
        savedState.performRestore(null)
        registry.currentState = Lifecycle.State.CREATED
    }
    fun attach(view: View) {
        view.setViewTreeLifecycleOwner(this)
        view.setViewTreeSavedStateRegistryOwner(this)
        view.setViewTreeViewModelStoreOwner(this)
    }
    fun resume() { registry.currentState = Lifecycle.State.RESUMED }
    fun destroy() { registry.currentState = Lifecycle.State.DESTROYED; viewModelStore.clear() }
}

private const val REACTION_MS = 4000L

/** Keeps the bubble's face in step with the assistant: thinking during a run, a short reaction when one ends. */
@OptIn(FlowPreview::class, ExperimentalCoroutinesApi::class)
fun followFace(repository: ChatRepository, scope: CoroutineScope) {
    var running = emptySet<String>()
    var reaction = BeepFace.Idle
    var reactionUntil = 0L
    fun pick(state: ChatState, connected: Boolean): BeepFace {
        val busy = state.conversations.filter { it.busy }.map { it.id }.toSet()
        val finished = state.conversations.filter { it.id in running && it.id !in busy }
        running = busy
        if (finished.isNotEmpty()) {
            // A run the user stopped gets no reaction.
            val failed = finished.any { it.error.isNotEmpty() && it.error != "Stopped" }
            reaction = if (failed) BeepFace.Sad else if (finished.all { it.error.isEmpty() }) BeepFace.Happy else BeepFace.Idle
            reactionUntil = System.currentTimeMillis() + REACTION_MS
        }
        return when {
            busy.isNotEmpty() -> BeepFace.Thinking
            System.currentTimeMillis() < reactionUntil -> reaction
            !connected -> BeepFace.Sleepy
            else -> BeepFace.Idle
        }
    }
    scope.launch {
        combine(repository.state, repository.connected) { state, connected -> state to connected }
            .transformLatest { (state, connected) ->
                emit(pick(state, connected))
                val left = reactionUntil - System.currentTimeMillis()
                if (left > 0) { delay(left); emit(pick(state, connected)) }
            }
            // Brief blips, like the connection check at startup, should not flash a sleepy face.
            .debounce(500).distinctUntilChanged()
            .collect { Bubble.face.value = it }
    }
}
