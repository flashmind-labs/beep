package com.flashmind.beep

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.BubbleChart
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Apps
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.outlined.Key
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Everything that is not a conversation or one of the drawer tiles. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(status: String, repository: ChatRepository, onBack: () -> Unit, onBubbleSettings: () -> Unit, onConnection: () -> Unit) {
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Settings"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp)) {
            ThemePicker()
            AvatarPicker()
            BubblePicker(onBubbleSettings)
            ScreenContextSettings()
            Column {
                HorizontalDivider(color = Beep.colors.softLine)
                AccountRow(repository)
                HorizontalDivider(color = Beep.colors.softLine)
                SettingsRow(Icons.Outlined.Link, "Connection", status, onConnection)
                HorizontalDivider(color = Beep.colors.softLine)
            }
        }
    }
}

/**
 * This phone's account name and whether its key is safe. Without the Google
 * backup the user can save the key as a profile file, and import that file
 * on the connection screen of a new phone.
 */
@Composable
private fun AccountRow(repository: ChatRepository) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val name by repository.accountName.collectAsState()
    val backedUp by repository.backedUp.collectAsState()
    var saved by remember { mutableStateOf(false) }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) scope.launch {
            saved = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(repository.exportProfile().toByteArray()) } != null
                }.getOrDefault(false)
            }
            Toast.makeText(context, if (saved) "Key saved. Keep the file private." else "Could not save the key", Toast.LENGTH_SHORT).show()
        }
    }
    val detail = when {
        backedUp == true -> "Key backed up to your Google account"
        saved -> "Key saved to a file"
        else -> "Key not backed up · tap to save it to a file"
    }
    SettingsRow(Icons.Outlined.Key, name.ifEmpty { "Account" }, detail) { export.launch("beep-$name.json") }
}

/** Which floating chat the float button opens, and whether the overlay may draw yet. */
@Composable
private fun BubblePicker(onBubbleSettings: () -> Unit) {
    val context = LocalContext.current
    val kind = Bubble.kind.value ?: Bubble.kind(context)
    var allowed by remember { mutableStateOf(Bubble.allowed(context)) }
    LifecycleResumeEffect(Unit) { allowed = Bubble.allowed(context); onPauseOrDispose {} }
    Column {
        Text("Floating chat", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            BubbleKind.entries.forEach { option ->
                val selected = option == kind
                val shape = RoundedCornerShape(14.dp)
                Row(Modifier.fillMaxWidth()
                    .border(if (selected) 2.dp else 1.dp, if (selected) Beep.colors.ink else Beep.colors.softLine, shape)
                    .clip(shape)
                    .clickable { Bubble.pick(context, option) }
                    .padding(horizontal = 12.dp, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(option.label, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                        Text(option.hint, fontSize = 12.sp, color = Beep.colors.muted)
                    }
                    if (selected) Icon(Icons.Outlined.Check, "Selected", Modifier.size(16.dp), tint = Beep.colors.ink)
                }
            }
        }
        if (kind == BubbleKind.Overlay && !allowed) {
            Spacer(Modifier.height(4.dp))
            SettingsRow(Icons.Outlined.BubbleChart, "Allow display over other apps",
                "Until then the float button uses the Android bubble", onBubbleSettings)
        }
    }
}

/** Permissions that tell the assistant what the user is looking at behind the bubble. */
@Composable
private fun ScreenContextSettings() {
    val context = LocalContext.current
    var usage by remember { mutableStateOf(ScreenContext.usageAllowed(context)) }
    LifecycleResumeEffect(Unit) {
        usage = ScreenContext.usageAllowed(context)
        onPauseOrDispose {}
    }
    Column {
        Text("Screen context", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        SettingsRow(Icons.Outlined.Apps, "Name the open app",
            if (usage) "On: the bubble tells Beep which app you are in" else "Off: allow usage access") {
            ScreenContext.usageSettings(context)
        }
    }
}

@Composable
private fun SettingsRow(icon: ImageVector, label: String, detail: String?, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, Modifier.size(20.dp), tint = Beep.colors.muted)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(label, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
            if (detail != null) Text(detail, fontSize = 12.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = Beep.colors.muted)
    }
}
