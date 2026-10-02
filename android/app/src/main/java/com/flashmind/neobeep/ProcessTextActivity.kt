package com.flashmind.neobeep

import android.content.ClipData
import android.content.ClipboardManager
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
import kotlinx.coroutines.launch

class ProcessTextActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val text = intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()
        val readonly = intent.getBooleanExtra("android.intent.extra.PROCESS_TEXT_READONLY", true)

        if (text.isNullOrBlank()) {
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
                TranslateScreen(
                    text = text,
                    readonly = readonly,
                    repository = repository,
                    onReplace = { translated ->
                        val result = Intent().apply {
                            putExtra(Intent.EXTRA_PROCESS_TEXT, translated)
                        }
                        setResult(RESULT_OK, result)
                        finish()
                    },
                    onCopy = { translated ->
                        val clipboard = getSystemService(ClipboardManager::class.java)
                        clipboard.setPrimaryClip(ClipData.newPlainText("Translation", translated))
                        Toast.makeText(this, "Copied", Toast.LENGTH_SHORT).show()
                    },
                    onDismiss = { finish() }
                )
            }
        }
    }
}

private val Languages = listOf(
    "Auto", "English", "Spanish", "French", "German", "Portuguese",
    "Italian", "Chinese", "Japanese", "Korean", "Arabic", "Russian",
    "Hindi", "Dutch", "Turkish"
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TranslateScreen(
    text: String,
    readonly: Boolean,
    repository: ChatRepository,
    onReplace: (String) -> Unit,
    onCopy: (String) -> Unit,
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

    var selectedLanguage by remember { mutableStateOf("Auto") }
    var dropdownExpanded by remember { mutableStateOf(false) }
    var translatedText by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    fun doTranslate() {
        loading = true
        error = ""
        translatedText = null
        scope.launch {
            try {
                val result = repository.translate(text, selectedLanguage)
                translatedText = result
            } catch (e: Exception) {
                error = e.message ?: "Translation failed"
            } finally {
                loading = false
            }
        }
    }

    // Auto-translate on first load
    LaunchedEffect(Unit) { doTranslate() }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ink.copy(alpha = 0.6f))
            .clickable(indication = null, interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }) { onDismiss() }
    ) {
        Spacer(Modifier.weight(1f))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
                .background(surface)
                .clickable(enabled = false) {} // block click-through
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
            Text("Translate with Beep", color = textPrimary, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(16.dp))

            // Original text
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(12.dp))
                    .background(elevated)
                    .border(1.dp, border, RoundedCornerShape(12.dp))
                    .padding(12.dp)
            ) {
                Text("Original", color = textMuted, fontSize = 11.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(4.dp))
                Text(
                    text, color = textPrimary, fontSize = 14.sp,
                    maxLines = 6, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.verticalScroll(rememberScrollState())
                )
            }
            Spacer(Modifier.height(12.dp))

            // Language selector row
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("To:", color = textMuted, fontSize = 13.sp)
                Spacer(Modifier.width(8.dp))
                ExposedDropdownMenuBox(
                    expanded = dropdownExpanded,
                    onExpandedChange = { dropdownExpanded = it }
                ) {
                    Box(
                        Modifier
                            .menuAnchor()
                            .clip(RoundedCornerShape(8.dp))
                            .background(elevated)
                            .border(1.dp, border, RoundedCornerShape(8.dp))
                            .padding(horizontal = 12.dp, vertical = 8.dp)
                    ) {
                        Text(
                            if (selectedLanguage == "Auto") "Auto-detect" else selectedLanguage,
                            color = textPrimary, fontSize = 13.sp
                        )
                    }
                    ExposedDropdownMenu(
                        expanded = dropdownExpanded,
                        onDismissRequest = { dropdownExpanded = false },
                        modifier = Modifier.background(elevated)
                    ) {
                        Languages.forEach { lang ->
                            DropdownMenuItem(
                                text = { Text(if (lang == "Auto") "Auto-detect" else lang, color = textPrimary, fontSize = 13.sp) },
                                onClick = {
                                    if (selectedLanguage != lang) {
                                        selectedLanguage = lang
                                        dropdownExpanded = false
                                        doTranslate()
                                    } else {
                                        dropdownExpanded = false
                                    }
                                },
                                modifier = Modifier.background(if (lang == selectedLanguage) border else Color.Transparent)
                            )
                        }
                    }
                }
                Spacer(Modifier.weight(1f))
                if (!loading && translatedText == null && error.isEmpty()) {
                    Button(
                        onClick = { doTranslate() },
                        colors = ButtonDefaults.buttonColors(containerColor = moss, contentColor = ink),
                        shape = RoundedCornerShape(8.dp),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)
                    ) {
                        Text("Translate", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
            Spacer(Modifier.height(16.dp))

            // Result area
            when {
                loading -> {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(12.dp))
                            .background(elevated)
                            .padding(20.dp),
                        contentAlignment = Alignment.Center
                    ) {
                        TranslatePulsingDots(moss)
                    }
                }
                error.isNotEmpty() -> {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(12.dp))
                            .background(elevated)
                            .border(1.dp, coral.copy(alpha = 0.3f), RoundedCornerShape(12.dp))
                            .padding(12.dp)
                    ) {
                        Text(error, color = coral, fontSize = 13.sp)
                        Spacer(Modifier.height(8.dp))
                        Row {
                            TextButton(onClick = { doTranslate() }) {
                                Text("Retry", color = moss, fontSize = 13.sp)
                            }
                        }
                    }
                }
                translatedText != null -> {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(12.dp))
                            .background(elevated)
                            .border(1.dp, moss.copy(alpha = 0.3f), RoundedCornerShape(12.dp))
                            .padding(12.dp)
                    ) {
                        Text("Translation", color = textMuted, fontSize = 11.sp, fontWeight = FontWeight.Medium)
                        Spacer(Modifier.height(4.dp))
                        Text(
                            translatedText!!, color = textPrimary, fontSize = 14.sp,
                            modifier = Modifier.verticalScroll(rememberScrollState())
                        )
                    }
                    Spacer(Modifier.height(16.dp))

                    // Action buttons
                    Row(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)
                    ) {
                        OutlinedButton(
                            onClick = { onCopy(translatedText!!) },
                            border = BorderStroke(1.dp, border),
                            shape = RoundedCornerShape(8.dp),
                            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)
                        ) {
                            Text("Copy", color = textPrimary, fontSize = 13.sp)
                        }
                        if (!readonly) {
                            Button(
                                onClick = { onReplace(translatedText!!) },
                                colors = ButtonDefaults.buttonColors(containerColor = moss, contentColor = ink),
                                shape = RoundedCornerShape(8.dp),
                                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)
                            ) {
                                Text("Replace", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    }
                }
            }

            // Bottom spacing for navigation bar
            Spacer(Modifier.height(8.dp))
        }
    }
}

@Composable
private fun TranslatePulsingDots(color: Color) {
    val infiniteTransition = rememberInfiniteTransition(label = "translate-dots")
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        repeat(3) { index ->
            val alpha by infiniteTransition.animateFloat(
                initialValue = 0.25f, targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = tween(450, easing = EaseInOutCubic, delayMillis = index * 160),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-alpha-$index"
            )
            val scale by infiniteTransition.animateFloat(
                initialValue = 0.7f, targetValue = 1.15f,
                animationSpec = infiniteRepeatable(
                    animation = tween(450, easing = EaseInOutCubic, delayMillis = index * 160),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-scale-$index"
            )
            Box(
                Modifier
                    .size(6.dp)
                    .graphicsLayer { scaleX = scale; scaleY = scale }
                    .background(color.copy(alpha = alpha), CircleShape)
            )
        }
    }
}
