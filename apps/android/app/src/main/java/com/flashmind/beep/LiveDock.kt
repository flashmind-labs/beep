package com.flashmind.beep

import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

/** Live mode's own colors: mint while it listens, coral while Beep speaks. */
private val Listen = Color(0xFF3B9B7A)
private val Speak = Color(0xFFF07A6A)
private val SpeakWord = Color(0xFFD9604F)

/** The spoken commands, shown while nothing has been said yet: what to say, and what it does. */
private val COMMANDS = listOf("send" to "sends", "cancel" to "drops it", "scratch that" to "", "stop" to "", "repeat that" to "",
    "new chat" to "", "bye" to "leaves")

/** The hint under the caption for [state], or null for none. */
internal fun liveHint(state: LiveState, sendOnPause: Boolean, waiting: Boolean): String? {
    if (state.problem != null) return null
    if (waiting || state.stage == LiveStage.Waiting) return "Beep goes on once you answer"
    if (state.voiceUnavailable && state.stage == LiveStage.Speaking) return "Beep’s voice is unavailable, so its replies only show here"
    return when (state.stage) {
        LiveStage.Connecting -> null
        LiveStage.Listening -> when {
            state.soon != null -> "Sending… keep talking to add more"
            sendOnPause -> "Pause to send · Tap the orb to leave · Hold it to talk"
            else -> "Say “send” when you’re done · Tap the orb to leave"
        }
        LiveStage.Holding -> "Let go to send · Slide away to cancel"
        LiveStage.Thinking -> "Tap the orb to stop the answer"
        LiveStage.Speaking -> "Tap the orb or say “stop” to interrupt"
        LiveStage.Waiting -> null
    }
}

/** The end of [text] when it runs long, so the newest words stay in view. */
private fun tail(text: String, max: Int = 160): String =
    if (text.length <= max) text else "…" + text.takeLast(max).substringAfter(' ')

/**
 * Live mode in place of the composer: the orb, what it is doing, the words being heard or the
 * line being read, and the controls. The chat above stays as it is, so turns, cards and the
 * approval card are read and answered there.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun LiveDock(live: LiveVoice, compact: Boolean) {
    val state by live.state.collectAsState()
    val level by live.level.collectAsState()
    val context = LocalContext.current
    val view = LocalView.current
    val still = reduceMotion()
    val waiting = state.ask != null || state.stage == LiveStage.Waiting
    val replying = state.problem == null && !waiting && (state.stage == LiveStage.Thinking || state.stage == LiveStage.Speaking)
    // Leaving during a reply cancels it too, as the close button does.
    fun leave() { if (state.problem != null) live.close() else live.close(LeaveCause.Closed) }
    BackHandler { leave() }
    DisposableEffect(view) {
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = false }
    }
    // Each launch slides the dock up and springs the orb out.
    val opening = remember { Animatable(if (still) 1f else 0f) }
    LaunchedEffect(state.launches) {
        if (still) return@LaunchedEffect
        opening.snapTo(0f)
        opening.animateTo(1f, spring(dampingRatio = 0.6f, stiffness = 260f))
    }
    val (word, wordColor) = when {
        state.problem != null -> "Voice unavailable" to Beep.colors.muted
        waiting -> "Waiting for you" to Beep.colors.muted
        state.stage == LiveStage.Connecting -> "Connecting" to Beep.colors.muted
        state.stage == LiveStage.Listening -> "Listening" to Listen
        state.stage == LiveStage.Holding -> "Holding" to Listen
        state.stage == LiveStage.Thinking -> "Thinking" to Beep.colors.muted
        else -> "Speaking" to SpeakWord
    }
    val hint = liveHint(state, live.sendOnPause, waiting)
    val commands = state.problem == null && !waiting && state.stage == LiveStage.Listening && state.words.isEmpty() && state.line == null
    Box(Modifier.fillMaxWidth().padding(horizontal = 10.dp).padding(top = if (compact) 4.dp else 8.dp, bottom = 10.dp)
        .graphicsLayer { val o = opening.value.coerceIn(0f, 1f); alpha = o; translationY = (1 - opening.value) * 28.dp.toPx() }) {
        Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(24.dp), Beep.colors.surface, pressableLook())
            .padding(start = 8.dp, end = 10.dp, top = 10.dp, bottom = 10.dp).semantics { contentDescription = "Live voice" },
            verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Orb(state, waiting, { level }, still, live, Modifier.graphicsLayer { val s = 0.2f + 0.8f * opening.value; scaleX = s; scaleY = s })
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(word.uppercase(), fontSize = 11.sp, lineHeight = 15.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.9.sp, color = wordColor)
                    Caption(state)
                }
                Box(Modifier.align(Alignment.Top).size(36.dp).clip(CircleShape).background(Beep.colors.page)
                    .border(1.5.dp, Beep.colors.softLine, CircleShape).clickable(role = Role.Button, onClick = ::leave)
                    .semantics { contentDescription = if (replying) "Cancel the reply and leave" else "Leave live mode" },
                    contentAlignment = Alignment.Center) {
                    Icon(Icons.Outlined.Close, null, Modifier.size(18.dp), tint = Beep.colors.ink)
                }
            }
            val soon = state.soon
            if (soon != null && state.stage == LiveStage.Listening && state.problem == null) SoonBar(soon, Modifier.padding(start = 58.dp))
            val problem = state.problem
            if (problem != null || hint != null || commands || replying) FlowRow(Modifier.fillMaxWidth().padding(start = 58.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp),
                itemVerticalAlignment = Alignment.CenterVertically) {
                if (hint != null) Text(hint, Modifier.weight(1f), fontSize = 12.sp, lineHeight = 16.sp, color = Beep.colors.muted)
                if (commands) FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    COMMANDS.forEach { (said, does) ->
                        Text(buildAnnotatedString {
                            withStyle(SpanStyle(color = Beep.colors.ink, fontWeight = FontWeight.Medium)) { append("“$said”") }
                            if (does.isNotEmpty()) withStyle(SpanStyle(color = Beep.colors.muted)) { append(" $does") }
                        }, fontSize = 11.sp, modifier = Modifier.clip(CircleShape).background(Beep.colors.page).padding(horizontal = 8.dp, vertical = 2.dp))
                    }
                }
                val pad = PaddingValues(horizontal = 14.dp, vertical = 6.dp)
                if (problem != null) Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    ChunkyButton(onClick = live::retry, contentPadding = pad) {
                        Text("Try again", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                    if (problem.microphone) ChunkyButton(onClick = {
                        context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                    }, color = Listen, contentColor = Color.White, contentPadding = pad) {
                        Text("Open Settings", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                }
                if (replying) ChunkyButton(onClick = live::cancelReply, contentPadding = pad) {
                    Text("Cancel", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
        // Toasts float just over the dock, on the chat, so the list does not jump.
        AnimatedVisibility(state.toast != null, Modifier.align(Alignment.TopCenter).offset(y = (-32).dp),
            enter = scaleIn(initialScale = 0.8f) + fadeIn(), exit = scaleOut(targetScale = 0.8f) + fadeOut()) {
            var last by remember { mutableStateOf("") }
            state.toast?.let { last = it }
            Text(last, fontSize = 13.sp, fontWeight = FontWeight.Medium, color = Beep.colors.page,
                modifier = Modifier.clip(CircleShape).background(Beep.colors.ink).padding(horizontal = 12.dp, vertical = 5.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite })
        }
    }
}

/**
 * Next to the orb: the line before in small type, then your words, the line Beep reads, its
 * step, or why live voice stopped.
 */
@Composable
private fun Caption(state: LiveState) {
    val problem = state.problem
    Column(Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }) {
        when {
            problem != null -> Placeholder(problem.text, maxLines = 4)
            state.stage == LiveStage.Connecting -> Placeholder("Getting ready…")
            state.ask != null || state.stage == LiveStage.Waiting -> Placeholder("Allow or deny in the chat")
            state.stage == LiveStage.Listening || state.stage == LiveStage.Holding -> when {
                state.words.isNotEmpty() -> Said(tail(state.words))
                state.line != null -> { Before(state.line); Placeholder("Go ahead") }
                else -> Placeholder("Go ahead, I’m listening")
            }
            state.stage == LiveStage.Thinking -> {
                state.said?.let { Before(it) }
                when {
                    state.reconnecting -> Step("Reconnecting…")
                    state.step != null -> Step(state.step)
                    else -> Placeholder("…")
                }
            }
            else -> {
                state.prev?.let { Before(it) }
                state.line?.let { Said(tail(it)) }
                state.step?.let { Step(it) }
            }
        }
    }
}

@Composable
private fun Said(text: String) {
    Text(text, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink, maxLines = 3,
        overflow = TextOverflow.Ellipsis)
}

@Composable
private fun Placeholder(text: String, maxLines: Int = 3) {
    Text(text, fontSize = 15.sp, lineHeight = 20.sp, color = Beep.colors.dim, maxLines = maxLines, overflow = TextOverflow.Ellipsis)
}

/** The line before, one line in small type. */
@Composable
private fun Before(text: String) {
    Text(text, fontSize = 12.sp, lineHeight = 16.sp, color = Beep.colors.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

@Composable
private fun Step(text: String) {
    Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        CircularProgressIndicator(Modifier.size(11.dp), color = Beep.colors.muted, strokeWidth = 2.dp)
        Text(text, fontSize = 13.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** How long until a pause sends, across the dock under the caption. */
@Composable
private fun SoonBar(soon: LiveSoon, modifier: Modifier = Modifier) {
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(soon) {
        while (isActive) { now = SystemClock.elapsedRealtime(); delay(16) }
    }
    val done = ((now - soon.from).toFloat() / soon.ms.coerceAtLeast(100)).coerceIn(0f, 1f)
    Box(modifier.fillMaxWidth().height(3.dp).clip(CircleShape).background(Beep.colors.softLine)
        .semantics { contentDescription = "Sending after the pause" }) {
        Box(Modifier.fillMaxWidth(done).height(3.dp).background(Listen))
    }
}

/**
 * The orb, which also takes the taps: a tap leaves while listening, stops the answer while
 * thinking and interrupts while speaking; holding talks until let go, and sliding away while
 * holding cancels. Mint while it listens, darker and breathing while it thinks, coral while it
 * speaks, grey while it waits; its rings grow with the voice.
 */
@Composable
private fun Orb(state: LiveState, waiting: Boolean, level: () -> Float, still: Boolean, live: LiveVoice, modifier: Modifier = Modifier) {
    var away by remember { mutableStateOf(false) }
    var pressed by remember { mutableStateOf(false) }
    val tones = when {
        state.stage == LiveStage.Speaking -> listOf(Color(0xFFFFD0C7), Speak, Color(0xFF5A231C), Speak)
        state.stage == LiveStage.Thinking -> listOf(Color(0xFF7FBFA6), Color(0xFF2E6F59), Color(0xFF0D2A20), Listen)
        state.problem != null || waiting || state.stage == LiveStage.Connecting ->
            listOf(Color(0xFF9FC2B2), Color(0xFF4F6E61), Color(0xFF1E2E27), Color(0xFF92A59A))
        else -> listOf(Color(0xFFA6EBCF), Listen, Color(0xFF0F3B2C), Listen)
    }
    val breathing = state.stage == LiveStage.Thinking && !still
    val breath by rememberInfiniteTransition(label = "orb-breath").animateFloat(1f, if (breathing) 1.06f else 1f,
        infiniteRepeatable(tween(1200), RepeatMode.Reverse), label = "orb-breath-scale")
    val enabled = state.problem == null
    Canvas(modifier.size(48.dp)
        .graphicsLayer { val s = breath * (if (pressed) 0.92f else 1f); scaleX = s; scaleY = s; alpha = if (away) 0.5f else 1f }
        .semantics {
            role = Role.Button
            contentDescription = when (state.stage) {
                LiveStage.Speaking -> "Stop speaking"
                LiveStage.Thinking -> "Skip the spoken answer"
                else -> "Tap to leave, hold to talk"
            }
            onClick { live.tap(); true }
        }
        .then(if (!enabled) Modifier else Modifier.pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                pressed = true
                var released = false
                withTimeoutOrNull(350) {
                    released = waitForUpOrCancellation() != null
                    true
                }.let { finished -> if (finished != null && !released) { pressed = false; return@awaitEachGesture } }
                if (released) { pressed = false; live.tap(); return@awaitEachGesture }
                live.holdStart()
                val limit = 90.dp.toPx()
                while (true) {
                    val event = awaitPointerEvent()
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    away = (change.position - down.position).getDistance() > limit
                    if (!change.pressed) break
                }
                if (away) live.holdCancel() else live.holdEnd()
                away = false
                pressed = false
            }
        })) {
        val v = if (still) 0f else level()
        val r = 16.dp.toPx()
        val center = Offset(size.width / 2, size.height / 2)
        val glow = tones[3]
        val ring = Stroke(1.5.dp.toPx())
        drawCircle(glow.copy(alpha = 0.07f + v * 0.2f), r * (1.18f + v * 0.55f), center, style = ring)
        drawCircle(glow.copy(alpha = 0.16f + v * 0.35f), r * (1.05f + v * 0.35f), center, style = ring)
        drawCircle(Brush.radialGradient(0f to tones[0], 0.46f to tones[1], 1f to tones[2],
            center = Offset(center.x - r * 0.28f, center.y - r * 0.4f), radius = r * 1.5f), r, center)
    }
}
