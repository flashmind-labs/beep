package com.flashmind.beep

import androidx.compose.foundation.Image
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource

/** Beep's activity in the floating chat. */
enum class BeepFace { Idle, Thinking, Happy, Sad, Sleepy }

@Composable
fun BeepHead(face: BeepFace, modifier: Modifier = Modifier) {
    Image(painterResource(R.drawable.ic_beep), "Beep: ${face.name.lowercase()}", modifier)
}
