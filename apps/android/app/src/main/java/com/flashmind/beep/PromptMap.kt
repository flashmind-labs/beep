package com.flashmind.beep

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.abs
import kotlin.math.roundToInt

/** Room above and below the ticks, so the first and last stay clear of the list's edges. */
private val MAP_INSET = 14.dp

/**
 * The desktop's prompt map for the phone: a tick per prompt on the chat's right edge once it scrolls.
 * Touch has no hover, so pressing the strip shows the nearest prompt, dragging moves between them, and
 * lifting the finger jumps there. A tick sits at its message's place in the chat by index, since the
 * list has no heights for rows it has not laid out.
 */
@Composable
internal fun PromptMap(messages: List<ChatMessage>, list: LazyListState, modifier: Modifier, onJump: (Int) -> Unit) {
    // Message indexes are the list's item indexes: the messages are its first items.
    val prompts = remember(messages) { messages.indices.filter { messages[it].role == "user" } }
    val scrolls by remember { derivedStateOf { list.canScrollForward || list.canScrollBackward } }
    var held by remember { mutableStateOf<Int?>(null) }
    // The prompt whose turn is at the top of the view.
    val current by remember(prompts) {
        derivedStateOf { prompts.indexOfLast { it <= list.firstVisibleItemIndex }.coerceAtLeast(0) }
    }
    val haptics = LocalHapticFeedback.current
    AnimatedVisibility(prompts.size >= 2 && scrolls, modifier, enter = fadeIn(), exit = fadeOut()) {
        BoxWithConstraints {
            val density = LocalDensity.current
            val track = maxHeight - MAP_INSET * 2
            fun at(slot: Int): Dp = MAP_INSET + track * (prompts[slot].toFloat() / messages.size)
            fun nearest(y: Float): Int = with(density) { prompts.indices.minBy { abs(at(it).toPx() - y) } }
            Box(Modifier.align(Alignment.TopEnd).fillMaxHeight().width(24.dp)
                .then(if (held != null) Modifier.background(Beep.colors.softLine.copy(alpha = 0.5f)) else Modifier)
                .semantics { contentDescription = "Prompts in this chat" }
                .pointerInput(prompts, messages.size) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        down.consume()
                        held = nearest(down.position.y)
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            change.consume()
                            if (!change.pressed) break
                            val next = nearest(change.position.y)
                            if (next != held) {
                                held = next
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            }
                        }
                        held?.let { onJump(prompts[it]) }
                        held = null
                    }
                }) {
                prompts.indices.forEach { slot ->
                    val lit = slot == held
                    val width by animateDpAsState(if (lit) 16.dp else if (slot == current) 14.dp else 10.dp, label = "tick")
                    Box(Modifier.align(Alignment.TopEnd).offset(y = at(slot) - 1.5.dp).padding(end = 4.dp)
                        .size(width, 3.dp)
                        .background(when { lit -> Beep.colors.brand; slot == current -> Beep.colors.ink; else -> Beep.colors.dim },
                            RoundedCornerShape(2.dp))
                        .semantics { contentDescription = messages[prompts[slot]].text; onClick { onJump(prompts[slot]); true } })
                }
            }
            held?.let { slot ->
                val prompt = messages[prompts[slot]]
                val shape = RoundedCornerShape(12.dp)
                Column(Modifier.align(Alignment.TopEnd).padding(end = 28.dp).widthIn(max = 250.dp)
                    // Centred on the tick, but kept inside the list.
                    .layout { measurable, constraints ->
                        val placeable = measurable.measure(constraints)
                        val y = (at(slot).toPx() - placeable.height / 2f).roundToInt()
                            .coerceIn(0, (constraints.maxHeight - placeable.height).coerceAtLeast(0))
                        layout(placeable.width, placeable.height) { placeable.place(0, y) }
                    }
                    .beepFrame(shape, Beep.colors.surface).padding(horizontal = 12.dp, vertical = 8.dp)) {
                    Text("Prompt ${slot + 1} of ${prompts.size}", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
                    Text(prompt.text, fontSize = 13.sp, lineHeight = 18.sp, color = Beep.colors.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}
