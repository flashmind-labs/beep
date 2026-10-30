package com.flashmind.beep

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.WorkItem
import com.flashmind.beep.proto.WorkKind
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.time.Duration
import java.time.Instant

/** Tools of a turn that start or change background work; when one finishes, the chat lists its work again. */
internal val WORK_TOOLS = setOf("spawn_agent", "run_in_background", "message_agent", "cancel_job")

/** How often the list is read again while something runs. */
private const val WORK_POLL_MS = 4_000L

/**
 * The sub-agents and jobs running in [chat], as a strip above the composer. Tapping it lists them, each
 * with Stop; those that ended stay listed while others run. It goes once nothing runs: the results come
 * back into the chat as a "Background work" turn.
 */
@Composable
internal fun BackgroundWork(repository: ChatRepository, chat: Conversation, compact: Boolean) {
    val t = Beep.colors
    val started = chat.messages.lastOrNull { it.role == "assistant" }?.toolCalls?.count { it.isDone && it.name in WORK_TOOLS } ?: 0
    var items by remember(chat.id) { mutableStateOf(emptyList<WorkItem>()) }
    var open by remember(chat.id) { mutableStateOf(false) }
    var refresh by remember(chat.id) { mutableStateOf(0) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(chat.id, chat.busy, started, refresh) {
        while (true) {
            items = repository.listWork(chat.id)
            if (items.none { it.outcome == null }) break
            delay(WORK_POLL_MS)
        }
    }
    val running = items.count { it.outcome == null }
    AnimatedVisibility(visible = running > 0, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
        Column(Modifier.padding(horizontal = if (compact) 10.dp else 14.dp).padding(top = 6.dp).fillMaxWidth()
            .beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), t.surface, pressableLook())) {
            Row(Modifier.fillMaxWidth().clickable(onClickLabel = if (open) "Hide background work" else "Show background work") { open = !open }
                .padding(horizontal = 14.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = t.muted)
                Text("$running working in the background", Modifier.weight(1f), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Icon(if (open) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, null, Modifier.size(18.dp), tint = t.muted)
            }
            AnimatedVisibility(visible = open) {
                Column(Modifier.heightIn(max = 260.dp).verticalScroll(rememberScrollState())) {
                    items.forEach { item ->
                        Box(Modifier.fillMaxWidth().height(1.dp).background(t.softLine))
                        WorkRow(item, onStop = {
                            scope.launch {
                                runCatching { repository.cancelWork(chat.id, item.id) }
                                refresh++
                            }
                        })
                    }
                }
            }
        }
    }
}

@Composable
private fun WorkRow(item: WorkItem, onStop: () -> Unit) {
    val t = Beep.colors
    val outcome = item.outcome
    Row(Modifier.fillMaxWidth().padding(start = 14.dp, end = 4.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.padding(top = 2.dp).size(18.dp), contentAlignment = Alignment.Center) {
            when {
                outcome == null -> CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 2.dp, color = t.muted)
                outcome.ok -> Icon(Icons.Outlined.Check, "Done", Modifier.size(16.dp), tint = t.ok)
                else -> Icon(Icons.Outlined.Close, "Did not finish", Modifier.size(16.dp), tint = t.dim)
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(kindLabel(item.kind).uppercase(), fontSize = 10.sp, fontWeight = FontWeight.SemiBold, color = t.dim, letterSpacing = 0.4.sp)
            Text(item.task, fontSize = 14.sp, fontWeight = FontWeight.Medium, lineHeight = 19.sp, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Text(if (outcome == null) "${item.step.ifBlank { "Starting" }} · ${minutesSince(item.startedAt)}" else outcome.text,
                fontSize = 12.sp, color = t.muted, lineHeight = 17.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (outcome == null) TextButton(onClick = onStop) { Text("Stop", color = t.error, fontWeight = FontWeight.SemiBold, fontSize = 13.sp) }
    }
}

private fun kindLabel(kind: WorkKind): String = when (kind) {
    WorkKind.AGENT -> "Agent"
    WorkKind.JOB -> "Tool call"
}

/** "3 min" since [started], at least one. */
private fun minutesSince(started: Instant): String =
    "${Duration.between(started, Instant.now()).toMinutes().coerceAtLeast(1)} min"
