package com.flashmind.beep

import com.flashmind.beep.ffi.identityName
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.activity.compose.rememberLauncherForActivityResult
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

private fun java.io.InputStream.readProfileBytes(): ByteArray {
    val buffer = ByteArray(65537)
    var count = 0
    while (count < buffer.size) {
        val read = read(buffer, count, buffer.size - count)
        if (read < 0) break
        count += read
    }
    return buffer.copyOf(count)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ConnectionScreen(initial: ConnectionSettings, accountName: String, canGoBack: Boolean, onBack: () -> Unit, onConnect: suspend (ConnectionSettings, ByteArray?) -> Unit) {
    var nodeId by remember { mutableStateOf(initial.nodeId) }
    /** An identity from an imported profile, used instead of this phone's. */
    var imported by remember { mutableStateOf<ByteArray?>(null) }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    val profilePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                val config = withContext(Dispatchers.IO) {
                    val raw = context.contentResolver.openInputStream(uri)?.use { it.readProfileBytes() } ?: error("Cannot open profile")
                    require(raw.size <= 65536) { "Profile is too large" }; Profile.fromJson(raw.toString(Charsets.UTF_8))
                }
                nodeId = config.settings.nodeId; imported = config.identity; error = ""
            } catch (e: Exception) { error = e.message.orEmpty() }
        }
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText(if (canGoBack) "Connection" else "Welcome to Beep"), style = displayStyle(24.sp)) },
            navigationIcon = { if (canGoBack) IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize().imePadding(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(66.dp))
                Spacer(Modifier.height(24.dp))
                Text(displayText("Your assistant.\nYour space."), style = displayStyle(38.sp, 42.sp))
                Spacer(Modifier.height(12.dp))
                Text("Connect to your Beep server to bring your conversations and memory along.", color = TextMuted, lineHeight = 23.sp)
            }
            item { ChunkyButton(onClick = { profilePicker.launch(arrayOf("application/json", "text/*", "application/octet-stream")) }, enabled = !saving, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Outlined.FileOpen, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Import connection profile", fontWeight = FontWeight.Medium)
            } }
            item { OutlinedTextField(nodeId, { nodeId = it }, label = { Text("Node ID") }, placeholder = { Text("hex public key") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving) }
            item {
                val account = imported?.let { "You will sign in as ${identityName(it)}, from the profile. This replaces the account on this phone." }
                    ?: "You sign in as $accountName. This phone's key is your account, so there is no password."
                Text(account, fontSize = 12.sp, color = TextMuted, lineHeight = 18.sp)
                Text("The node ID is printed when the server starts.", modifier = Modifier.padding(top = 8.dp), fontSize = 12.sp, color = TextMuted, lineHeight = 18.sp)
                if (canGoBack) Text("Changing the server or account clears this device’s chat cache. Server history is kept.", modifier = Modifier.padding(top = 8.dp), fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) item { Text(error, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
            item { ChunkyButton(onClick = {
                saving = true; error = ""
                scope.launch {
                    try { onConnect(ConnectionSettings(nodeId.trim()), imported) }
                    catch (e: Exception) { error = e.message ?: "Could not connect" }
                    finally { saving = false }
                }
            }, enabled = !saving && nodeId.isNotBlank(), modifier = Modifier.fillMaxWidth().height(56.dp),
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent) {
                Spacer(Modifier.weight(1f))
                if (saving) { CircularProgressIndicator(Modifier.size(18.dp), color = Beep.colors.onAccent, strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                Text(if (saving) "Connecting…" else "Connect to Beep", fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
                Spacer(Modifier.weight(1f))
            } }
        }
    }
}
