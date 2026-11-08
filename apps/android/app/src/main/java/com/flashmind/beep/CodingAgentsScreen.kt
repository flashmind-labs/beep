package com.flashmind.beep

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.CodingStart
import com.flashmind.beep.proto.*
import kotlinx.coroutines.launch

@Composable
internal fun CodingAgentsScreen(repository: ChatRepository, project: String? = null, initialPrompt: String = "",
    startImmediately: Boolean = false, initialSession: MachineSession? = null, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val sessions = workspaceLoad(project ?: "all-sessions", poll = true) { api.codingSessions(project) }
    val usage = workspaceLoad(repository) { api.codingUsage() }
    var starting by rememberSaveable { mutableStateOf(startImmediately) }
    var selected by remember(initialSession?.id) { mutableStateOf(initialSession) }
    var query by rememberSaveable { mutableStateOf("") }
    val picked = selected
    if (picked != null) { CodingSessionScreen(repository, picked, onBack = { if (initialSession != null) onBack() else { selected = null; sessions.refresh() } }); return }
    if (starting) { CodingStartScreen(repository, project, initialPrompt, onBack = { starting = false }) {
        starting = false; selected = it; sessions.refresh()
    }; return }
    WorkspacePage("Coding agents", onBack, sessions::refresh,
        actions = { TextButton(onClick = { starting = true }) { Text("Start", fontSize = 12.sp) } }, root = LocalTabRoot.current) {
        WorkspaceBody {
            WorkspaceSearch(query, "Filter sessions") { query = it }
            WorkspaceHeading("Sessions")
            WorkspaceError(sessions.error, sessions::refresh)
            if (sessions.loading && sessions.value == null) WorkspaceLoading()
            val filtered = sessions.value.orEmpty().filter { (it.title.orEmpty() + it.agent + it.machineName + it.cwd).contains(query, true) }
            BeepCard(Modifier.fillMaxWidth()) { filtered.forEach { session ->
                WorkspaceRow(Icons.Outlined.Code, session.title?.ifEmpty { null } ?: session.agent,
                    "${session.agent} · ${session.machineName}", session.status.workspaceLabel(), onClick = { selected = session },
                    tagColor = if (session.status == AgentStatus.ASKING) Beep.colors.warn else Beep.colors.brand)
            } }
            if (filtered.isEmpty() && !sessions.loading && sessions.error == null) WorkspaceEmpty("No matching coding sessions")
            WorkspaceHeading("Usage today")
            WorkspaceError(usage.error, usage::refresh)
            usage.value?.let { today ->
                WorkspaceFacts(listOf("Estimated cost" to String.format(java.util.Locale.ROOT, "$%.2f", today.costUsd), "Tokens" to today.tokens.toString()))
                BeepCard { today.limits.forEach { limit -> WorkspaceRow(Icons.Outlined.Code, limit.agent,
                    if (limit.minutes < 60u) "${limit.minutes} minutes" else "${limit.minutes / 60u} hours", "${limit.usedPercent.toInt()}% used") } }
            }
        }
    }
}

@Composable
private fun CodingStartScreen(repository: ChatRepository, project: String?, initialPrompt: String,
    onBack: () -> Unit, onStarted: (MachineSession) -> Unit) {
    val api = repository.workspaceApi()
    val hosts = workspaceLoad(repository) { api.codingHosts() }
    val projects = workspaceLoad(repository) { api.listImportedProjects() }
    var pickedProject by rememberSaveable { mutableStateOf(project.orEmpty()) }
    var machineId by rememberSaveable { mutableStateOf("") }
    var agentId by rememberSaveable { mutableStateOf("") }
    var picking by rememberSaveable { mutableStateOf(false) }
    var prompt by rememberSaveable { mutableStateOf(initialPrompt) }
    var folder by rememberSaveable { mutableStateOf("") }
    var branch by rememberSaveable { mutableStateOf("") }
    var worktree by rememberSaveable { mutableStateOf(true) }
    var model by rememberSaveable { mutableStateOf("") }
    var effortName by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val host = hosts.value?.find { it.machineId == machineId } ?: hosts.value?.firstOrNull()
    val agent = host?.agents?.find { it == agentId } ?: host?.agents?.firstOrNull()
    val models = workspaceLoad("${host?.machineId}/$agent") { if (host != null && agent != null) api.codingModels(host.machineId, agent) else null }
    val projectChoices = projects.value.orEmpty().map { it.project }.distinct()
    val imported = projects.value?.find { it.project == pickedProject && it.machineId == host?.machineId }
    val projectMachines = workspaceLoad(pickedProject) { if (pickedProject.isNotEmpty()) api.projectMachines(pickedProject) else emptyList() }
    val projectFolder = projectMachines.value?.find { it.machineId == host?.machineId }?.folder ?: imported?.folder
    val cwd = folder.trim().ifEmpty { projectFolder ?: host?.workspace.orEmpty() }
    val efforts = when (agent) { "claude" -> listOf("Low", "Medium", "High", "Xhigh", "Max"); "codex" -> listOf("Low", "Medium", "High"); "pi" -> listOf("Off", "Minimal", "Low", "Medium", "High", "Xhigh", "Max"); else -> emptyList() }
    LaunchedEffect(host?.machineId, agent) { model = ""; effortName = "" }
    if (picking) {
        WorkspacePage("Choose coding agent", { picking = false }) {
            WorkspaceBody {
                WorkspaceHeading(host?.machineName?.let { "Available on $it" } ?: "Available agents")
                BeepCard { host?.agents.orEmpty().forEach { choice ->
                    WorkspaceRow(Icons.Outlined.Code, choice, "Default model", if (choice == agent) "Selected" else null,
                        onClick = { agentId = choice; model = ""; effortName = ""; picking = false })
                } }
                WorkspaceHeading("Run settings")
                if (efforts.isNotEmpty()) WorkspaceChoice("Thinking", effortName.ifEmpty { "Default" }, listOf("Default") + efforts) {
                    effortName = (listOf("Default") + efforts)[it].takeUnless { it == "Default" }.orEmpty()
                }
            }
        }
        return
    }
    WorkspacePage("Start coding agent", onBack) {
        WorkspaceBody {
            OutlinedTextField(prompt, { prompt = it }, Modifier.fillMaxWidth(), label = { Text("Task") }, minLines = 4, maxLines = 8)
            WorkspaceChoice("Project", pickedProject.ifEmpty { "No project" }, listOf("No project") + projectChoices) {
                pickedProject = if (it == 0) "" else projectChoices[it - 1]; folder = ""
            }
            WorkspaceChoice("Machine", host?.machineName.orEmpty(), hosts.value.orEmpty().map { it.machineName }) { machineId = hosts.value!![it].machineId; folder = "" }
            Text("Coding agent", fontSize = 12.sp, color = Beep.colors.muted)
            BeepCard { WorkspaceRow(Icons.Outlined.Code, agent ?: "No coding agent available",
                listOf(model.ifEmpty { "Default model" }, effortName.ifEmpty { "Default thinking" }).joinToString(" · "),
                "Change", onClick = if (host?.agents.isNullOrEmpty()) null else { { picking = true } }) }
            OutlinedTextField(folder, { folder = it }, Modifier.fillMaxWidth(), label = { Text("Folder") }, placeholder = { Text(cwd.ifEmpty { "Machine default" }) }, singleLine = true)
            if (pickedProject.isNotEmpty()) {
                WorkspaceChoice("Checkout", if (worktree) "New worktree" else "Current checkout", listOf("New worktree", "Current checkout")) { worktree = it == 0 }
                if (worktree) OutlinedTextField(branch, { branch = it }, Modifier.fillMaxWidth(), label = { Text("Branch (optional)") }, singleLine = true)
            }
            val choices = models.value
            if (choices?.pick == AgentModelPick.NAMED) OutlinedTextField(model, { model = it }, Modifier.fillMaxWidth(), label = { Text("Model (optional)") }, singleLine = true)
            if (choices?.pick == AgentModelPick.LISTED) WorkspaceChoice("Model", model.ifEmpty { "Default" }, listOf("Default") + choices.models.map { it.name }) {
                model = if (it == 0) "" else choices.models[it - 1].id
            }
            if (efforts.isNotEmpty()) WorkspaceChoice("Thinking", effortName.ifEmpty { "Default" }, listOf("Default") + efforts) {
                effortName = (listOf("Default") + efforts)[it].takeUnless { it == "Default" }.orEmpty()
            }
            WorkspaceError(hosts.error, hosts::refresh)
            WorkspaceError(projects.error, projects::refresh)
            WorkspaceError(projectMachines.error, projectMachines::refresh)
            WorkspaceError(models.error, models::refresh)
            WorkspaceError(error)
            if (hosts.value?.isEmpty() == true) WorkspaceEmpty("No online machine offers a coding agent.")
            Button(onClick = { scope.launch {
                busy = true
                workspaceAction({ error = it }) {
                    val checkout = if (pickedProject.isEmpty() || !worktree) null else if (branch.isBlank()) AgentCheckoutPlan.NewWorktree(null) else AgentCheckoutPlan.NamedWorktree(null, branch.trim())
                    val effort = AgentEffort.entries.find { it.workspaceLabel() == effortName }
                    onStarted(api.startCoding(CodingStart(host!!.machineId, agent!!, cwd.ifEmpty { null }, prompt.trim(), model.ifEmpty { null }, effort, checkout)).session)
                }
                busy = false
            } }, enabled = !busy && prompt.isNotBlank() && host != null && agent != null && (pickedProject.isEmpty() || cwd.isNotBlank()), modifier = Modifier.fillMaxWidth()) {
                Text(if (busy) "Starting..." else "Start agent")
            }
        }
    }
}

@Composable
private fun CodingSessionScreen(repository: ChatRepository, initial: MachineSession, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    var session by remember { mutableStateOf(initial) }
    val data = workspaceLoad("${session.machineId}/${session.agent}/${session.id}", poll = true) {
        api.codingSession(session.machineId, session.agent, session.id)
    }
    val usage = workspaceLoad("usage/${session.machineId}/${session.id}", poll = true) {
        api.codingSessionUsage(session.machineId, session.agent, session.id)
    }
    var message by rememberSaveable(session.id) { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var close by remember { mutableStateOf(false) }
    var diff by rememberSaveable(session.id) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun act(action: suspend () -> Unit) { scope.launch {
        if (busy) return@launch
        busy = true
        workspaceAction({ error = it }) { action(); data.refresh() }
        busy = false
    } }
    if (diff) {
        val changes = workspaceLoad("${session.machineId}/${session.cwd}") { api.sessionDiff(session.machineId, session.cwd) }
        WorkspacePage("Changes", { diff = false }, changes::refresh) {
            WorkspaceBody { WorkspaceError(changes.error, changes::refresh); if (changes.loading && changes.value == null) WorkspaceLoading(); changes.value?.let { WorkspaceDiff(it) } }
        }
        return
    }
    val view = data.value
    val shown = view?.session ?: session
    if (close) AlertDialog(onDismissRequest = { close = false }, title = { Text("Close session?") }, text = { Text("The coding agent stops running.") },
        confirmButton = { TextButton(onClick = { close = false; act { api.steerCoding(shown.machineId, shown.agent, shown.id, AgentSteer.Close) } }) { Text("Close session") } },
        dismissButton = { TextButton(onClick = { close = false }) { Text("Cancel") } })
    val running = !shown.external && shown.status != AgentStatus.ENDED
    var menu by remember { mutableStateOf(false) }
    val list = rememberLazyListState()
    val count = view?.entries?.size ?: 0
    LaunchedEffect(count) { if (count > 0) list.scrollToItem(list.layoutInfo.totalItemsCount - 1) }
    WorkspacePage(shown.title?.ifEmpty { null } ?: shown.agent, onBack, data::refresh, actions = {
        if (running) {
            TextButton(onClick = { act { api.steerCoding(shown.machineId, shown.agent, shown.id, AgentSteer.Cancel) } }, enabled = !busy) {
                Text("Stop", color = Beep.colors.error, fontSize = 13.sp)
            }
            Box {
                IconButton(onClick = { menu = true }) { Icon(Icons.Outlined.MoreVert, "More") }
                DropdownMenu(menu, { menu = false }) {
                    DropdownMenuItem({ Text("Close session", color = Beep.colors.error) }, onClick = { menu = false; close = true })
                }
            }
        }
    }) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        val cost = usage.value?.let { used -> used.costUsd?.let { String.format(java.util.Locale.ROOT, "$%.2f", it) } ?: "${used.tokens} tokens" }
                        Text(listOfNotNull(shown.agent, shown.machineName, shown.cwd, cost).filter { it.isNotEmpty() }.joinToString(" · "),
                            Modifier.weight(1f), fontSize = 12.sp, color = Beep.colors.muted, maxLines = 2)
                        Text(shown.status.workspaceLabel(), fontSize = 11.sp,
                            color = if (shown.status == AgentStatus.ASKING) Beep.colors.warn else Beep.colors.muted)
                    }
                    WorkspaceError(error)
                    WorkspaceError(data.error, data::refresh)
                    if (data.loading && view == null) WorkspaceLoading()
                    if ((view?.earlier ?: 0u) > 0u) Text("${view!!.earlier} earlier entries", fontSize = 11.sp, color = Beep.colors.dim)
                }
            }
            itemsIndexed(view?.entries.orEmpty()) { index, entry -> TranscriptEntry(entry, remember(shown.id, index) { mutableStateOf(false) }) }
            item {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    WorkspaceError(view?.error)
                    if (!view?.plan.isNullOrEmpty()) BeepCard(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(11.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text("Plan", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
                            view!!.plan.forEachIndexed { i, step ->
                                Text("${i + 1}. ${step.content}", fontSize = 12.sp,
                                    color = if (step.status == AgentPlanStatus.IN_PROGRESS) Beep.colors.ink else Beep.colors.muted,
                                    textDecoration = if (step.status == AgentPlanStatus.COMPLETED) TextDecoration.LineThrough else null)
                            }
                        }
                    }
                    val files = view?.files.orEmpty()
                    if (files.isNotEmpty()) BeepCard(Modifier.fillMaxWidth()) {
                        WorkspaceRow(Icons.Outlined.Difference, if (files.size == 1) "1 file changed" else "${files.size} files changed", tag = "Review", onClick = { diff = true })
                    }
                }
            }
        }
        shown.question?.let { question ->
            Column(Modifier.padding(horizontal = 12.dp, vertical = 6.dp).fillMaxWidth()
                .background(Beep.colors.surface, RoundedCornerShape(12.dp))
                .border(1.dp, Beep.colors.warn.copy(alpha = 0.5f), RoundedCornerShape(12.dp)).padding(11.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(question.text, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                question.detail?.let { Text(it, fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = Beep.colors.muted, maxLines = 6) }
                question.options.forEach { option ->
                    val onClick = { act { api.answerCoding(shown.machineId, shown.agent, shown.id, question.id, option.id) } }
                    if (option.allow) Button(onClick, Modifier.fillMaxWidth(), enabled = !busy) { Text(option.name) }
                    else OutlinedButton(onClick, Modifier.fillMaxWidth(), enabled = !busy) { Text(option.name) }
                }
            }
        }
        if (shown.external) Button(onClick = { act { session = api.resumeCoding(shown.machineId, shown.agent, shown.id).session } },
            Modifier.fillMaxWidth().padding(12.dp), enabled = !busy) { Text("Resume session") }
        else if (running) Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp).fillMaxWidth()
            .background(Beep.colors.surface, RoundedCornerShape(18.dp))
            .border(1.dp, Beep.colors.softLine, RoundedCornerShape(18.dp)).padding(start = 14.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
            verticalAlignment = Alignment.Bottom) {
            Box(Modifier.weight(1f).padding(vertical = 7.dp)) {
                if (message.isEmpty()) Text("Message ${shown.agent}", color = Beep.colors.dim, fontSize = 14.sp)
                BasicTextField(message, { message = it }, Modifier.fillMaxWidth(), maxLines = 5,
                    textStyle = TextStyle(color = Beep.colors.ink, fontSize = 14.sp), cursorBrush = SolidColor(Beep.colors.accent))
            }
            IconButton(onClick = { val text = message.trim(); act {
                api.steerCoding(shown.machineId, shown.agent, shown.id, AgentSteer.Prompt(text)); message = ""
            } }, Modifier.size(34.dp).background(if (message.isBlank()) Beep.colors.raised else Beep.colors.accent, CircleShape),
                enabled = !busy && message.isNotBlank()) {
                Icon(Icons.Outlined.ArrowUpward, "Send", Modifier.size(18.dp), tint = Beep.colors.onAccent)
            }
        }
    }
}

/** One transcript entry as the mock draws it: your bubble, the agent's markdown, a muted line per tool or thought. */
@Composable
private fun TranscriptEntry(entry: AgentEntry, open: MutableState<Boolean>) {
    when (val kind = entry.kind) {
        AgentEntryKind.User -> Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
            Spacer(Modifier.weight(1f).widthIn(min = 40.dp))
            Box(Modifier.widthIn(max = 300.dp).background(Beep.colors.accent.copy(alpha = 0.22f), RoundedCornerShape(12.dp, 12.dp, 4.dp, 12.dp))) {
                MessageContent(entry.text, role = "agent", compact = true, padded = true)
            }
        }
        AgentEntryKind.Agent -> MessageContent(entry.text, role = "agent", compact = true)
        AgentEntryKind.Tool, is AgentEntryKind.Thought -> Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth().clickable { open.value = !open.value }.padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (kind is AgentEntryKind.Thought) {
                    Text(kind.ms?.let { "Thought for ${maxOf(1uL, it / 1000uL)} s" } ?: "Thought", fontSize = 12.sp, color = Beep.colors.dim, fontStyle = FontStyle.Italic)
                } else {
                    val line = entry.text.lineSequence().firstOrNull().orEmpty()
                    val name = line.substringBefore(":")
                    val rest = line.substringAfter(":", "").trim()
                    Text(name.take(1).uppercase(), Modifier.size(18.dp).background(Beep.colors.raised, RoundedCornerShape(4.dp)).wrapContentHeight(),
                        fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = Beep.colors.muted, textAlign = TextAlign.Center)
                    Text(name, fontSize = 12.sp, color = Beep.colors.muted)
                    Text(rest, fontSize = 11.5.sp, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Medium, color = Beep.colors.ink,
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (open.value) WorkspaceCode(entry.text)
        }
        AgentEntryKind.Question -> Text(entry.text, fontSize = 12.sp, color = Beep.colors.warn)
        else -> Text(entry.text, fontSize = 12.sp, color = Beep.colors.dim)
    }
}
