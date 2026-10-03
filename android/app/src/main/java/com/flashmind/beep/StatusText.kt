package com.flashmind.beep

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlin.math.abs

/** Statuses that only mean "waiting on Beep". They become a rotating verb instead of literal text. */
private val idleStatuses = setOf("Sending…", "Beep is thinking…", "Working...")

private val verbs = listOf(
    "Thinking", "Pondering", "Mulling it over", "Noodling", "Brewing", "Tinkering", "Puzzling",
    "Musing", "Percolating", "Untangling", "Cooking", "Conjuring", "Scheming", "Wrangling",
    "Ruminating", "Doodling", "Hatching", "Simmering", "Crunching", "Figuring",
)

/**
 * The progress line under a pending reply. While Beep is just busy it cycles through playful
 * verbs; anything more specific, like "Stopping…", is shown as is. Either way a highlight sweeps
 * across the letters.
 */
@Composable
fun StatusText(status: String) {
    val idle = status in idleStatuses
    var index by remember { mutableIntStateOf(verbs.indices.random()) }
    LaunchedEffect(idle) {
        while (idle) {
            delay(2600)
            index = (index + 1 + (0 until verbs.size - 1).random()) % verbs.size
        }
    }
    val text = if (idle) verbs[index] + "…" else status
    AnimatedContent(text, label = "status",
        transitionSpec = {
            (slideInVertically(tween(260)) { it / 2 } + fadeIn(tween(260))) togetherWith
                (slideOutVertically(tween(200)) { -it / 2 } + fadeOut(tween(200)))
        }) { shown -> Shimmer(shown) }
}

/** Text with a bright band that moves across it, letter by letter. */
@Composable
private fun Shimmer(text: String) {
    val base = Beep.colors.muted
    val bright = Beep.colors.ink
    if (reduceMotion()) {
        Text(text, color = base, fontSize = 13.sp, maxLines = 2)
        return
    }
    val sweep by rememberInfiniteTransition(label = "shimmer").animateFloat(
        initialValue = -4f, targetValue = text.length + 4f,
        animationSpec = infiniteRepeatable(tween(90 * (text.length + 8), easing = LinearEasing), RepeatMode.Restart),
        label = "shimmer-sweep",
    )
    Text(buildAnnotatedString {
        text.forEachIndexed { i, c ->
            val glow = (1f - abs(i - sweep) / 3f).coerceIn(0f, 1f)
            withStyle(SpanStyle(color = lerp(base, bright, glow))) { append(c) }
        }
    }, fontSize = 13.sp, maxLines = 2)
}

/**
 * The tail of the model's reasoning while it thinks: a few lines in a soft box, the oldest fading
 * out at the top. It expands when reasoning starts and folds away once Beep moves on.
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
        val muted = Beep.colors.muted
        Box(Modifier.padding(top = 8.dp).fillMaxWidth()
            .clip(RoundedCornerShape(Beep.shapes.smallRadius))
            .background(Beep.colors.surface)
            .padding(horizontal = 12.dp, vertical = 8.dp)) {
            Box(Modifier.fillMaxWidth().heightIn(max = 68.dp).clipToBounds()
                .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                .drawWithContent {
                    drawContent()
                    drawRect(Brush.verticalGradient(0f to Color.Transparent, 0.45f to Color.Black), blendMode = BlendMode.DstIn)
                },
                contentAlignment = Alignment.BottomStart) {
                Text(shown.takeLast(600), color = muted, fontSize = 12.sp, lineHeight = 17.sp,
                    fontStyle = FontStyle.Italic, modifier = Modifier.wrapContentHeight(Alignment.Bottom, unbounded = true))
            }
        }
    }
}
