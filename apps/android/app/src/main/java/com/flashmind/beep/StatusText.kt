package com.flashmind.beep

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material3.Icon
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Statuses that only mean "waiting on Beep". They all read as "Thinking…". */
private val idleStatuses = setOf("Sending…", "Beep is thinking…", "Working...")

/**
 * The progress line under a pending reply: "Thinking…" while Beep is just busy, anything more
 * specific, like "Stopping…", as is. Either way a highlight sweeps across the letters.
 */
@Composable
fun StatusText(status: String) {
    val text = if (status in idleStatuses) "Thinking…" else status
    AnimatedContent(text, label = "status",
        transitionSpec = {
            (slideInVertically(tween(260)) { it / 2 } + fadeIn(tween(260))) togetherWith
                (slideOutVertically(tween(200)) { -it / 2 } + fadeOut(tween(200)))
        }) { shown -> Shimmer(shown) }
}

/** Text with a bright band that moves across it, letter by letter. */
@Composable
private fun Shimmer(text: String) {
    Text(text, color = Beep.colors.muted, fontSize = 13.sp, maxLines = 2)
}

/** Most reasoning kept for one reply, in characters. The oldest goes first. */
const val MAX_REASONING = 16_000

/** Reasoning `more` after `before`, a blank line apart. */
fun joinReasoning(before: String, more: String): String {
    val first = before.trim()
    val next = more.trim()
    val joined = when {
        next.isEmpty() -> first
        first.isEmpty() -> next
        else -> "$first\n\n$next"
    }
    return joined.takeLast(MAX_REASONING)
}

/**
 * The tail of the model's reasoning while it thinks: several lines in a soft box, the oldest fading
 * out at the top. It expands when reasoning starts and folds away once Beep moves on; the reasoning
 * then stays on the reply under [ReasoningRow].
 */
@Composable
fun ThinkingPreview(thinking: String, modifier: Modifier = Modifier) {
    val text = thinking.trim()
    AnimatedVisibility(text.isNotEmpty(), modifier,
        enter = fadeIn(tween(220)) + expandVertically(tween(220)),
        exit = fadeOut(tween(180)) + shrinkVertically(tween(220))) {
        // Keep the last text while folding away, so the box does not empty before it closes.
        var shown by remember { mutableStateOf(text) }
        if (text.isNotEmpty()) shown = text
        Box(Modifier.padding(top = 8.dp).fillMaxWidth()
            .clip(RoundedCornerShape(Beep.shapes.smallRadius))
            .background(Beep.colors.surface)
            .padding(horizontal = 12.dp, vertical = 8.dp)) {
            Box(Modifier.fillMaxWidth().heightIn(max = 140.dp).clipToBounds()
                .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                .drawWithContent {
                    drawContent()
                    drawRect(Brush.verticalGradient(0f to Color.Transparent, 0.2f to Color.Black), blendMode = BlendMode.DstIn)
                },
                contentAlignment = Alignment.BottomStart) {
                ThoughtsText(shown.takeLast(1500), Modifier.wrapContentHeight(Alignment.Bottom, unbounded = true))
            }
        }
    }
}

/** The reasoning behind a reply, folded into a "Thoughts" row that opens on tap. */
@Composable
fun ReasoningRow(reasoning: String, modifier: Modifier = Modifier) {
    var open by rememberSaveable { mutableStateOf(false) }
    val muted = Beep.colors.muted
    val turn by animateFloatAsState(if (open) 180f else 0f, label = "thoughts-chevron")
    Column(modifier) {
        Row(Modifier.clip(RoundedCornerShape(Beep.shapes.smallRadius))
            .clickable(onClickLabel = if (open) "Hide thoughts" else "Show thoughts") { open = !open }
            .heightIn(min = 32.dp).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Text("Thoughts", fontSize = 12.sp, fontWeight = FontWeight.Medium, color = muted)
            Icon(Icons.Outlined.KeyboardArrowDown, null,
                Modifier.padding(start = 2.dp).size(16.dp).graphicsLayer { rotationZ = turn }, tint = muted)
        }
        AnimatedVisibility(open, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            SelectionContainer {
                ThoughtsText(reasoning, Modifier.padding(bottom = 6.dp).fillMaxWidth()
                    .clip(RoundedCornerShape(Beep.shapes.smallRadius))
                    .background(Beep.colors.surface)
                    .padding(horizontal = 12.dp, vertical = 10.dp))
            }
        }
    }
}

/** Reasoning text with the same markdown as replies, kept small and muted. */
@Composable
private fun ThoughtsText(text: String, modifier: Modifier = Modifier) {
    val inline = inlineColors()
    MathText(remember(text, inline) { formattedMessage(text, inline, headingSize = 14.sp, codeSize = 12.sp) },
        color = Beep.colors.muted, fontSize = 13.sp, lineHeight = 19.sp, modifier = modifier)
}
