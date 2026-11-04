package com.flashmind.beep

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.runtime.saveable.rememberSaveable
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

@Composable
internal fun OnboardingScreen(
    repository: ChatRepository,
    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>?,
    onDone: () -> Unit
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var locationGranted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
        )
    }
    var notificationGranted by remember {
        mutableStateOf(
            Build.VERSION.SDK_INT < 33 ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        )
    }
    // Re-check permissions when returning from system dialogs
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                locationGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
                    ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
                notificationGranted = Build.VERSION.SDK_INT < 33 ||
                    ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    Scaffold(containerColor = Ink) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(28.dp),
            verticalArrangement = Arrangement.Center
        ) {
            Box(Modifier.popIn("onboarding-logo").beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.user)) {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
            }
            Spacer(Modifier.height(28.dp))
            Text(displayText("Almost ready."), style = displayStyle(24.sp))
            Spacer(Modifier.height(8.dp))
            Text("A couple of permissions to make Beep more useful. You can skip any of these.", color = TextMuted, lineHeight = 22.sp)
            Spacer(Modifier.height(32.dp))

            // Location permission
            PermissionCard(
                icon = Icons.Outlined.LocationOn,
                title = "Location",
                description = "Let Beep answer questions about nearby places, weather, and directions.",
                granted = locationGranted,
                onRequest = {
                    locationPermission?.launch(arrayOf(
                        Manifest.permission.ACCESS_FINE_LOCATION,
                        Manifest.permission.ACCESS_COARSE_LOCATION
                    ))
                }
            )
            Spacer(Modifier.height(12.dp))

            // Notification permission
            if (Build.VERSION.SDK_INT >= 33) {
                val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
                    notificationGranted = granted
                }
                PermissionCard(
                    icon = Icons.Outlined.Notifications,
                    title = "Notifications",
                    description = "Get notified when scheduled routines finish or when Beep has something for you.",
                    granted = notificationGranted,
                    onRequest = { notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS) }
                )
                Spacer(Modifier.height(12.dp))
            }

            KeyCard(repository)
            Spacer(Modifier.height(12.dp))
            ComputerCard(repository)
            Spacer(Modifier.height(20.dp))
            ChunkyButton(
                onClick = {
                    if (locationGranted) {
                        // Trigger location updates now that permission is granted
                        (context as? MainActivity)?.startLocationUpdates()
                    }
                    onDone()
                },
                modifier = Modifier.fillMaxWidth().height(56.dp),
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent
            ) {
                Text("Continue to Beep", fontWeight = FontWeight.SemiBold, fontSize = 16.sp, modifier = Modifier.weight(1f), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            }
            Spacer(Modifier.height(8.dp))
            TextButton(
                onClick = onDone,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Skip for now", color = TextMuted)
            }
        }
    }
}

/** Restores an account from the key file saved on another phone. The only place a key can be imported. */
@Composable
private fun KeyCard(repository: ChatRepository) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    val accountName by repository.accountName.collectAsState()
    var imported by rememberSaveable { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            busy = true; error = ""
            try {
                val identity = withContext(Dispatchers.IO) {
                    val raw = context.contentResolver.openInputStream(uri)?.use { it.readProfileBytes() } ?: error("Cannot open the file")
                    require(raw.size <= 65536) { "The file is too large" }
                    Profile.fromJson(raw.toString(Charsets.UTF_8)).identity ?: error("This file has no key")
                }
                withContext(Dispatchers.IO) { repository.importIdentity(identity) }
                imported = true
            } catch (e: Exception) { error = e.message.orEmpty() }
            busy = false
        }
    }
    BeepCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Outlined.Key, null, tint = if (imported) Beep.colors.ok else TextMuted, modifier = Modifier.size(24.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(if (imported) "Signed in as $accountName" else "Moving from another phone?", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                    Text(
                        if (imported) "From the key file. This replaces the new account on this phone."
                        else "You are $accountName. Import the key file you saved on your old phone to keep that account.",
                        fontSize = 12.sp, color = TextMuted, lineHeight = 17.sp
                    )
                }
            }
            Spacer(Modifier.height(12.dp))
            ChunkyButton(
                onClick = { if (!busy) picker.launch(arrayOf("application/json", "text/*", "application/octet-stream")) },
                color = Beep.colors.user, contentColor = Beep.colors.onUser,
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)
            ) {
                Text(if (busy) "Importing" else "Import key", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
            if (error.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Text(error, fontSize = 12.sp, color = Beep.colors.error)
            }
        }
    }
}

/** What the computer card says once a code went through: a title and a line under it. */
private class Done(val title: String, val text: String)

/** Joins a computer's account, or links a computer to this one, from the code it shows. */
@Composable
private fun ComputerCard(repository: ChatRepository) {
    var done by remember { mutableStateOf<Done?>(null) }
    val codes = rememberLinkCodes(repository,
        onLinked = { done = Done("Linked $it", "It sees the chats and memories on this phone.") },
        onJoined = { done = Done("On $it's account", "This phone now shows $it's chats and memories.") })
    LinkCodeSheets(codes)
    BeepCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                val finished = done
                Icon(if (finished != null) Icons.Outlined.CheckCircle else Icons.Outlined.Laptop, null,
                    tint = if (finished != null) Beep.colors.ok else TextMuted, modifier = Modifier.size(24.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(finished?.title ?: "Using Beep on a computer?", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                    Text(finished?.text ?: "Scan the code Beep shows on your computer to use the same chats and memories here.",
                        fontSize = 12.sp, color = TextMuted, lineHeight = 17.sp)
                }
            }
            if (done == null) {
                Spacer(Modifier.height(12.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    ChunkyButton(onClick = { if (!codes.busy) codes.scan() }, color = Beep.colors.user, contentColor = Beep.colors.onUser,
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)) {
                        Text(if (codes.busy) "Working" else "Scan code", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                    Spacer(Modifier.width(8.dp))
                    TextButton(onClick = { codes.typing = true }, enabled = !codes.busy) {
                        Text("Type code", fontSize = 13.sp, color = TextMuted)
                    }
                }
            }
        }
    }
}

/** Reads at most one byte past the size limit, so an oversized file is caught without loading all of it. */
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

@Composable
private fun PermissionCard(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    description: String,
    granted: Boolean,
    onRequest: () -> Unit
) {
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = if (granted) Beep.colors.ok else TextMuted, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                Text(description, fontSize = 12.sp, color = TextMuted, lineHeight = 17.sp)
            }
            Spacer(Modifier.width(12.dp))
            if (granted) {
                Icon(Icons.Outlined.CheckCircle, "Granted", tint = Beep.colors.ok, modifier = Modifier.size(24.dp))
            } else {
                ChunkyButton(onClick = onRequest, color = Beep.colors.user, contentColor = Beep.colors.onUser,
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)) {
                    Text("Allow", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}
