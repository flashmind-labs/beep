package com.flashmind.beep

import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.text.format.DateUtils
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Laptop
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.LinkPreview
import com.flashmind.beep.ffi.linkPreview
import com.flashmind.beep.proto.DeviceKind
import com.flashmind.beep.proto.LinkedDevice
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A link code the phone read, waiting for the user to confirm it. */
private class Scanned(val uri: String, val preview: LinkPreview)

/**
 * The phone and the computers linked to its account. A computer shows a code
 * in its Link to phone window; scanning it here asks first, then links it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkedDevicesScreen(repository: ChatRepository, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var devices by remember { mutableStateOf<List<LinkedDevice>?>(null) }
    var removing by remember { mutableStateOf<LinkedDevice?>(null) }
    var scanned by remember { mutableStateOf<Scanned?>(null) }

    fun refresh() = scope.launch {
        runCatching { withContext(Dispatchers.IO) { repository.listDevices() } }
            .onSuccess { devices = it }
            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
    }
    LaunchedEffect(Unit) { refresh() }

    fun read(text: String?) {
        val uri = text?.trim().orEmpty()
        runCatching { linkPreview(uri) }
            .onSuccess { scanned = Scanned(uri, it) }
            .onFailure { Toast.makeText(context, "That is not a Beep link code", Toast.LENGTH_SHORT).show() }
    }
    fun scan() {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { read(it.rawValue) }
            .addOnFailureListener {
                // No scanner module, as on most emulators: take a copied code instead.
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                read(clipboard.primaryClip?.getItemAt(0)?.text?.toString())
            }
    }

    removing?.let { device ->
        AlertDialog(
            onDismissRequest = { removing = null },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.removeDevice(device.id) } }
                            .onSuccess { Toast.makeText(context, "${device.name} removed", Toast.LENGTH_SHORT).show(); refresh() }
                            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                    }
                }) { Text("Remove", color = Beep.colors.error) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
            title = { Text("Remove ${device.name}?") },
            text = { Text("It is signed out at once and can no longer see your chats or memories.") },
            containerColor = Beep.colors.surface)
    }

    scanned?.let { offer ->
        ModalBottomSheet(onDismissRequest = { scanned = null }, containerColor = Beep.colors.surface) {
            LinkSheet(offer.preview, onCancel = { scanned = null }, onLink = {
                scanned = null
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { repository.linkDevice(offer.uri) } }
                        .onSuccess { Toast.makeText(context, "${it.name} linked", Toast.LENGTH_SHORT).show(); refresh() }
                        .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_LONG).show() }
                }
            })
        }
    }

    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Linked devices"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().padding(horizontal = 20.dp, vertical = 8.dp)) {
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                Text("Devices on this account", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
                Spacer(Modifier.height(4.dp))
                devices.orEmpty().forEach { device -> DeviceRow(device) { removing = device } }
                HorizontalDivider(Modifier.padding(vertical = 10.dp), color = Beep.colors.softLine)
                val linked = devices.orEmpty().any { !it.primary }
                Text((if (linked) "A linked computer sees your chats and memories. " else "No computers linked yet. ") +
                    "Phone actions such as contacts and alarms stay on this phone.",
                    fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            }
            val primary = devices.orEmpty().any { it.primary && it.thisDevice }
            if (primary) {
                Button(onClick = ::scan, modifier = Modifier.fillMaxWidth().height(52.dp),
                    shape = RoundedCornerShape(16.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = Beep.colors.accent, contentColor = Beep.colors.onAccent)) {
                    Icon(Icons.Outlined.Laptop, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp))
                    Text("Link a computer", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                }
                Text("On the computer, open the Beep menu and choose Link to phone.",
                    fontSize = 12.sp, color = Beep.colors.muted,
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 8.dp))
            }
        }
    }
}

@Composable
private fun DeviceRow(device: LinkedDevice, onRemove: () -> Unit) {
    val computer = device.kind == DeviceKind.DESKTOP
    val name = if (device.thisDevice && !computer) Build.MODEL else device.name
    val note = when {
        device.thisDevice -> "This phone"
        computer -> "Desktop, linked " + DateUtils.getRelativeTimeSpanString(device.linkedAt.toEpochMilli())
        else -> "Phone"
    }
    Row(Modifier.fillMaxWidth().padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        DeviceIcon(if (computer) Icons.Outlined.Laptop else Icons.Outlined.PhoneAndroid,
            Beep.colors.tints[if (computer) 0 else 1], 40)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(name, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
            Text(note, fontSize = 12.sp, color = Beep.colors.muted)
        }
        if (device.primary) {
            Text("Primary", fontSize = 12.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink,
                modifier = Modifier.background(Beep.colors.tints[1], RoundedCornerShape(8.dp)).padding(horizontal = 8.dp, vertical = 3.dp))
        } else {
            TextButton(onClick = onRemove) { Text("Remove", color = Beep.colors.error) }
        }
    }
}

@Composable
private fun DeviceIcon(icon: ImageVector, tint: Color, size: Int) {
    Box(Modifier.size(size.dp).background(tint, RoundedCornerShape((size / 3.5).dp)), contentAlignment = Alignment.Center) {
        Icon(icon, null, Modifier.size((size / 2).dp), tint = Beep.colors.ink)
    }
}

/** Says what is about to be linked, with the code the computer shows too. */
@Composable
private fun LinkSheet(preview: LinkPreview, onCancel: () -> Unit, onLink: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp).padding(bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            DeviceIcon(Icons.Outlined.Laptop, Beep.colors.tints[0], 52)
            Spacer(Modifier.width(14.dp))
            Column {
                Text("Link ${preview.name}?", fontSize = 20.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.ink)
                Text("Desktop", fontSize = 13.sp, color = Beep.colors.muted)
            }
        }
        Text("It will see your chats and memories.", fontSize = 15.sp, lineHeight = 22.sp, color = Beep.colors.ink)
        Column(Modifier.fillMaxWidth().background(Beep.colors.raised, RoundedCornerShape(14.dp))
            .padding(horizontal = 14.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row {
                Text("Code", fontSize = 13.sp, color = Beep.colors.muted, modifier = Modifier.width(84.dp))
                Text(preview.code, fontSize = 13.sp, fontFamily = FontFamily.Monospace, letterSpacing = 1.3.sp, color = Beep.colors.ink)
            }
            Row {
                Text("Stays here", fontSize = 13.sp, color = Beep.colors.muted, modifier = Modifier.width(84.dp))
                Text("Contacts, alarms, location", fontSize = 13.sp, color = Beep.colors.ink)
            }
        }
        Text("Check that the code matches the one on the computer.", fontSize = 12.sp, color = Beep.colors.muted)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onCancel) { Text("Cancel") }
            Spacer(Modifier.width(8.dp))
            Button(onClick = onLink, shape = RoundedCornerShape(14.dp),
                colors = ButtonDefaults.buttonColors(containerColor = Beep.colors.accent, contentColor = Beep.colors.onAccent)) {
                Text("Link")
            }
        }
    }
}
