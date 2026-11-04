package com.flashmind.beep

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.proto.Grant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.Duration
import java.time.Instant

/** The card asking for a grant is an ordinary approval whose action ends like this, as the server builds it. */
private const val FOR_A_ROUTINE = " for a routine"

/** Whether an approval asks for a routine's grant, which has no Always: a grant always ends. */
internal fun isRoutineGrant(action: String): Boolean = action.endsWith(FOR_A_ROUTINE)

/** Card fields the grant list shows on its own, so they are left out of the details. */
private val shownApart = setOf("Routine", "Ends")

/** Whether a field of the grant's card goes in its details; the routine and the end have their own lines. */
internal fun isGrantDetail(label: String): Boolean = label !in shownApart

/** "Ends in 40 min", "Ends in 5 h", "Ends in 3 d", or "Ended" once it is past. */
internal fun grantEnds(expiresAt: Instant, now: Instant): String {
    val left = Duration.between(now, expiresAt)
    if (left.isNegative || left.isZero) return "Ended"
    val minutes = (left.toSeconds() + 59) / 60
    val hours = (left.toMinutes() + 30) / 60
    val days = (left.toHours() + 12) / 24
    return when {
        minutes < 60 -> "Ends in $minutes min"
        hours < 48 -> "Ends in $hours h"
        else -> "Ends in $days d"
    }
}

/** "Last used 40 min ago", or "Not used yet". */
internal fun grantLastUsed(lastUsedAt: Instant?, now: Instant): String {
    lastUsedAt ?: return "Not used yet"
    val ago = Duration.between(lastUsedAt, now).coerceAtLeast(Duration.ZERO)
    return when {
        ago.toMinutes() < 1 -> "Last used just now"
        ago.toMinutes() < 60 -> "Last used ${ago.toMinutes()} min ago"
        ago.toHours() < 24 -> "Last used ${ago.toHours()} h ago"
        else -> "Last used ${ago.toDays()} d ago"
    }
}

/** What a routine's card says about its grants: "1 grant, ends in 5 h", the soonest end first. */
internal fun routineGrantsLine(expiries: List<Instant>, now: Instant): String? {
    val soonest = expiries.minOrNull() ?: return null
    val count = if (expiries.size == 1) "1 grant" else "${expiries.size} grants"
    return "$count, ${grantEnds(soonest, now).replaceFirstChar { it.lowercase() }}"
}

/**
 * What routines may do without asking. Opened from Settings it lists every grant; opened from a
 * routine, only that routine's. Revoke asks first, then ends the grant on the server.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GrantsScreen(repository: ChatRepository, routineId: String?, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var grants by remember { mutableStateOf<List<Grant>?>(null) }
    var error by remember { mutableStateOf("") }
    var asking by remember { mutableStateOf<Grant?>(null) }
    fun refresh() = scope.launch {
        runCatching { withContext(Dispatchers.IO) { repository.listGrants() } }
            .onSuccess { grants = it; error = "" }
            .onFailure { error = userMessage(it) }
    }
    fun revoke(grant: Grant) = scope.launch {
        runCatching { withContext(Dispatchers.IO) { repository.revokeGrant(grant.id) } }
            .onSuccess { grants = grants?.filter { it.id != grant.id } }
            // Already gone on the server: drop it here too.
            .onFailure { if (it is BeepException.NotFound) grants = grants?.filter { g -> g.id != grant.id } else error = userMessage(it) }
    }
    LaunchedEffect(Unit) { refresh() }
    asking?.let { grant ->
        AlertDialog(onDismissRequest = { asking = null }, containerColor = Beep.colors.surface,
            title = { Text(displayText("Revoke this grant?"), style = displayStyle(18.sp)) },
            text = { Text("${grant.routine.ifBlank { "This routine" }} will ask before each ${grant.action} again.", fontSize = 14.sp, lineHeight = 20.sp, color = Beep.colors.muted) },
            confirmButton = {
                ChunkyButton(onClick = { asking = null; revoke(grant) }, color = Beep.colors.error, contentColor = Beep.colors.page) {
                    Text("Revoke", fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = { TextButton(onClick = { asking = null }) { Text("Cancel") } })
    }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Routine grants"), style = displayStyle(18.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = Beep.colors.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            val shown = grants?.filter { routineId == null || it.routineId == routineId }
            when {
                shown == null -> if (error.isEmpty()) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                }
                shown.isEmpty() -> Text("No routine grants.\nWhen a routine needs to act without asking, Beep asks you once in a chat.",
                    fontSize = 13.sp, lineHeight = 20.sp, color = Beep.colors.muted, textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(40.dp))
                else -> {
                    val now = Instant.now()
                    LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        items(shown, key = { it.id }) { grant -> GrantCard(grant, now, onRevoke = { asking = grant }) }
                        item(key = "hint") {
                            Text(if (routineId != null) "Only this routine's grants." else "Routines do these without asking. Each one ends on its own, and anything else still asks.",
                                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.dim, textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun GrantCard(grant: Grant, now: Instant, onRevoke: () -> Unit) {
    val t = Beep.colors
    BeepCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(BeepSpace.row), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                AppLogo(grant.app.lowercase().replace(" ", ""), grant.app, size = 32.dp)
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text(grant.routine.ifBlank { "Routine" }, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = t.ink,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("${grant.app}: ${grant.action}", fontSize = 12.sp, color = t.muted)
                }
            }
            val details = grant.fields.filter { isGrantDetail(it.label) }
            if (details.isNotEmpty()) Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius))
                .background(t.raised).padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                details.forEach { field ->
                    Row {
                        Text(field.label, fontSize = 12.sp, color = t.muted, modifier = Modifier.width(76.dp))
                        Text(field.value, fontSize = 13.sp, color = t.ink, lineHeight = 18.sp, modifier = Modifier.weight(1f))
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(grantEnds(grant.expiresAt, now), fontSize = 12.sp, fontWeight = FontWeight.Medium, color = t.brand)
                    Text(grantLastUsed(grant.lastUsedAt, now), fontSize = 12.sp, color = t.muted)
                }
                TextButton(onClick = onRevoke) { Text("Revoke", color = t.error, fontWeight = FontWeight.Medium) }
            }
        }
    }
}
