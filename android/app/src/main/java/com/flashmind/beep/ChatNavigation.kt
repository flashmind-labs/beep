package com.flashmind.beep

import androidx.compose.ui.draw.alpha
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import kotlin.math.abs
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.LocalDate
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

// Short names for the current theme's colors.
private val TextPrimary: Color @Composable @ReadOnlyComposable get() = Beep.colors.ink
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

@Composable
internal fun ConnectionDot(connected: Boolean, status: String) {
    val reconnecting = !connected && status.contains("Reconnecting", ignoreCase = true)
    if (reconnecting) {
        val infiniteTransition = rememberInfiniteTransition(label = "reconnect-dot")
        val alpha by infiniteTransition.animateFloat(0.3f, 1f, infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "reconnect-alpha")
        Box(Modifier.size(7.dp).background(Beep.colors.warn.copy(alpha = alpha), CircleShape))
    } else {
        Box(Modifier.size(7.dp).background(if (connected) Beep.colors.ok else Beep.colors.error, CircleShape))
    }
}

/** Swipe left starts a new chat, swipe right goes back to the last one, or opens the chat list when there is none. */
@Composable
internal fun SwipeChats(canStartNew: Boolean, hasPrevious: Boolean, onNew: () -> Unit, onBack: () -> Unit,
                       modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    val density = LocalDensity.current
    val trigger = with(density) { 72.dp.toPx() }
    val limit = with(density) { 110.dp.toPx() }
    val latestNew by rememberUpdatedState(onNew)
    val latestBack by rememberUpdatedState(onBack)
    val latestCanStartNew by rememberUpdatedState(canStartNew)
    Box(modifier.pointerInput(Unit) {
        detectHorizontalDragGestures(
            onDragEnd = {
                val moved = offset.value
                if (moved <= -trigger && latestCanStartNew) latestNew() else if (moved >= trigger) latestBack()
                scope.launch { offset.animateTo(0f, spring(dampingRatio = Spring.DampingRatioMediumBouncy)) }
            },
            onDragCancel = { scope.launch { offset.animateTo(0f) } },
        ) { change, amount ->
            change.consume()
            // Damped so the chat lags the finger, and capped once past the trigger.
            val min = if (latestCanStartNew) -limit else -limit / 4
            scope.launch { offset.snapTo((offset.value + amount * 0.5f).coerceIn(min, limit)) }
        }
    }) {
        val progress = (abs(offset.value) / trigger).coerceIn(0f, 1f)
        if (offset.value < 0 && canStartNew)
            SwipeHint(Icons.Outlined.Add, "New chat", progress, Modifier.align(Alignment.CenterEnd).padding(end = 16.dp))
        if (offset.value > 0)
            SwipeHint(if (hasPrevious) Icons.AutoMirrored.Outlined.ArrowBack else Icons.Outlined.Menu,
                if (hasPrevious) "Previous" else "All chats", progress, Modifier.align(Alignment.CenterStart).padding(start = 16.dp))
        Box(Modifier.fillMaxSize().graphicsLayer { translationX = offset.value; alpha = 1f - progress * 0.25f }) { content() }
    }
}

@Composable
private fun SwipeHint(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, progress: Float, modifier: Modifier) {
    val ready = progress >= 1f
    Column(modifier.alpha(progress).graphicsLayer { scaleX = 0.8f + progress * 0.2f; scaleY = scaleX },
        horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(44.dp).background(if (ready) Beep.colors.accent else Beep.colors.surface, CircleShape)
            .border(2.dp, Beep.colors.line, CircleShape), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(22.dp), tint = if (ready) Beep.colors.onAccent else TextPrimary)
        }
        Spacer(Modifier.height(6.dp))
        Text(label, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)
    }
}

@Composable
internal fun DrawerTile(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tint: Color, modifier: Modifier = Modifier, onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, modifier = modifier.padding(end = 3.dp, bottom = 3.dp), color = tint,
        shape = RoundedCornerShape(Beep.shapes.smallRadius), contentPadding = PaddingValues(horizontal = 6.dp, vertical = 12.dp)) {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(icon, null, tint = TextPrimary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.height(6.dp))
            Text(label, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextPrimary, maxLines = 1,
                overflow = TextOverflow.Ellipsis)
        }
    }
}

/** A day as the chat shows it: "Today", "Yesterday", "3 Oct", or "3 Oct 2025" in another year. */
internal fun dayLabel(day: LocalDate, today: LocalDate = LocalDate.now()): String = when {
    day == today -> "Today"
    day == today.minusDays(1) -> "Yesterday"
    day.year == today.year -> day.format(DateTimeFormatter.ofPattern("d MMM"))
    else -> day.format(DateTimeFormatter.ofPattern("d MMM yyyy"))
}

/** When a chat last changed, for the chat list: "Today, 14:32", "Yesterday, 09:10", "3 Oct, 18:05". */
internal fun chatTime(millis: Long, now: ZonedDateTime = ZonedDateTime.now()): String {
    val at = Instant.ofEpochMilli(millis).atZone(now.zone)
    val time = at.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
    return "${dayLabel(at.toLocalDate(), now.toLocalDate())}, $time"
}

/**
 * The messages that start a new day, with that day, so the chat can mark it.
 * Only prompts carry a time; a reply belongs to the day of its prompt, and a
 * prompt the server has not saved yet to [now]'s day.
 */
internal fun dayBreaks(messages: List<ChatMessage>, now: ZonedDateTime = ZonedDateTime.now()): Map<String, LocalDate> {
    val breaks = mutableMapOf<String, LocalDate>()
    var current: LocalDate? = null
    for (m in messages) {
        val day = when {
            m.sentAt > 0 -> Instant.ofEpochMilli(m.sentAt).atZone(now.zone).toLocalDate()
            m.role == "assistant" -> current
            else -> now.toLocalDate()
        } ?: continue
        if (day != current) breaks[m.id] = day
        current = day
    }
    return breaks
}
