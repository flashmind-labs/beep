package com.flashmind.beep

import android.net.Uri
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.ui.res.painterResource
import androidx.compose.foundation.Image
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

/** What a tool is doing, in words. Unknown tools fall back to their name: "GMAIL_SEND_EMAIL" becomes "Gmail: send email". */
fun toolLabel(name: String): String = toolLabels[name] ?: humanize(name)

private fun humanize(name: String): String {
    val words = name.split('_', '.', '-').filter { it.isNotBlank() }.map { it.lowercase() }
    if (words.isEmpty()) return ""
    // Connected app tools are named APP_ACTION.
    if (name == name.uppercase() && words.size > 1)
        return words[0].replaceFirstChar { it.uppercase() } + ": " + words.drop(1).joinToString(" ")
    return words.joinToString(" ").replaceFirstChar { it.uppercase() }
}

private val toolLabels = mapOf(
    "web_search" to "Searching the web",
    "decision" to "Weighing the options",
    "get_location" to "Checking your location",
    "get_notifications" to "Reading notifications",
    "get_time" to "Checking the time",
    "open_app" to "Opening an app",
    "set_alarm" to "Setting an alarm",
    "open_browser" to "Opening a page",
    "browser" to "Browsing",
    "open_maps" to "Looking up a place",
    "query_contacts" to "Looking up contacts",
    "suggest_replies" to "Suggesting replies",
    "task_progress" to "Updating progress",
    "memory_store" to "Remembering",
    "memory_recall" to "Recalling memories",
    "memory_update" to "Updating a memory",
    "memory_forget" to "Forgetting a memory",
    "cron_create" to "Setting a reminder",
    "schedule_once" to "Setting a reminder",
    "cron_edit" to "Changing a reminder",
    "cron_list" to "Checking reminders",
    "cron_delete" to "Deleting a reminder",
    "cron_history" to "Checking past reminders",
    "habit_create" to "Creating a habit",
    "habit_log" to "Logging a habit",
    "habit_status" to "Checking habits",
    "habit_delete" to "Deleting a habit",
)

/** "Searching the web…" becomes "Searched the web" once the tool is done. */
private fun doneLabel(label: String): String {
    val trimmed = label.trimEnd('…', '.', ' ')
    val first = trimmed.substringBefore(' ')
    val past = pastTense[first.lowercase()] ?: return trimmed
    return past + trimmed.substring(first.length)
}

private val pastTense = mapOf(
    "searching" to "Searched", "reading" to "Read", "checking" to "Checked", "looking" to "Looked",
    "fetching" to "Fetched", "opening" to "Opened", "saving" to "Saved", "finding" to "Found",
    "getting" to "Got", "creating" to "Created", "updating" to "Updated", "deleting" to "Deleted",
    "sending" to "Sent", "setting" to "Set", "using" to "Used", "remembering" to "Remembered",
    "loading" to "Loaded", "browsing" to "Browsed", "scheduling" to "Scheduled", "running" to "Ran",
    "weighing" to "Weighed", "recalling" to "Recalled", "forgetting" to "Forgot", "changing" to "Changed",
    "logging" to "Logged", "suggesting" to "Suggested",
)

private data class Source(val title: String, val url: String) {
    val host: String get() = runCatching { Uri.parse(url).host?.removePrefix("www.") }.getOrNull().orEmpty()
}

/** Pages a search tool returned, from its `{"results":[{"url","title"}]}` output. */
private fun sourcesOf(result: String): List<Source> {
    if (!result.trimStart().startsWith("{")) return emptyList()
    val results = runCatching { JSONObject(result).optJSONArray("results") }.getOrNull() ?: return emptyList()
    return (0 until results.length()).mapNotNull { i ->
        val item = results.optJSONObject(i) ?: return@mapNotNull null
        val url = item.optString("url")
        if (url.isBlank()) null else Source(item.optString("title").ifBlank { url }, url)
    }
}

/**
 * One tool call as a tinted pill. The icon wiggles while the tool runs and a
 * check gets stamped on when it finishes. A finished search says how many pages
 * it read, and tapping it lists them.
 */
@Composable
fun ToolChip(call: ToolCall, tint: Color, modifier: Modifier = Modifier, onOpen: (String) -> Unit = {}) {
    val t = LocalBeep.current
    val noMotion = reduceMotion()
    val running = !call.isDone
    val quiet = t.shapes.card == CardLook.Hairline
    val shape = RoundedCornerShape(50)
    val frame = if (quiet) Modifier.clip(shape).background(tint)
        else Modifier.beepFrame(shape, tint)
    val described = toolLabel(call.name).ifBlank { call.description }.ifBlank { "Using a tool" }
    val sources = remember(call.result) { if (running) emptyList() else sourcesOf(call.result) }
    val label = when {
        running -> described
        sources.isNotEmpty() -> doneLabel(described) + " \u00b7 " + (if (sources.size == 1) "1 source" else "${sources.size} sources")
        else -> doneLabel(described)
    }
    var open by remember(call.callId) { mutableStateOf(false) }
    // Remembered once, so chips that were already done when the chat opened stay still.
    val sawRunning = remember(call.callId) { running }

    Column(modifier) {
        Row(Modifier.popIn(call.callId.ifEmpty { call.name }, play = running)
            .then(frame)
            .then(if (sources.isNotEmpty()) Modifier.clickable { open = !open } else Modifier)
            .padding(start = 5.dp, end = 12.dp, top = 5.dp, bottom = 5.dp),
            verticalAlignment = Alignment.CenterVertically) {
            val wiggle by if (running && !noMotion) rememberInfiniteTransition(label = "tool-wiggle").animateFloat(
                initialValue = -12f, targetValue = 12f,
                animationSpec = infiniteRepeatable(tween(260), RepeatMode.Reverse), label = "tool-wiggle-angle")
                else remember { mutableFloatStateOf(0f) }
            val appLogo = remember(call.name) { appLogoForTool(call.name) }
            Box(Modifier.size(26.dp).graphicsLayer { rotationZ = wiggle }
                .background(if (appLogo != null) Color.White else t.colors.surface, CircleShape),
                contentAlignment = Alignment.Center) {
                if (appLogo != null) Image(painterResource(appLogo), null, Modifier.size(15.dp))
                else Icon(toolIconFor(call.name.ifEmpty { call.description }), null, Modifier.size(15.dp), tint = t.colors.ink)
            }
            Spacer(Modifier.width(8.dp))
            Text(label, fontSize = 13.sp, fontWeight = FontWeight.Medium,
                color = if (quiet) t.colors.muted else t.colors.ink,
                maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
            Spacer(Modifier.width(8.dp))
            Box(Modifier.size(18.dp), contentAlignment = Alignment.Center) {
                if (running) {
                    CircularProgressIndicator(Modifier.size(13.dp), color = t.colors.muted, strokeWidth = 2.dp)
                } else {
                    val stamp = remember(call.callId) { Animatable(if (sawRunning && !noMotion) 0f else 1f) }
                    LaunchedEffect(call.callId) { stamp.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = 500f)) }
                    val ok = call.success
                    Box(Modifier.size(18.dp)
                        .graphicsLayer {
                            val s = 1.8f - 0.8f * stamp.value
                            scaleX = s; scaleY = s; alpha = stamp.value.coerceIn(0f, 1f)
                            rotationZ = -14f * (1 - stamp.value)
                        }
                        .background(if (ok) t.colors.ok else t.colors.error, CircleShape),
                        contentAlignment = Alignment.Center) {
                        Icon(if (ok) Icons.Outlined.Check else Icons.Outlined.Close, if (ok) "Done" else "Failed",
                            Modifier.size(12.dp), tint = Color.White)
                    }
                }
            }
            if (sources.isNotEmpty()) {
                val turn by animateFloatAsState(if (open) 180f else 0f, label = "sources-chevron")
                Icon(Icons.Outlined.KeyboardArrowDown, if (open) "Hide sources" else "Show sources",
                    Modifier.padding(start = 4.dp).size(18.dp).graphicsLayer { rotationZ = turn }, tint = t.colors.muted)
            }
        }
        AnimatedVisibility(open, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Column(Modifier.padding(top = 6.dp, start = 4.dp)
                .border(1.dp, t.colors.softLine, RoundedCornerShape(t.shapes.smallRadius))
                .clip(RoundedCornerShape(t.shapes.smallRadius))
                .background(t.colors.surface)) {
                sources.forEachIndexed { i, source ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(t.colors.softLine))
                    Row(Modifier.fillMaxWidth().clickable { onOpen(source.url) }.padding(horizontal = 12.dp, vertical = 9.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(22.dp).background(t.colors.tints[i % t.colors.tints.size], CircleShape),
                            contentAlignment = Alignment.Center) {
                            Text(source.host.take(1).uppercase(), fontSize = 11.sp, fontWeight = FontWeight.Bold, color = t.colors.ink)
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(source.title, fontSize = 13.sp, fontWeight = FontWeight.Medium, color = t.colors.ink,
                                maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(source.host, fontSize = 11.sp, color = t.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}

/** Three small previews of the looks. Tapping one switches the whole app. */
@Composable
fun ThemePicker(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val current = Beep.style
    Column(modifier) {
        Text("Look", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            BeepStyle.entries.forEach { style ->
                val tokens = BeepTokens.of(style)
                val c = tokens.colors
                val selected = style == current
                val shape = RoundedCornerShape(14.dp)
                val lift by animateSelection(selected)
                Row(Modifier.fillMaxWidth()
                    .graphicsLayer { translationX = 4.dp.toPx() * lift }
                    .border(if (selected) 2.dp else 1.dp, if (selected) Beep.colors.ink else Beep.colors.softLine, shape)
                    .clip(shape)
                    .background(c.page)
                    .clickable { Appearance.set(context, style) }
                    .padding(horizontal = 12.dp, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        Box(Modifier.size(14.dp).background(c.user, CircleShape)
                            .border(if (tokens.shapes.outline > 0.dp) 1.5.dp else 0.dp, c.line, CircleShape))
                        Box(Modifier.size(14.dp).background(c.accent, CircleShape))
                        Box(Modifier.size(14.dp).background(lerp(c.ok, c.page, 0.15f), CircleShape))
                    }
                    Spacer(Modifier.width(12.dp))
                    Text(if (tokens.fonts.displayUppercase) style.label.uppercase() else style.label,
                        fontFamily = tokens.fonts.display, fontWeight = tokens.fonts.displayWeight,
                        fontSize = 14.sp, color = c.ink, maxLines = 1, modifier = Modifier.weight(1f))
                    if (selected) Icon(Icons.Outlined.Check, "Selected", Modifier.size(16.dp), tint = c.ink)
                }
            }
        }
    }
}

@Composable
private fun animateSelection(selected: Boolean) =
    animateFloatAsState(if (selected) 1f else 0f,
        spring(dampingRatio = 0.5f, stiffness = 500f), label = "theme-lift")
