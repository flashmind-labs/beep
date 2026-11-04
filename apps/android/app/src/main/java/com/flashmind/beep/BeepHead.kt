package com.flashmind.beep

import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.widget.ImageView
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView

/** Beep's expression on the bubble. */
enum class BeepFace { Idle, Thinking, Happy, Sad, Sleepy }

/** The avatar plays the loop for its mood, round. Asleep, it holds still on the idle loop. */
@Composable
fun BeepHead(face: BeepFace, modifier: Modifier = Modifier, avatar: Avatar = Avatars.current) {
    val clips = avatar.clips
    val clip = when (face) {
        BeepFace.Thinking -> clips.thinking
        BeepFace.Happy -> clips.talking
        BeepFace.Idle, BeepFace.Sad, BeepFace.Sleepy -> clips.idle
    }
    val resources = LocalContext.current.resources
    val drawable = remember(clip) { ImageDecoder.decodeDrawable(ImageDecoder.createSource(resources, clip)) }
    DisposableEffect(drawable) { onDispose { (drawable as? AnimatedImageDrawable)?.stop() } }
    AndroidView({ ImageView(it).apply { scaleType = ImageView.ScaleType.CENTER_CROP } }, modifier.clip(CircleShape)) { view ->
        view.setImageDrawable(drawable)
        (drawable as? AnimatedImageDrawable)?.let { if (face != BeepFace.Thinking && face != BeepFace.Happy) it.stop() else it.start() }
    }
}
