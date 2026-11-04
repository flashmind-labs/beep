package com.flashmind.beep

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.proto.Skill
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val Coral: Color @Composable @ReadOnlyComposable get() = Beep.colors.error
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

/** How the user and their groups like recurring tasks done. Beep writes these; the user can pause, edit, undo or delete them. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SkillsScreen(repository: ChatRepository, onBack: () -> Unit) {
    val cached = remember { repository.cachedSkills() }
    var skills by remember { mutableStateOf(cached.orEmpty()) }
    var loading by remember { mutableStateOf(cached == null) }
    var error by remember { mutableStateOf("") }
    var openId by rememberSaveable { mutableStateOf<String?>(null) }
    var editing by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        error = ""
        try { skills = withContext(Dispatchers.IO) { repository.listSkills() } }
        catch (e: Exception) { error = userMessage(e) }
        finally { loading = false }
    } }
    fun keep(skill: Skill) { skills = skills.map { if (it.id == skill.id) skill else it } }
    fun say(text: String) { scope.launch { snackbar.currentSnackbarData?.dismiss(); snackbar.showSnackbar(text) } }
    LaunchedEffect(Unit) { refresh() }
    val open = skills.firstOrNull { it.id == openId }
    BackHandler(enabled = openId != null) { openId = null }

    if (open != null && editing) EditSkillDialog(open, onDismiss = { editing = false }, onSave = { name, description, body ->
        // Only the fields that changed go out, so an unchanged name never clashes with itself.
        val saved = withContext(Dispatchers.IO) {
            repository.updateSkill(open.id, name.takeIf { it != open.name }, description.takeIf { it != open.description },
                body.takeIf { it != open.body })
        }
        keep(saved); editing = false; say("Saved")
    })
    if (open != null && confirmDelete) AlertDialog(
        onDismissRequest = { confirmDelete = false },
        confirmButton = { ChunkyButton(onClick = {
            confirmDelete = false
            scope.launch {
                try {
                    withContext(Dispatchers.IO) { repository.deleteSkill(open.id) }
                    skills = skills.filter { it.id != open.id }; openId = null; say("Skill deleted")
                } catch (e: Exception) { error = userMessage(e) }
            }
        }, color = Coral, contentColor = Color.White) { Text("Delete", fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancel") } },
        title = { Text(displayText("Delete this skill?"), style = displayStyle(18.sp)) },
        text = { Text("Beep forgets how you like \"${open.name}\" done. It may learn it again if you keep asking for it.", color = TextMuted) },
        containerColor = Surface)

    Scaffold(containerColor = Ink, snackbarHost = {
        SnackbarHost(snackbar) { Snackbar(it, actionColor = Beep.colors.ok, shape = RoundedCornerShape(Beep.shapes.smallRadius)) }
    }, topBar = {
        if (open == null) TopAppBar(modifier = Modifier.beepBar(), expandedHeight = 54.dp, title = { Text(displayText("Skills"), style = displayStyle(18.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
        else TopAppBar(modifier = Modifier.beepBar(), expandedHeight = 54.dp, title = { Text(open.name, style = displayStyle(18.sp), maxLines = 1, overflow = TextOverflow.Ellipsis) },
            navigationIcon = { IconButton(onClick = { openId = null }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = {
                IconButton(onClick = { editing = true }) { Icon(Icons.Outlined.Edit, "Edit") }
                IconButton(onClick = { confirmDelete = true }) { Icon(Icons.Outlined.Delete, "Delete", tint = Coral) }
            },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (open != null) SkillDetail(open, onToggle = { enabled ->
                keep(open.copy(enabled = enabled))
                scope.launch {
                    try { keep(withContext(Dispatchers.IO) { repository.updateSkill(open.id, enabled = enabled) }); say(if (enabled) "Resumed" else "Paused") }
                    catch (e: Exception) { keep(open); error = userMessage(e) }
                }
            }, onUndo = {
                scope.launch {
                    try { keep(withContext(Dispatchers.IO) { repository.undoSkill(open.id) }); say("Back to the earlier version") }
                    catch (e: Exception) { error = userMessage(e) }
                }
            })
            else if (loading && skills.isEmpty()) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (skills.isEmpty()) Text("No skills yet.\nWhen Beep has to work out how you like something done, like booking a court or a weekly summary, it writes it down here and does it your way next time.",
                fontSize = 13.sp, color = TextMuted, lineHeight = 22.sp, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = TextAlign.Center)
            else SkillList(skills) { openId = it.id }
        }
    }
}

@Composable
private fun SkillList(skills: List<Skill>, onOpen: (Skill) -> Unit) {
    val yours = skills.filter { it.groupId == null }
    val groups = skills.filter { it.groupId != null }.groupBy { it.groupId }.values.sortedBy { it.first().groupName.orEmpty().lowercase() }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item(key = "intro") {
            Text("Beep writes these from how you ask for things, and updates them when you correct it.", fontSize = 13.sp, lineHeight = 18.sp,
                color = TextMuted, modifier = Modifier.padding(horizontal = 4.dp))
        }
        if (yours.isNotEmpty()) {
            item(key = "section-yours") { SectionLabel("Yours") }
            items(yours, key = { it.id }) { SkillCard(it) { onOpen(it) } }
        }
        groups.forEach { list ->
            item(key = "section-${list.first().groupId}") { SectionLabel(list.first().groupName ?: "Group") }
            items(list, key = { it.id }) { SkillCard(it) { onOpen(it) } }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(text, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted, modifier = Modifier.padding(start = 4.dp, top = 8.dp))
}

@Composable
private fun SkillCard(skill: Skill, onClick: () -> Unit) {
    BeepCard(Modifier.fillMaxWidth().alpha(if (skill.enabled) 1f else 0.65f).clickable(onClick = onClick)) {
        Row(Modifier.padding(BeepSpace.row), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(skill.name, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(skill.description, fontSize = 12.sp, lineHeight = 17.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                val state = if (skill.enabled) usage(skill) else "Paused"
                Text(state + if (skill.edited) " · edited by you" else "", fontSize = 12.sp, color = TextDim, modifier = Modifier.padding(top = 2.dp))
            }
            Spacer(Modifier.width(12.dp))
            Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = TextMuted)
        }
    }
}

@Composable
private fun SkillDetail(skill: Skill, onToggle: (Boolean) -> Unit, onUndo: () -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)) {
        skill.groupName?.let { group ->
            Text(group, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.clip(CircleShape).background(Beep.colors.tints[2])
                .border(1.5.dp, Beep.colors.line, CircleShape).padding(horizontal = 8.dp, vertical = 2.dp))
        }
        if (skill.description.isNotBlank()) Text(skill.description, fontSize = 14.sp, lineHeight = 20.sp, color = TextMuted)
        BeepCard(Modifier.fillMaxWidth()) {
            Row(Modifier.padding(BeepSpace.row), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Use this skill", fontSize = 14.sp, fontWeight = FontWeight.Medium)
                    Text(if (skill.enabled) usage(skill) else "Paused: Beep works it out each time", fontSize = 12.sp, color = TextMuted)
                }
                BeepSwitch(checked = skill.enabled, onCheckedChange = onToggle, label = "Use this skill")
            }
        }
        BeepCard(Modifier.fillMaxWidth()) {
            Box(Modifier.padding(horizontal = 12.dp, vertical = 10.dp)) { MessageContent(skill.body, "assistant", compact = true, padded = false) }
        }
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.radius)).background(Beep.colors.tints[1])
            .border(1.5.dp, Beep.colors.line, RoundedCornerShape(Beep.shapes.radius)).padding(start = 12.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.AutoAwesome, null, Modifier.size(20.dp))
            Spacer(Modifier.width(10.dp))
            val day = localDay(skill.updatedAt)?.let(::onDay) ?: "recently"
            Text(if (skill.edited) "You edited this $day." else "Beep updated this $day.", fontSize = 13.sp, lineHeight = 18.sp,
                modifier = Modifier.weight(1f).padding(vertical = 6.dp))
            if (skill.canUndo) TextButton(onClick = onUndo, contentPadding = PaddingValues(horizontal = 8.dp)) { Text("Undo", color = Moss) }
        }
    }
}

/** Edits a skill's three texts. [onSave] throws to keep the dialog open; a name clash shows here. */
@Composable
private fun EditSkillDialog(skill: Skill, onDismiss: () -> Unit, onSave: suspend (String, String, String) -> Unit) {
    var name by remember(skill.id) { mutableStateOf(skill.name) }
    var description by remember(skill.id) { mutableStateOf(skill.description) }
    var body by remember(skill.id) { mutableStateOf(skill.body) }
    var error by remember { mutableStateOf("") }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val fieldColors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
        unfocusedBorderColor = Border, focusedBorderColor = Moss)
    AlertDialog(onDismissRequest = onDismiss, containerColor = Surface,
        title = { Text(displayText("Edit skill"), style = displayStyle(18.sp)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(name, { name = it; error = "" }, label = { Text("Name") }, singleLine = true, modifier = Modifier.fillMaxWidth(), colors = fieldColors)
            OutlinedTextField(description, { description = it }, label = { Text("When it applies") }, maxLines = 3, modifier = Modifier.fillMaxWidth(), colors = fieldColors)
            OutlinedTextField(body, { body = it }, label = { Text("Instructions") }, minLines = 7, maxLines = 14, modifier = Modifier.fillMaxWidth(),
                textStyle = LocalTextStyle.current.copy(fontSize = 13.sp, lineHeight = 19.sp), colors = fieldColors)
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
        } },
        confirmButton = { ChunkyButton(onClick = {
            saving = true
            scope.launch {
                try { onSave(name.trim(), description.trim(), body.trim()) }
                catch (e: BeepException.Conflict) { error = "You already have a skill with that name" }
                catch (e: Exception) { error = userMessage(e) }
                finally { saving = false }
            }
        }, color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
            enabled = !saving && name.isNotBlank() && body.isNotBlank()) { Text("Save", fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } })
}

/** "Used yesterday · 6 uses", or "Not used yet". */
private fun usage(skill: Skill): String {
    val day = skill.lastUsed?.let(::localDay) ?: return "Not used yet"
    val uses = if (skill.uses == 1u) "1 use" else "${skill.uses} uses"
    return "Used ${relativeDay(day)} · $uses"
}

private fun localDay(rfc3339: String): LocalDate? =
    runCatching { OffsetDateTime.parse(rfc3339).atZoneSameInstant(ZoneId.systemDefault()).toLocalDate() }.getOrNull()

/** "today", "yesterday", "Sunday" within the week, then "2 Oct", with the year when it is not this year. */
private fun relativeDay(day: LocalDate, today: LocalDate = LocalDate.now()): String = when {
    !day.isBefore(today) -> "today"
    day == today.minusDays(1) -> "yesterday"
    day.isAfter(today.minusDays(7)) -> day.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())
    day.year == today.year -> day.format(DateTimeFormatter.ofPattern("d MMM"))
    else -> day.format(DateTimeFormatter.ofPattern("d MMM yyyy"))
}

/** [relativeDay] after "this": "today", "yesterday", "on Sunday", "on 2 Oct". */
private fun onDay(day: LocalDate): String = relativeDay(day).let { if (it == "today" || it == "yesterday") it else "on $it" }
