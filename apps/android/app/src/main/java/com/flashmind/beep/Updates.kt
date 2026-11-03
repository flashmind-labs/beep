package com.flashmind.beep

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.AppUpdate

/** Updates shown before the rest fold behind "N more". */
private const val SHOWN_UPDATES = 3

/**
 * What the event triage agent found in the user's apps, on its own card above an empty chat's
 * greeting rather than inside a reply. Tapping one hands it to [onOpen], which opens its chat;
 * the cross only dismisses it. Nothing shows while there are none.
 */
@Composable
internal fun UpdatesCard(updates: List<AppUpdate>, onOpen: (AppUpdate) -> Unit, onDismiss: (List<Long>) -> Unit, modifier: Modifier = Modifier) {
    if (updates.isEmpty()) return
    val t = Beep.colors
    var expanded by remember { mutableStateOf(false) }
    val shown = if (expanded) updates else updates.take(SHOWN_UPDATES)
    BeepCard(modifier.fillMaxWidth(), color = t.tints[1 % t.tints.size]) {
        Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Updates", fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
            TextButton(onClick = { onDismiss(updates.map { it.id }) }) {
                Text("Clear", fontSize = 12.sp, color = t.muted)
            }
        }
        shown.forEachIndexed { index, update ->
            HorizontalDivider(color = t.softLine)
            UpdateRow(update, index, onOpen = { onOpen(update) }, onDismiss = { onDismiss(listOf(update.id)) })
        }
        if (!expanded && updates.size > SHOWN_UPDATES) {
            HorizontalDivider(color = t.softLine)
            Text("${updates.size - SHOWN_UPDATES} more", fontSize = 12.sp, fontWeight = FontWeight.Medium, color = t.muted,
                modifier = Modifier.fillMaxWidth().clickable(onClickLabel = "Show all updates") { expanded = true }
                    .padding(horizontal = 12.dp, vertical = 12.dp))
        }
    }
}

@Composable
private fun UpdateRow(update: AppUpdate, index: Int, onOpen: () -> Unit, onDismiss: () -> Unit) {
    val t = Beep.colors
    Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 2.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.Top) {
        Row(Modifier.weight(1f).clickable(onClickLabel = "Ask about this", onClick = onOpen).padding(vertical = 4.dp),
            verticalAlignment = Alignment.Top) {
            AppLogo(update.app, update.appName, size = 26.dp)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(update.summary, fontSize = 13.sp, lineHeight = 18.sp)
                val time = listTime(update.at.toEpochMilli())
                Text(if (update.appName.isEmpty()) time else "${update.appName} · $time", fontSize = 11.5.sp, color = t.muted)
            }
        }
        IconButton(onClick = onDismiss, modifier = Modifier.size(40.dp)) {
            Icon(Icons.Outlined.Close, "Dismiss", Modifier.size(18.dp), tint = t.muted)
        }
    }
}
