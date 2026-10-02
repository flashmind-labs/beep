package com.flashmind.neobeep

import android.content.Intent
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
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class ShareReceiveActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val sharedText = when (intent?.action) {
            Intent.ACTION_SEND -> {
                when {
                    intent.type?.startsWith("text/") == true ->
                        intent.getStringExtra(Intent.EXTRA_TEXT)
                    intent.type?.startsWith("image/") == true ->
                        "[Image shared -- image processing not yet supported]"
                    else -> intent.getStringExtra(Intent.EXTRA_TEXT)
                }
            }
            else -> null
        }

        if (sharedText.isNullOrBlank()) {
            finish()
            return
        }

        val repository = (application as BeepApplication).repository

        setContent {
            MaterialTheme(colorScheme = darkColorScheme(
                primary = Color(0xFF3B9B7A),
                onPrimary = Color(0xFF0A0E0C),
                background = Color(0xFF0A0E0C),
                surface = Color(0xFF121916),
                onSurface = Color(0xFFE4EAE2),
                surfaceVariant = Color(0xFF1A231E),
                onSurfaceVariant = Color(0xFF7A8A80),
                outline = Color(0xFF243028),
                error = Color(0xFFF07A6A)
            )) {
                ShareScreen(
                    sharedText = sharedText,
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
    repository: ChatRepository,
    onSent: () -> Unit,
    onDismiss: () -> Unit
) {
    val ink = Color(0xFF0A0E0C)
    val surface = Color(0xFF121916)
    val elevated = Color(0xFF1A231E)
    val border = Color(0xFF243028)
    val moss = Color(0xFF3B9B7A)
    val coral = Color(0xFFF07A6A)
    val textPrimary = Color(0xFFE4EAE2)
    val textMuted = Color(0xFF7A8A80)

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
                    .heightIn(max = 120.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(elevated)
                    .border(1.dp, border, RoundedCornerShape(12.dp))
                    .padding(12.dp)
            ) {
                Text("Shared content", color = textMuted, fontSize = 11.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(4.dp))
                Text(
                    sharedText, color = textPrimary, fontSize = 14.sp,
                    maxLines = 6, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.verticalScroll(rememberScrollState())
                )
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
                    val message = buildString {
                        if (instruction.isNotBlank()) {
                            append(instruction.trim())
                            append(":\n\n")
                        }
                        append(sharedText)
                    }
                    repository.newChat()
                    repository.send(message)
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
