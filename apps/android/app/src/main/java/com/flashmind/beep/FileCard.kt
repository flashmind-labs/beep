package com.flashmind.beep

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.text.format.Formatter
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import com.flashmind.beep.ffi.BeepException
import kotlinx.coroutines.launch
import java.io.File

/** A file Beep sent. Tapping fetches it once into the cache and opens it; the share button hands it to another app. */
@Composable
fun FileCard(card: ToolCard.File) {
    val t = Beep
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var loading by remember(card.id) { mutableStateOf(false) }
    var gone by remember(card.id) { mutableStateOf(false) }
    fun withFile(then: (Uri) -> Unit) {
        if (loading || gone) return
        loading = true
        scope.launch {
            runCatching { SentFiles.fetch(context, card) }
                .onSuccess(then)
                .onFailure { error ->
                    if (error is BeepException.NotFound) gone = true
                    else Toast.makeText(context, userMessage(error), Toast.LENGTH_SHORT).show()
                }
            loading = false
        }
    }
    val detail = when {
        gone -> "No longer available"
        loading -> "Downloading..."
        else -> listOf(kindOf(card), Formatter.formatShortFileSize(context, card.size)).filter(String::isNotBlank).joinToString(", ")
    }
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(t.shapes.smallRadius))
        .border(1.dp, t.colors.softLine, RoundedCornerShape(t.shapes.smallRadius))
        .background(t.colors.surface).alpha(if (gone) 0.6f else 1f)
        .clickable(enabled = !gone) { withFile { open(context, it, card.mime) } }
        .padding(start = 12.dp, top = 10.dp, bottom = 10.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.size(40.dp).background(t.colors.tints[0], RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
            Icon(Icons.Outlined.Description, null, Modifier.size(20.dp), tint = t.colors.ink)
        }
        Column(Modifier.weight(1f)) {
            Text(card.name, fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink,
                maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(detail, fontSize = 12.sp, color = t.colors.muted, maxLines = 1)
        }
        when {
            loading -> Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = t.colors.muted)
            }
            !gone -> IconButton(onClick = { withFile { share(context, it, card.mime) } }) {
                Icon(Icons.Outlined.Share, "Share", Modifier.size(18.dp), tint = t.colors.ink)
            }
        }
    }
}

/** "PDF" for a PDF, else the extension in capitals. */
private fun kindOf(card: ToolCard.File): String =
    if (card.mime == "application/pdf") "PDF" else card.name.substringAfterLast('.', "").uppercase().take(5)

private fun open(context: Context, uri: Uri, mime: String) {
    val view = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    try {
        context.startActivity(view)
    } catch (_: ActivityNotFoundException) {
        share(context, uri, mime)
    }
}

private fun share(context: Context, uri: Uri, mime: String) {
    val send = Intent(Intent.ACTION_SEND).setType(mime).putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    send.clipData = ClipData.newRawUri(null, uri)
    context.startActivity(Intent.createChooser(send, null))
}

/** Sent files, kept in the cache under their id so each downloads once. */
object SentFiles {
    private const val FOLDER = "sent"

    suspend fun fetch(context: Context, card: ToolCard.File): Uri {
        val dir = File(context.cacheDir, "$FOLDER/${card.id}").apply { mkdirs() }
        val file = File(dir, card.name.replace('/', '_'))
        if (!file.exists()) {
            val client = BeepClients.current() ?: throw BeepException.Connection("not connected to your server")
            client.saveFile(card.id, file.path)
        }
        return FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    }
}
