package com.flashmind.beep

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.animation.core.*
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.foundation.Image
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class ShareReceiveActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        if (intent?.action != Intent.ACTION_SEND) {
            finish()
            return
        }
        val sharedText = intent.getStringExtra(Intent.EXTRA_TEXT).orEmpty()
        val sharedImage = if (intent.type?.startsWith("image/") == true) {
            @Suppress("DEPRECATION")
            val uri = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                else intent.getParcelableExtra(Intent.EXTRA_STREAM)
            uri?.let { encodeImageUri(this, it) }
        } else null

        if (sharedText.isBlank() && sharedImage == null) {
            finish()
            return
        }

        val repository = (application as BeepApplication).repository

        setContent {
            BeepTheme {
                ShareScreen(
                    sharedText = sharedText,
                    sharedImage = sharedImage,
                    repository = repository,
                    onSent = {
                        Toast.makeText(this, "Sent to Beep", Toast.LENGTH_SHORT).show()
                        val main = Intent(this, MainActivity::class.java).apply {
                            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                        }
                        startActivity(main)
                        finish()
                    },
                    onDismiss = { finish() }
                )
            }
        }
    }
}

private val Suggestions = listOf("Summarize", "Translate", "Explain", "Save to memory")

@Composable
private fun ShareScreen(
    sharedText: String,
    sharedImage: String?,
    repository: ChatRepository,
    onSent: () -> Unit,
    onDismiss: () -> Unit
) {
    val ink = Beep.colors.page
    val surface = Beep.colors.surface
    val elevated = Beep.colors.raised
    val border = Beep.colors.softLine
    val moss = Beep.colors.brand
    val coral = Beep.colors.error
    val textPrimary = Beep.colors.ink
    val textMuted = Beep.colors.muted

    var instruction by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ink.copy(alpha = 0.6f))
            .clickable(
                indication = null,
                interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
            ) { onDismiss() }
    ) {
        Spacer(Modifier.weight(1f))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
                .background(surface)
                .clickable(enabled = false) {}
                .padding(20.dp)
        ) {
            // Drag handle
            Box(
                Modifier
                    .align(Alignment.CenterHorizontally)
                    .width(36.dp)
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(border)
            )
            Spacer(Modifier.height(16.dp))

            // Title
            Text("Share to Beep", color = textPrimary, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(16.dp))

            // Shared content preview
            Column(
                Modifier
                    .fillMaxWidth()
                    .heightIn(max = 180.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(elevated)
                    .border(1.dp, border, RoundedCornerShape(12.dp))
                    .padding(12.dp)
            ) {
                Text("Shared content", color = textMuted, fontSize = 11.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(4.dp))
                val bitmap = remember(sharedImage) { sharedImage?.let(::decodeImageBase64) }
                if (bitmap != null) {
                    Image(
                        bitmap.asImageBitmap(), contentDescription = "Shared image",
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.size(80.dp).clip(RoundedCornerShape(8.dp))
                    )
                }
                if (sharedText.isNotBlank()) {
                    if (bitmap != null) Spacer(Modifier.height(6.dp))
                    Text(
                        sharedText, color = textPrimary, fontSize = 14.sp,
                        maxLines = 6, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.verticalScroll(rememberScrollState())
                    )
                }
            }
            Spacer(Modifier.height(12.dp))

            // Suggestion chips
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Suggestions.forEach { label ->
                    val selected = instruction.equals(label, ignoreCase = true)
                    Surface(
                        shape = RoundedCornerShape(16.dp),
                        color = if (selected) moss.copy(alpha = 0.2f) else elevated,
                        border = BorderStroke(1.dp, if (selected) moss else border),
                        modifier = Modifier.clickable {
                            instruction = if (selected) "" else label
                        }
                    ) {
                        Text(
                            label, color = if (selected) moss else textPrimary,
                            fontSize = 12.sp, fontWeight = FontWeight.Medium,
                            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp)
                        )
                    }
                }
            }
            Spacer(Modifier.height(12.dp))

            // Instruction text field
            OutlinedTextField(
                value = instruction,
                onValueChange = { instruction = it },
                placeholder = { Text("Add an instruction...", color = textMuted, fontSize = 14.sp) },
                modifier = Modifier.fillMaxWidth(),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = moss,
                    unfocusedBorderColor = border,
                    cursorColor = moss,
                    focusedTextColor = textPrimary,
                    unfocusedTextColor = textPrimary
                ),
                shape = RoundedCornerShape(12.dp),
                singleLine = true
            )
            Spacer(Modifier.height(16.dp))

            // Send button
            Button(
                onClick = {
                    if (sending) return@Button
                    sending = true
                    val message = when {
                        sharedText.isBlank() -> instruction.trim().ifBlank { "What's in this image?" }
                        instruction.isBlank() -> sharedText
                        else -> "${instruction.trim()}:\n\n$sharedText"
                    }
                    repository.newChat()
                    repository.send(message, imageBase64 = sharedImage)
                    onSent()
                },
                enabled = !sending,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(
                    containerColor = moss,
                    contentColor = ink,
                    disabledContainerColor = moss.copy(alpha = 0.4f),
                    disabledContentColor = ink.copy(alpha = 0.5f)
                ),
                shape = RoundedCornerShape(12.dp),
                contentPadding = PaddingValues(vertical = 14.dp)
            ) {
                Text(
                    if (sending) "Sending..." else "Send to Beep",
                    fontSize = 15.sp, fontWeight = FontWeight.SemiBold
                )
            }

            // Bottom spacing for navigation bar
            Spacer(Modifier.height(8.dp))
        }
    }
}
