package com.flashmind.beep

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.animation.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.draw.clip
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.draw.scale
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale
import java.time.format.DateTimeFormatter

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun CronsScreen(repository: ChatRepository, onBack: () -> Unit, onAskBeep: () -> Unit, onOpenChat: (String) -> Unit) {
    val cached = remember { repository.cachedCrons() }
    var crons by remember { mutableStateOf(cached.orEmpty()) }
    var loading by remember { mutableStateOf(cached == null) }
    var error by remember { mutableStateOf("") }
    var showCreate by remember { mutableStateOf(false) }
    // Swiped away and waiting out the Undo bar; the delete is only sent once it closes.
    var hidden by remember { mutableStateOf(setOf<String>()) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    fun delete(cron: ChatRepository.Cron) {
        hidden = hidden + cron.id
        scope.launch {
            snackbar.currentSnackbarData?.dismiss()
            var undone = false
            try {
                undone = snackbar.showSnackbar("Routine deleted", "Undo", duration = SnackbarDuration.Short) == SnackbarResult.ActionPerformed
            } finally {
                // Leaving the screen cancels the bar; the delete still goes out.
                val failed = if (undone) null else withContext(NonCancellable + Dispatchers.IO) {
                    runCatching { repository.deleteCron(cron.id) }.exceptionOrNull()
                }
                if (failed != null) error = failed.message.orEmpty()
                if (undone || failed != null) hidden = hidden - cron.id
            }
        }
    }
    fun run(cron: ChatRepository.Cron) { scope.launch {
        try { onOpenChat(withContext(Dispatchers.IO) { repository.runCron(cron.id) }) }
        catch (e: Exception) { error = userMessage(e) }
    } }
    fun refresh() { scope.launch {
        error = ""
        try { crons = withContext(Dispatchers.IO) { repository.listCrons() } }
        catch (e: Exception) { error = userMessage(e) }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    if (showCreate) {
        CreateCronDialog(onDismiss = { showCreate = false }, onAskBeep = { showCreate = false; onAskBeep() }, onCreate = { label, prompt, schedule ->
            scope.launch {
                try { withContext(Dispatchers.IO) { repository.createCron(label, prompt, schedule) }; showCreate = false; refresh() }
                catch (e: Exception) { error = userMessage(e) }
            }
        })
    }
    Scaffold(containerColor = Ink, snackbarHost = {
        SnackbarHost(snackbar) { Snackbar(it, actionColor = Beep.colors.ok, shape = RoundedCornerShape(12.dp)) }
    }, topBar = {
        TopAppBar(title = { Text(displayText("Routines"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { ChunkyButton(onClick = { showCreate = true }, modifier = Modifier.padding(end = 14.dp), color = Beep.colors.accent,
                contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(8.dp)) { Icon(Icons.Outlined.Add, "New routine", Modifier.size(20.dp)) } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (crons.isEmpty()) Text("No routines yet.\nSchedule tasks for Beep to run on a timer.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(crons.filter { it.id !in hidden }, key = { it.id }) { cron ->
                    RoutineCard(cron, Modifier.animateItem(), onRun = { run(cron) }, onDelete = { delete(cron) }, onToggle = { enabled ->
                        scope.launch {
                            try { withContext(Dispatchers.IO) { repository.toggleCron(cron.id, enabled) }; refresh() }
                            catch (e: Exception) { error = userMessage(e) }
                        }
                    })
                }
                item(key = "hint") {
                    Text("Swipe right to run now, left to delete", fontSize = 11.sp, color = TextDim, textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp).animateItem())
                }
            }
        }
    }
}

/** A routine that runs when swiped right and is deleted when swiped left. */
@Composable
private fun RoutineCard(cron: ChatRepository.Cron, modifier: Modifier, onRun: () -> Unit, onDelete: () -> Unit, onToggle: (Boolean) -> Unit) {
    val state = rememberSwipeToDismissBoxState(confirmValueChange = { value ->
        when (value) {
            // Running is not a dismissal: start it and let the card spring back.
            SwipeToDismissBoxValue.StartToEnd -> { onRun(); false }
            SwipeToDismissBoxValue.EndToStart, SwipeToDismissBoxValue.Settled -> true
        }
    }, positionalThreshold = { it * 0.35f })
    // Wait for the slide-out to finish before the list closes the gap.
    LaunchedEffect(state.currentValue) {
        if (state.currentValue == SwipeToDismissBoxValue.EndToStart) onDelete()
    }
    val armed = state.targetValue != SwipeToDismissBoxValue.Settled
    val haptics = LocalHapticFeedback.current
    LaunchedEffect(armed) { if (armed) haptics.performHapticFeedback(HapticFeedbackType.LongPress) }
    val shape = RoundedCornerShape(Beep.shapes.radius)
    SwipeToDismissBox(state, modifier = modifier.clip(shape), backgroundContent = {
        val toDelete = state.dismissDirection == SwipeToDismissBoxValue.EndToStart
        val color by animateColorAsState(when {
            !armed -> Border
            toDelete -> MaterialTheme.colorScheme.error
            else -> Beep.colors.accent
        }, tween(150), label = "swipe-color")
        val scale by animateFloatAsState(if (armed) 1.3f else 1f, spring(dampingRatio = Spring.DampingRatioMediumBouncy), label = "swipe-icon")
        if (state.dismissDirection != SwipeToDismissBoxValue.Settled) {
            Box(Modifier.fillMaxSize().background(color).padding(horizontal = 22.dp),
                contentAlignment = if (toDelete) Alignment.CenterEnd else Alignment.CenterStart) {
                val tint = Beep.colors.onAccent
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (!toDelete) Icon(Icons.Outlined.PlayArrow, null, Modifier.size(20.dp).scale(scale), tint = tint)
                    Text(if (toDelete) "Delete" else "Run now", fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = tint)
                    if (toDelete) Icon(Icons.Outlined.Delete, null, Modifier.size(20.dp).scale(scale), tint = tint)
                }
            }
        }
    }) {
        BeepCard(Modifier.fillMaxWidth()) {
            Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f)) {
                    Text(cron.label.ifBlank { "Reminder" }, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Spacer(Modifier.height(3.dp))
                    Text(cron.prompt, fontSize = 12.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 18.sp)
                    Spacer(Modifier.height(4.dp))
                    Text(describeSchedule(cron.schedule), fontSize = 11.sp, color = TextDim)
                    if (cron.enabled && onceAt(cron.schedule) == null) cron.nextRunAt?.let(::describeNextRun)?.let {
                        Text(it, fontSize = 11.sp, color = TextDim)
                    }
                }
                Switch(checked = cron.enabled, onCheckedChange = onToggle, modifier = Modifier.height(28.dp),
                    colors = SwitchDefaults.colors(checkedTrackColor = Beep.colors.ok, uncheckedTrackColor = Border))
            }
        }
    }
}

private enum class Repeat(val title: String) { Daily("Every day"), Weekdays("Weekdays"), Weekly("Weekly"), Hourly("Every hour"), Custom("Cron") }

/** Cron weekday numbers, Sunday being 0, in the order the picker shows them. */
private val cronDays = listOf(1, 2, 3, 4, 5, 6, 0)

private fun cronDayName(day: Int, style: TextStyle = TextStyle.SHORT): String =
    DayOfWeek.of(if (day == 0) 7 else day).getDisplayName(style, Locale.getDefault())

private fun clockTime(hour: Int, minute: Int): String =
    LocalTime.of(hour, minute).format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))

/** Reads the cron shapes the dialog writes as words. Anything else is shown as written. */
private fun describeSchedule(expression: String): String {
    onceAt(expression)?.let { return "Once, " + describeDay(it) }
    val parts = expression.trim().split(Regex("\\s+"))
    if (parts.size != 5 || parts[2] != "*" || parts[3] != "*") return expression
    val (min, hour, _, _, days) = parts
    val minute = min.toIntOrNull()?.takeIf { it in 0..59 } ?: return expression
    if (hour == "*" && days == "*") return if (minute == 0) "Every hour" else "Every hour at :%02d".format(minute)
    val at = clockTime(hour.toIntOrNull()?.takeIf { it in 0..23 } ?: return expression, minute)
    return when {
        days == "*" -> "Every day at $at"
        days == "1-5" -> "Weekdays at $at"
        days.toIntOrNull() in 0..6 -> "Every ${cronDayName(days.toInt(), TextStyle.FULL)} at $at"
        days.split(",").all { it.toIntOrNull() in 0..6 } -> days.split(",").joinToString(", ") { cronDayName(it.toInt()) } + " at $at"
        else -> expression
    }
}

/** A one-off reminder's time in the phone's timezone; its schedule is a timestamp rather than cron. */
private fun onceAt(schedule: String): ZonedDateTime? =
    runCatching { OffsetDateTime.parse(schedule.trim()).atZoneSameInstant(ZoneId.systemDefault()) }.getOrNull()

/** "today 9:00", "tomorrow 9:00", "Mon 9:00" or, further out, "12 Oct 9:00". */
private fun describeDay(at: ZonedDateTime): String {
    val today = LocalDate.now()
    val date = at.toLocalDate()
    val day = when {
        date == today -> "today"
        date == today.plusDays(1) -> "tomorrow"
        date.isBefore(today.plusDays(7)) && !date.isBefore(today) -> at.dayOfWeek.getDisplayName(TextStyle.SHORT, Locale.getDefault())
        else -> "${date.dayOfMonth} ${date.month.getDisplayName(TextStyle.SHORT, Locale.getDefault())}"
    }
    return "$day ${clockTime(at.hour, at.minute)}"
}

/** "Next: Mon 9:00" in the phone's own timezone. */
private fun describeNextRun(rfc3339: String): String? =
    onceAt(rfc3339)?.let { "Next: " + describeDay(it) }

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun CreateCronDialog(onDismiss: () -> Unit, onAskBeep: () -> Unit, onCreate: (String, String, String) -> Unit) {
    var label by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf(Repeat.Daily) }
    var days by remember { mutableStateOf(setOf(1)) }
    var custom by remember { mutableStateOf("") }
    var pickingTime by remember { mutableStateOf(false) }
    val time = rememberTimePickerState(initialHour = 9, initialMinute = 0)
    val schedule = when (repeat) {
        Repeat.Daily -> "${time.minute} ${time.hour} * * *"
        Repeat.Weekdays -> "${time.minute} ${time.hour} * * 1-5"
        Repeat.Weekly -> if (days.isEmpty()) "" else "${time.minute} ${time.hour} * * ${cronDays.filter { it in days }.joinToString(",")}"
        Repeat.Hourly -> "${time.minute} * * * *"
        Repeat.Custom -> custom.trim()
    }
    val fieldColors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
        unfocusedBorderColor = Border, focusedBorderColor = Moss)
    if (pickingTime) AlertDialog(onDismissRequest = { pickingTime = false }, containerColor = Surface,
        text = { TimePicker(time) },
        confirmButton = { TextButton(onClick = { pickingTime = false }) { Text("Done") } })
    AlertDialog(onDismissRequest = onDismiss, containerColor = Surface,
        title = { Text(displayText("New routine"), style = displayStyle(22.sp)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(label, { label = it }, label = { Text("Name") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Morning briefing") }, colors = fieldColors)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Repeat.entries.forEach { r -> FilterChip(selected = repeat == r, onClick = { repeat = r }, label = { Text(r.title) }) }
            }
            if (repeat == Repeat.Weekly) FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                cronDays.forEach { d ->
                    FilterChip(selected = d in days, onClick = { days = if (d in days) days - d else days + d },
                        label = { Text(cronDayName(d, TextStyle.NARROW)) })
                }
            }
            when (repeat) {
                Repeat.Custom -> OutlinedTextField(custom, { custom = it }, label = { Text("Cron expression") }, singleLine = true,
                    modifier = Modifier.fillMaxWidth(), placeholder = { Text("0 9 * * *") }, colors = fieldColors)
                Repeat.Hourly -> OutlinedButton(onClick = { pickingTime = true }) { Text("At :%02d past the hour".format(time.minute)) }
                else -> OutlinedButton(onClick = { pickingTime = true }) { Text("At ${clockTime(time.hour, time.minute)}") }
            }
            OutlinedTextField(prompt, { prompt = it }, label = { Text("What should Beep do?") }, maxLines = 4, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Give me a summary of...") }, colors = fieldColors)
            TextButton(onClick = onAskBeep, contentPadding = PaddingValues(0.dp)) { Text("Or just tell Beep in a chat") }
        } },
        confirmButton = { ChunkyButton(onClick = { onCreate(label, prompt, schedule) }, color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
            enabled = label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) { Text("Create", fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}
