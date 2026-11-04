package com.flashmind.beep

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.proto.HistoryEntry
import com.flashmind.beep.proto.SharedLink
import com.flashmind.beep.proto.SharedSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Shares [chat] as it is now and shows its link. Opening the sheet again on the
 * same chat updates the shared copy and keeps the link.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareSheet(repository: ChatRepository, chat: Conversation, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var link by remember(chat.id) { mutableStateOf<SharedLink?>(null) }
    LaunchedEffect(chat.id) {
        runCatching { withContext(Dispatchers.IO) { repository.shareChat(chat.id) } }
            .onSuccess { link = it }
            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show(); onDismiss() }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(52.dp).background(Beep.colors.tints[1], RoundedCornerShape(Beep.shapes.radius)), contentAlignment = Alignment.Center) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(24.dp), tint = Beep.colors.ink)
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(displayText("Share this chat"), style = displayStyle(18.sp), color = Beep.colors.ink)
                    Text(chat.title, fontSize = 13.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            val url = link?.url
            Box(Modifier.fillMaxWidth().height(46.dp).background(Beep.colors.page, RoundedCornerShape(Beep.shapes.smallRadius)).padding(horizontal = 14.dp),
                contentAlignment = Alignment.CenterStart) {
                if (url == null) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                else Text(url, fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = Beep.colors.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OutlinedButton(onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Beep chat", url))
                    Toast.makeText(context, "Link copied", Toast.LENGTH_SHORT).show()
                }, enabled = url != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(Beep.shapes.smallRadius)) {
                    Icon(Icons.Outlined.ContentCopy, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Copy", fontWeight = FontWeight.SemiBold)
                }
                Button(onClick = {
                    val text = "${chat.title}\n$url"
                    context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null))
                }, enabled = url != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(Beep.shapes.smallRadius),
                    colors = ButtonDefaults.buttonColors(containerColor = Beep.colors.accent, contentColor = Beep.colors.onAccent)) {
                    Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Send", fontWeight = FontWeight.SemiBold)
                }
            }
            Text("A copy of the chat as it is now. Later messages are not in it, and sharing again updates it. " +
                "Only your messages and Beep's replies are shared, not tool results, thoughts or your location. " +
                "Anyone with Beep can open the link.",
                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = {
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.unshareChat(chat.id) } }
                            .onSuccess { Toast.makeText(context, "Stopped sharing. The link no longer opens.", Toast.LENGTH_SHORT).show(); onDismiss() }
                            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                    }
                }, enabled = url != null) { Text("Stop sharing", color = Beep.colors.error) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onDismiss) { Text("Done") }
            }
        }
    }
}

/** The shared copy behind a `beep://share/` link, read only, with a button to carry it on as one's own chat. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedChatScreen(repository: ChatRepository, token: String, onBack: () -> Unit, onContinued: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var shared by remember(token) { mutableStateOf<SharedSession?>(null) }
    var gone by remember(token) { mutableStateOf(false) }
    var copying by remember(token) { mutableStateOf(false) }
    LaunchedEffect(token) {
        runCatching { withContext(Dispatchers.IO) { repository.sharedChat(token) } }
            .onSuccess { shared = it }
            .onFailure { if (it is BeepException.NotFound) gone = true else { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show(); onBack() } }
    }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(modifier = Modifier.beepBar(), expandedHeight = 54.dp, title = {
            Column {
                Text(displayText(shared?.title ?: "Shared chat"), style = displayStyle(18.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                shared?.let {
                    Text("Shared " + DateUtils.getRelativeTimeSpanString(it.sharedAt), fontSize = 12.sp, color = Beep.colors.muted)
                }
            }
        }, navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            val chat = shared
            when {
                gone -> Text("Not found.\nThe owner stopped sharing this chat, or deleted it.", textAlign = TextAlign.Center,
                    color = Beep.colors.muted, lineHeight = 22.sp, modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 60.dp))
                chat == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                }
                else -> {
                    LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = BeepSpace.page), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        itemsIndexed(chat.entries) { _, entry -> SharedEntry(entry) }
                    }
                    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 10.dp, bottom = 14.dp),
                        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = {
                            copying = true
                            scope.launch {
                                runCatching { withContext(Dispatchers.IO) { repository.continueShared(token) } }
                                    .onSuccess { onContinued() }
                                    .onFailure { copying = false; Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                            }
                        }, enabled = !copying, modifier = Modifier.fillMaxWidth().height(52.dp), shape = RoundedCornerShape(Beep.shapes.radius),
                            colors = ButtonDefaults.buttonColors(containerColor = Beep.colors.accent, contentColor = Beep.colors.onAccent)) {
                            Icon(Icons.Outlined.Add, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp))
                            Text("Continue in my chats", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                        }
                        Text("Read only. Continuing makes your own copy.", fontSize = 12.sp, color = Beep.colors.muted)
                    }
                }
            }
        }
    }
}

@Composable
private fun SharedEntry(entry: HistoryEntry) {
    val radius = Beep.shapes.radius
    when (entry) {
        is HistoryEntry.User -> Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
            Box(Modifier.padding(start = 48.dp).beepFrame(RoundedCornerShape(radius, radius, 6.dp, radius), Beep.colors.user)) {
                MessageContent(entry.content, "user", compact = false)
            }
        }
        is HistoryEntry.Assistant -> MessageContent(entry.content, "assistant", compact = false)
        else -> {}
    }
}
