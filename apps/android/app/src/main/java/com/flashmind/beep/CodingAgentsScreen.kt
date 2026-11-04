package com.flashmind.beep

import androidx.compose.foundation.layout.*
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
        actions = { TextButton(onClick = { starting = true }) { Text("Start", fontSize = 12.sp) } }) {
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
    WorkspacePage("Agent session", onBack, data::refresh) {
        WorkspaceBody {
            WorkspaceHeading(shown.title ?: shown.agent)
            WorkspaceFacts(listOf("Agent" to shown.agent, "Machine" to shown.machineName, "Status" to shown.status.workspaceLabel(), "Folder" to shown.cwd))
            WorkspaceError(error)
            WorkspaceError(data.error, data::refresh)
            WorkspaceError(view?.error)
            usage.value?.let { used -> WorkspaceFacts(listOf("Tokens" to used.tokens.toString(),
                "Estimated cost" to (used.costUsd?.let { String.format(java.util.Locale.ROOT, "$%.2f", it) } ?: "Not reported"))) }
            if (data.loading && view == null) WorkspaceLoading()
            if (shown.external) OutlinedButton(onClick = { act { session = api.resumeCoding(shown.machineId, shown.agent, shown.id).session } }, enabled = !busy) { Text("Continue in Beep") }
            shown.question?.let { question ->
                BeepCard(Modifier.fillMaxWidth()) {
                    WorkspaceHeading("Needs input")
                    WorkspaceCode(question.text + question.detail?.let { "\n$it" }.orEmpty())
                    question.options.forEach { option -> TextButton(onClick = {
                        act { api.answerCoding(shown.machineId, shown.agent, shown.id, question.id, option.id) }
                    }, enabled = !busy) { Text(option.name, color = if (option.allow) Beep.colors.brand else Beep.colors.muted) } }
                }
            }
            if (!view?.plan.isNullOrEmpty()) {
                WorkspaceHeading("Plan")
                BeepCard { view!!.plan.forEach { step -> WorkspaceRow(Icons.Outlined.Checklist, step.content, tag = step.status.workspaceLabel()) } }
            }
            WorkspaceHeading("Session log")
            if ((view?.earlier ?: 0u) > 0u) Text("${view!!.earlier} earlier entries omitted", fontSize = 12.sp, color = Beep.colors.muted)
            view?.entries?.forEachIndexed { index, entry ->
                var expanded by remember(shown.id, index) { mutableStateOf(entry.kind == AgentEntryKind.User || entry.kind == AgentEntryKind.Agent || entry.kind == AgentEntryKind.Question) }
                BeepCard(Modifier.fillMaxWidth()) {
                    WorkspaceRow(Icons.Outlined.Code, entryTitle(entry.kind), entry.text.lineSequence().firstOrNull().orEmpty().take(120), onClick = { expanded = !expanded })
                    if (expanded) {
                        if (entry.kind == AgentEntryKind.Tool || entry.kind is AgentEntryKind.Thought) WorkspaceCode(entry.text)
                        else WorkspaceProse(entry.text)
                    }
                }
            }
            WorkspaceHeading("Changes", "Review diff") { diff = true }
            BeepCard { view?.files?.forEach { file -> WorkspaceRow(Icons.AutoMirrored.Outlined.InsertDriveFile, file, onClick = { diff = true }) } }
            if (!shown.external && shown.status != AgentStatus.ENDED) {
                OutlinedTextField(message, { message = it }, Modifier.fillMaxWidth(), label = { Text("Message agent") }, minLines = 2)
                Button(onClick = { val text = message.trim(); act {
                    api.steerCoding(shown.machineId, shown.agent, shown.id, AgentSteer.Prompt(text)); message = ""
                } }, enabled = !busy && message.isNotBlank()) { Text("Send") }
                Row {
                    TextButton(onClick = { act { api.steerCoding(shown.machineId, shown.agent, shown.id, AgentSteer.Cancel) } }, enabled = !busy) { Text("Stop turn") }
                    TextButton(onClick = { close = true }, enabled = !busy) { Text("Close session", color = Beep.colors.error) }
                }
            }
        }
    }
}

private fun entryTitle(kind: AgentEntryKind): String = when (kind) {
    AgentEntryKind.User -> "You"
    AgentEntryKind.Agent -> "Agent"
    AgentEntryKind.Tool -> "Tool"
    AgentEntryKind.Question -> "Question"
    is AgentEntryKind.Thought -> "Thought"
    else -> "Session"
}
