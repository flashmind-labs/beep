package com.flashmind.beep

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.CommandStart
import com.flashmind.beep.ffi.TerminalConnection
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CancellationException
import com.flashmind.beep.proto.Terminal
import kotlinx.coroutines.launch

@Composable
internal fun TerminalsScreen(repository: ChatRepository, project: String? = null, machineId: String? = null, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val terminals = workspaceLoad("${project.orEmpty()}/${machineId.orEmpty()}", poll = true) { api.allTerminals() }
    var selected by remember { mutableStateOf<Terminal?>(null) }
    var creating by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    if (creating) { NewCommandScreen(repository, project, onBack = { creating = false }) { creating = false; selected = it; terminals.refresh() }; return }
    selected?.let { terminal -> TerminalScreen(repository, terminal, onBack = { selected = null; terminals.refresh() }); return }
    WorkspacePage("Terminals", onBack, terminals::refresh, actions = { TextButton(onClick = { creating = true }) { Text("New terminal", fontSize = 12.sp) } }) {
        WorkspaceBody {
            WorkspaceSearch(query, "Filter terminals") { query = it }
            WorkspaceError(terminals.error, terminals::refresh)
            if (terminals.loading && terminals.value == null) WorkspaceLoading()
            val filtered = terminals.value.orEmpty().filter { (project == null || it.project == project) && (machineId == null || it.machineId == machineId) && (it.name + it.machineName + it.command).contains(query, true) }
            BeepCard(Modifier.fillMaxWidth()) { filtered.forEach { terminal -> WorkspaceRow(Icons.Outlined.Terminal,
                terminal.name.ifEmpty { terminal.command }, "${terminal.machineName} · ${terminal.cwd}", terminalState(terminal), onClick = { selected = terminal }) } }
            if (filtered.isEmpty() && !terminals.loading && terminals.error == null) WorkspaceEmpty("No matching terminals")
        }
    }
}

@Composable
private fun NewCommandScreen(repository: ChatRepository, project: String?, onBack: () -> Unit, onStarted: (Terminal) -> Unit) {
    val api = repository.workspaceApi()
    val machines = workspaceLoad(repository) { repository.listMachines().filter { it.online } }
    val projectMachines = workspaceLoad(project) { if (project == null) emptyList() else api.projectMachines(project) }
    var machineId by rememberSaveable { mutableStateOf("") }
    var command by rememberSaveable { mutableStateOf("") }
    var folder by rememberSaveable { mutableStateOf("") }
    var name by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val machine = machines.value?.find { it.id == machineId } ?: machines.value?.firstOrNull()
    val cwd = folder.trim().ifEmpty { projectMachines.value?.find { it.machineId == machine?.id }?.folder.orEmpty() }
    WorkspacePage("New terminal", onBack) {
        WorkspaceBody {
            OutlinedTextField(command, { command = it }, Modifier.fillMaxWidth(), label = { Text("Command (optional)") }, placeholder = { Text("Shell") }, minLines = 3)
            WorkspaceChoice("Machine", machine?.name.orEmpty(), machines.value.orEmpty().map { it.name }) { machineId = machines.value!![it].id }
            OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth(), label = { Text("Name (optional)") }, singleLine = true)
            OutlinedTextField(folder, { folder = it }, Modifier.fillMaxWidth(), label = { Text("Folder (optional)") }, placeholder = { Text(cwd.ifEmpty { "Machine default" }) }, singleLine = true)
            WorkspaceError(machines.error, machines::refresh)
            WorkspaceError(projectMachines.error, projectMachines::refresh)
            WorkspaceError(error)
            if (machines.value?.isEmpty() == true) WorkspaceEmpty("No online machines")
            Button(onClick = { scope.launch {
                busy = true
                workspaceAction({ error = it }) { onStarted(api.startCommand(CommandStart(project.orEmpty(), machine!!.id, name.trim(), command, cwd))) }
                busy = false
            } }, enabled = !busy && machine != null) { Text(if (busy) "Starting..." else if (command.isBlank()) "Open shell" else "Run") }
        }
    }
}

@Composable
private fun TerminalScreen(repository: ChatRepository, initial: Terminal, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val output = workspaceLoad(initial.id, poll = true) { api.terminalOutput(initial.id) }
    var closing by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val terminal = output.value?.terminal ?: initial
    val owner = LocalLifecycleOwner.current
    var connection by remember(initial.id) { mutableStateOf<TerminalConnection?>(null) }
    var command by rememberSaveable(initial.id) { mutableStateOf("") }
    var interactive by remember(initial.id) { mutableStateOf(false) }
    var liveOutput by remember(initial.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(initial.id, owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            var stream: TerminalConnection? = null
            try {
                val transcript = TerminalText()
                stream = api.attachTerminal(initial.id)
                connection = stream
                interactive = true
                while (true) {
                    val frame = stream.next() ?: break
                    if (frame.data.isNotEmpty()) liveOutput = transcript.append(frame.data)
                    if (frame.viewOnly) { interactive = false; break }
                    if (frame.ended) break
                }
                output.refresh()
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { error = userMessage(e) }
            finally { interactive = false; connection = null; stream?.destroy() }
        }
    }
    fun keys(text: String, clear: Boolean = false) { scope.launch {
        val stream = connection ?: return@launch
        busy = true
        workspaceAction({ error = it }) { stream.keys(text); if (clear) command = ""; output.refresh() }
        busy = false
    } }
    if (closing) AlertDialog(onDismissRequest = { closing = false }, title = { Text("Stop terminal?") },
        text = { Text("The command and its child processes will stop.") },
        confirmButton = { TextButton(onClick = { closing = false; scope.launch {
            busy = true
            workspaceAction({ error = it }) { api.closeTerminal(terminal.id); output.refresh() }
            busy = false
        } }) { Text("Stop terminal") } }, dismissButton = { TextButton(onClick = { closing = false }) { Text("Cancel") } })
    WorkspacePage(terminal.name.ifEmpty { "Terminal" }, onBack, output::refresh, actions = {
        TextButton(onClick = { closing = true }, enabled = !busy && terminal.state is com.flashmind.beep.proto.TerminalState.Running) { Text("Stop", color = Beep.colors.error) }
    }) {
        WorkspaceBody(Modifier.weight(1f)) {
            WorkspaceFacts(listOf("Machine" to terminal.machineName, "Status" to terminalState(terminal), "Folder" to terminal.cwd, "Command" to terminal.command))
            WorkspaceError(output.error, output::refresh)
            WorkspaceError(error)
            if (output.loading && output.value == null) WorkspaceLoading()
            if (output.value?.earlier == true) Text("Earlier output omitted", fontSize = 12.sp, color = Beep.colors.muted)
            BeepCard(Modifier.fillMaxWidth()) { WorkspaceCode(liveOutput ?: output.value?.lines?.joinToString("\n")?.ifEmpty { "Waiting for output" } ?: "Waiting for output") }
        }
        if (interactive) {
            HorizontalDivider(color = Beep.colors.softLine)
            Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(command, { command = it }, Modifier.weight(1f), singleLine = true, label = { Text("Command") }, enabled = !busy)
                    Button(onClick = { keys(command + "\r", clear = true) }, enabled = !busy && command.isNotBlank()) { Text("Run") }
                }
                Row {
                    TextButton(onClick = { keys("\u0003") }, enabled = !busy) { Text("Ctrl+C") }
                    TextButton(onClick = { keys("\u0004") }, enabled = !busy) { Text("Ctrl+D") }
                }
            }
        }
    }
}

private fun terminalState(terminal: Terminal): String = when (val state = terminal.state) {
    com.flashmind.beep.proto.TerminalState.Running -> "Running"
    is com.flashmind.beep.proto.TerminalState.Exited -> state.code?.let { "Exited ($it)" } ?: "Exited"
    com.flashmind.beep.proto.TerminalState.Stopped -> "Stopped"
    com.flashmind.beep.proto.TerminalState.Offline -> "Offline"
    com.flashmind.beep.proto.TerminalState.Gone -> "Gone"
}
