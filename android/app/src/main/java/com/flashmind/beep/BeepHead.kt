package com.flashmind.beep

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.res.vectorResource
import androidx.annotation.DrawableRes
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import kotlin.random.Random

/** Beep's expression on the bubble. */
enum class BeepFace { Idle, Thinking, Happy, Sad, Sleepy }

private val InkLine = Color(0xFF10201A)
private val Blush = Color(0xFFF07A6A)

/**
 * Where an avatar's face sits, in its icon's 108-unit viewport. [unit] scales the features from
 * the 4-unit eye radius they were drawn for. [center] is the middle of the drawing, which is
 * cropped to [span] units around it.
 */
private class FaceSpot(
    @DrawableRes val body: Int, val center: Offset, val span: Float, val leftEye: Offset, val rightEye: Offset,
    val mouth: Offset, val unit: Float, val cheeks: List<Offset>, val cheekRadius: Float,
)

private fun Avatar.faceSpot() = when (this) {
    Avatar.Beep -> FaceSpot(R.drawable.ic_bubble_beep, Offset(55f, 56f), 56f, Offset(44f, 55f), Offset(55.5f, 55f),
        Offset(50.5f, 62f), 0.8f, listOf(Offset(59.5f, 62f)), 2.1f)
    Avatar.Bubble -> FaceSpot(R.drawable.ic_bubble_bubble, Offset(56f, 54f), 58f, Offset(47f, 50f), Offset(63f, 50f),
        Offset(56f, 58f), 0.875f, listOf(Offset(42.5f, 57.5f), Offset(69.5f, 57.5f)), 2.4f)
    Avatar.Spark -> FaceSpot(R.drawable.ic_bubble_spark, Offset(55f, 54f), 58f, Offset(46.5f, 60f), Offset(61f, 60f),
        Offset(54f, 67f), 0.8f, emptyList(), 0f)
    Avatar.Ghost -> FaceSpot(R.drawable.ic_bubble_ghost, Offset(55.5f, 56f), 58f, Offset(46f, 51f), Offset(61f, 51f),
        Offset(52f, 60f), 0.8f, listOf(Offset(65f, 59f)), 2.3f)
}

/** Where a face puts its features, in units of a 4-unit eye radius, relative to the avatar's [FaceSpot]. */
private data class Features(
    val open: Float = 1f, val happyEyes: Float = 0f, val smile: Float = 1f, val mouthWidth: Float = 7f,
    val mouthShift: Float = 0f, val worry: Float = 0f, val brow: Float = 0f, val blush: Float = 1f,
    val lookX: Float = 0f, val lookY: Float = 0f,
)

private fun BeepFace.features() = when (this) {
    BeepFace.Idle -> Features()
    BeepFace.Thinking -> Features(smile = 0f, mouthWidth = 3.5f, mouthShift = 2f, brow = 1f, blush = 0f, lookX = 1.5f, lookY = -2.5f)
    BeepFace.Happy -> Features(happyEyes = 1f, smile = 1.3f, mouthWidth = 8f)
    BeepFace.Sad -> Features(smile = -1f, mouthWidth = 6f, worry = 1f, blush = 0f, lookY = 1.5f)
    BeepFace.Sleepy -> Features(open = 0f, smile = 0f, mouthWidth = 2f, blush = 0f)
}

/**
 * Beep's robot head with a live face: it blinks, glances around, ponders while thinking, hops when
 * happy and breathes when asleep. [look] points the eyes somewhere, for example along a drag, in
 * the range -1..1 on each axis.
 */
@Composable
fun BeepHead(face: BeepFace, look: State<Offset>, modifier: Modifier = Modifier, avatar: Avatar = Avatars.current) {
    val target = face.features()
    val spot = avatar.faceSpot()
    val soft = spring<Float>(dampingRatio = 0.6f, stiffness = 300f)
    val open by animateFloatAsState(target.open, soft, label = "open")
    val happyEyes by animateFloatAsState(target.happyEyes, soft, label = "happyEyes")
    val smile by animateFloatAsState(target.smile, soft, label = "smile")
    val mouthWidth by animateFloatAsState(target.mouthWidth, soft, label = "mouthWidth")
    val mouthShift by animateFloatAsState(target.mouthShift, soft, label = "mouthShift")
    val worry by animateFloatAsState(target.worry, tween(250), label = "worry")
    val brow by animateFloatAsState(target.brow, tween(250), label = "brow")
    val blush by animateFloatAsState(target.blush, tween(400), label = "blush")

    // Blinks, the odd wink, and eyes that wander on their own.
    val leftLid = remember { Animatable(1f) }
    val rightLid = remember { Animatable(1f) }
    val glanceX = remember { Animatable(0f) }
    val glanceY = remember { Animatable(0f) }
    LaunchedEffect(face) {
        // A face change can cut a blink short.
        leftLid.snapTo(1f); rightLid.snapTo(1f)
        if (face == BeepFace.Sleepy || face == BeepFace.Happy) return@LaunchedEffect
        var blinks = 0
        while (true) {
            delay(Random.nextLong(2200, 5200))
            val wink = face == BeepFace.Idle && ++blinks % 4 == 0
            launch { rightLid.animateTo(0f, tween(70)); delay(if (wink) 450 else 40); rightLid.animateTo(1f, tween(110)) }
            if (!wink) { leftLid.animateTo(0f, tween(70)); delay(40); leftLid.animateTo(1f, tween(110)) }
        }
    }
    LaunchedEffect(face) {
        while (true) {
            val (x, y) = when (face) {
                // Pondering: look up to one side, then the other.
                BeepFace.Thinking -> listOf(0f, -3f).random() to Random.nextFloat() * 0.6f
                BeepFace.Idle -> Random.nextFloat() * 3f - 1.5f to Random.nextFloat() * 2f - 1f
                else -> 0f to 0f
            }
            launch { glanceX.animateTo(x, spring(dampingRatio = 0.7f, stiffness = 120f)) }
            glanceY.animateTo(y, spring(dampingRatio = 0.7f, stiffness = 120f))
            delay(if (face == BeepFace.Thinking) Random.nextLong(700, 1400) else Random.nextLong(1800, 4000))
        }
    }

    val motion = rememberInfiniteTransition(label = "motion")
    val cycle by motion.animateFloat(0f, 1f, infiniteRepeatable(tween(1600, easing = LinearEasing), RepeatMode.Restart), label = "cycle")
    val wave = sin(cycle * 2 * PI).toFloat()
    val hop = if (face == BeepFace.Happy) -abs(wave) * 0.08f else 0f
    val breath = if (face == BeepFace.Sleepy) 1f + wave * 0.025f else 1f

    val body = rememberVectorPainter(ImageVector.vectorResource(spot.body))
    Box(modifier.graphicsLayer {
        translationY = hop * size.height
        scaleX = breath; scaleY = breath
    }) {
        Canvas(Modifier.fillMaxSize()) {
            val scale = size.minDimension / spot.span
            withTransform({
                scale(scale, scale, pivot = Offset.Zero)
                translate(spot.span / 2 - spot.center.x, spot.span / 2 - spot.center.y)
            }) {
                with(body) { draw(Size(108f, 108f)) }
                val u = spot.unit
                val shift = Offset((target.lookX + glanceX.value + look.value.x * 2.5f) * u, (target.lookY + glanceY.value + look.value.y * 2f) * u)
                eye(spot.leftEye + shift, open * leftLid.value, happyEyes, u)
                eye(spot.rightEye + shift, open * rightLid.value, happyEyes, u)
                brows(spot, worry, brow)
                if (blush > 0.01f) spot.cheeks.forEach { drawCircle(Blush.copy(alpha = blush), spot.cheekRadius, it) }
                mouth(spot.mouth + Offset(mouthShift * u, 0f), smile, mouthWidth, u)
            }
        }
    }
}

private fun DrawScope.line(path: Path, alpha: Float = 1f) =
    drawPath(path, InkLine.copy(alpha = alpha.coerceIn(0f, 1f)), style = Stroke(3f, cap = StrokeCap.Round))

/** An eye that is a dot when open, a curve when shut, and an upturned arc when happy. */
private fun DrawScope.eye(at: Offset, open: Float, happy: Float, u: Float) {
    val (x, y) = at
    val height = 8f * u * open * (1f - happy)
    if (height > 1.5f) drawOval(InkLine, Offset(x - 4f * u, y - height / 2), Size(8f * u, height))
    val shut = (1f - open) * (1f - happy)
    if (shut > 0.05f) line(Path().apply { moveTo(x - 4f * u, y); quadraticTo(x, y + 3f * u, x + 4f * u, y) }, shut)
    if (happy > 0.05f) line(Path().apply { moveTo(x - 4f * u, y + u); quadraticTo(x, y - 4f * u, x + 4f * u, y + u) }, happy)
}

private fun DrawScope.brows(spot: FaceSpot, worry: Float, raised: Float) {
    val u = spot.unit
    val (lx, ly) = spot.leftEye
    val (rx, ry) = spot.rightEye
    if (worry > 0.01f) {
        line(Path().apply { moveTo(lx - 3f * u, ly - 3f * u); lineTo(lx + 3f * u, ly - 5.5f * u) }, worry)
        line(Path().apply { moveTo(rx + 3f * u, ry - 3f * u); lineTo(rx - 3f * u, ry - 5.5f * u) }, worry)
    }
    if (raised > 0.01f) line(Path().apply { moveTo(rx - 2f * u, ry - 8.5f * u); quadraticTo(rx + 1.5f * u, ry - 10.5f * u, rx + 5f * u, ry - 8.5f * u) }, raised)
}

/** A smile when [curve] is positive, a frown when negative, a flat line at zero. */
private fun DrawScope.mouth(at: Offset, curve: Float, halfWidth: Float, u: Float) {
    val y = at.y + maxOf(0f, -curve) * 3f * u
    line(Path().apply { moveTo(at.x - halfWidth * u, y); quadraticTo(at.x, y + curve * 5.5f * u, at.x + halfWidth * u, y) })
}
