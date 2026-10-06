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
import kotlinx.coroutines.launch

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

@Composable
internal fun OnboardingScreen(
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
            Box(Modifier.popIn("onboarding-logo").beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.user, pressableLook())) {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
            }
            Spacer(Modifier.height(28.dp))
            Text(displayText("Almost ready."), style = displayStyle(36.sp, 40.sp))
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
