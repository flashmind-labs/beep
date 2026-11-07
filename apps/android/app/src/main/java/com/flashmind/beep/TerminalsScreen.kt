package com.flashmind.beep

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.flashmind.beep.center.*
import com.flashmind.beep.ffi.BeepClient
import com.flashmind.beep.ffi.CommandStart
import com.flashmind.beep.ffi.TerminalConnection
import com.flashmind.beep.proto.Terminal
import com.flashmind.beep.proto.TerminalState
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch

/** Set by the command center to open a terminal as a full screen over the tabs. */
internal val LocalOpenTerminal = compositionLocalOf<((Terminal) -> Unit)?> { null }

@Composable
internal fun TerminalsScreen(repository: ChatRepository, project: String? = null, machineId: String? = null, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val terminals = workspaceLoad("${project.orEmpty()}/${machineId.orEmpty()}", poll = true) { api.allTerminals() }
    val machines = workspaceLoad(repository, poll = true) { repository.listMachines() }
    val open = LocalOpenTerminal.current
    var selected by remember { mutableStateOf<Terminal?>(null) }
    // The machine a new terminal starts on: "" lets the form pick one. Null while no form is open.
    var creating by rememberSaveable { mutableStateOf<String?>(null) }
    val show: (Terminal) -> Unit = { if (open != null) open(it) else selected = it }
    creating?.let { machine -> NewCommandScreen(repository, project, machine, onBack = { creating = null }) { creating = null; terminals.refresh(); show(it) }; return }
    selected?.let { terminal -> TerminalScreen(repository, terminal, onBack = { selected = null; terminals.refresh() }); return }
    val tabRoot = LocalTabRoot.current
    val inProject = terminals.value.orEmpty().filter { project == null || it.project == project }
    val known = machines.value.orEmpty().filter { machineId == null || it.id == machineId }
    val ids = (known.map { it.id } + inProject.map { it.machineId }.filter { machineId == null || it == machineId }).distinct()
    Column(Modifier.fillMaxSize().background(Beep.colors.page).statusBarsPadding()) {
        CcTop("Terminals", back = if (tabRoot) null else onBack) { CcIconButton(Icons.Outlined.Add, "New terminal") { creating = machineId.orEmpty() } }
        CcBody {
            WorkspaceError(terminals.error ?: machines.error) { terminals.refresh(); machines.refresh() }
            if (terminals.value == null && terminals.error == null) WorkspaceLoading()
            val groups = ids.mapNotNull { id ->
                val mine = inProject.filter { it.machineId == id }
                val machine = known.find { it.id == id }
                val online = machine?.online ?: false
                if (!online && mine.isEmpty()) null else Triple(machine?.name ?: mine.firstOrNull()?.machineName ?: id, online, mine) to id
            }
            if (terminals.value != null && groups.isEmpty()) CcEmpty("No terminals", "Open a shell on a linked machine.")
            groups.forEach { (group, id) ->
                val (name, online, mine) = group
                CcSection(name, mine.size, online = online, action = if (online) "New" else null, onAction = { creating = id }) {
                    if (mine.isNotEmpty()) CcPanel {
                        mine.forEach { terminal ->
                            CcRow(terminal.name.ifEmpty { "Shell" }, terminalSubtitle(terminal), chevron = online, onClick = if (online) ({ show(terminal) }) else null,
                                lead = { CcKindTile(">_") }, right = { CcTag(terminalState(terminal), if (terminal.state == TerminalState.Running) CcKind.Ok else CcKind.Plain) })
                        }
                    }
                }
            }
        }
    }
}

private fun terminalSubtitle(terminal: Terminal): String {
    val command = if (terminal.command.isEmpty()) "" else " · ${terminal.command}"
    return terminal.cwd.ifEmpty { terminal.project } + command
}

@Composable
private fun NewCommandScreen(repository: ChatRepository, project: String?, initialMachine: String, onBack: () -> Unit, onStarted: (Terminal) -> Unit) {
    val api = repository.workspaceApi()
    val machines = workspaceLoad(repository) { repository.listMachines().filter { it.online } }
    val projectMachines = workspaceLoad(project) { if (project == null) emptyList() else api.projectMachines(project) }
    var machineId by rememberSaveable { mutableStateOf(initialMachine) }
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
            WorkspaceChoice("Machine", machine?.name.orEmpty(), machines.value.orEmpty().map { it.name }) { machineId = machines.value!![it].id }
            OutlinedTextField(folder, { folder = it }, Modifier.fillMaxWidth(), label = { Text("Folder (optional)") }, placeholder = { Text(cwd.ifEmpty { "Machine default" }) }, singleLine = true)
            OutlinedTextField(command, { command = it }, Modifier.fillMaxWidth(), label = { Text("Command (optional)") }, placeholder = { Text("Shell") }, minLines = 3)
            OutlinedTextField(name, { name = it }, Modifier.fillMaxWidth(), label = { Text("Name (optional)") }, singleLine = true)
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

/** How the attached screen stands: the header bar and the key bar follow it. */
private sealed interface TerminalPhase {
    data object Connecting : TerminalPhase
    data object Attached : TerminalPhase
    data object ViewOnly : TerminalPhase
    data object Disconnected : TerminalPhase
    data class Ended(val text: String) : TerminalPhase
}

private sealed interface TerminalSend {
    data class Keys(val text: String) : TerminalSend
    data class Resize(val cols: Int, val rows: Int) : TerminalSend
}

/** The stream behind one terminal screen. Only the screen that runs it touches the connection. */
private class TerminalLink(private val api: BeepClient, private val id: String, private val emulator: TerminalEmulatorModel, initial: Terminal) {
    var terminal by mutableStateOf(initial)
        private set
    var phase by mutableStateOf<TerminalPhase>(TerminalPhase.Connecting)
        private set
    var error by mutableStateOf<String?>(null)
    private var connection: TerminalConnection? = null

    val attached get() = connection != null

    fun viewOnly() { phase = TerminalPhase.ViewOnly; emulator.viewOnly() }

    /** True when the stream is still there. Otherwise the caller attaches again. */
    fun takeControl(): Boolean {
        if (connection == null) { phase = TerminalPhase.Connecting; return false }
        phase = TerminalPhase.Attached
        emulator.takeControl()
        return true
    }

    private fun endedText(terminal: Terminal): String {
        val state = terminal.state
        return if (state is TerminalState.Exited && state.code != null) "Ended · exit ${state.code}" else "Ended"
    }

    suspend fun run() {
        error = null
        phase = TerminalPhase.Connecting
        emulator.reset()
        var stream: TerminalConnection? = null
        var next: TerminalPhase = TerminalPhase.Disconnected
        try {
            val snapshot = api.terminalOutput(id)
            terminal = snapshot.terminal
            if (snapshot.terminal.state != TerminalState.Running) {
                emulator.feed(snapshot.lines.joinToString("\r\n").toByteArray())
                phase = TerminalPhase.Ended(endedText(snapshot.terminal))
                return
            }
            val (cols, rows) = emulator.size() ?: return
            stream = api.attachTerminal(id, cols.coerceIn(1, 65535).toUShort(), rows.coerceIn(1, 65535).toUShort())
            connection = stream
            phase = TerminalPhase.Attached
            next = coroutineScope {
                // One sender keeps keys and resizes in order and ends with the stream.
                val outbox = Channel<TerminalSend>(Channel.UNLIMITED)
                val sender = launch {
                    for (item in outbox) {
                        try {
                            when (item) {
                                is TerminalSend.Keys -> stream.keys(item.text)
                                is TerminalSend.Resize -> stream.resize(item.cols.coerceIn(1, 65535).toUShort(), item.rows.coerceIn(1, 65535).toUShort())
                            }
                        } catch (e: CancellationException) { throw e }
                        catch (e: Exception) { error = userMessage(e) }
                    }
                }
                val utf8 = Utf8Input()
                emulator.connect(cols to rows, input = { bytes ->
                    utf8.text(bytes)?.takeIf { it.isNotEmpty() }?.let { outbox.trySend(TerminalSend.Keys(it)) }
                }, resize = { c, r -> outbox.trySend(TerminalSend.Resize(c, r)) })
                try {
                    var result: TerminalPhase = TerminalPhase.Disconnected
                    while (true) {
                        val frame = stream.next() ?: break
                        if (frame.ended) { result = TerminalPhase.Ended("Ended"); break }
                        if (frame.viewOnly) { result = TerminalPhase.ViewOnly; break }
                        emulator.feed(frame.data)
                    }
                    result
                } finally {
                    emulator.disconnect()
                    sender.cancel()
                    outbox.close()
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            error = userMessage(e)
        } finally {
            emulator.disconnect()
            emulator.viewOnly()
            connection = null
            stream?.destroy()
        }
        phase = next
        if (next is TerminalPhase.Ended) {
            try {
                val latest = api.terminalOutput(id).terminal
                terminal = latest
                phase = TerminalPhase.Ended(endedText(latest))
            } catch (e: CancellationException) { throw e } catch (_: Exception) {}
        }
    }
}

@Composable
internal fun TerminalScreen(repository: ChatRepository, initial: Terminal, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val emulator = remember(initial.id) { TerminalEmulatorModel(context) }
    val link = remember(initial.id) { TerminalLink(api, initial.id, emulator, initial) }
    var attempt by remember(initial.id) { mutableIntStateOf(0) }
    var confirming by remember { mutableStateOf(false) }
    var closing by remember { mutableStateOf(false) }
    BackHandler(onBack = onBack)
    DisposableEffect(emulator) { onDispose { emulator.close() } }
    // The stream is dropped when the app leaves the screen. A takeover by another device is not undone by coming back.
    LaunchedEffect(initial.id, attempt, owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            if (link.phase != TerminalPhase.ViewOnly) link.run()
        }
    }
    val phase = link.phase
    val ended = phase is TerminalPhase.Ended
    if (confirming) AlertDialog(onDismissRequest = { confirming = false }, title = { Text("Close terminal?") },
        text = { Text("The command will stop and the terminal will be removed.") },
        confirmButton = { TextButton(onClick = { confirming = false; closing = true; scope.launch {
            try { api.closeTerminal(initial.id); onBack() }
            catch (e: CancellationException) { throw e }
            catch (e: Exception) { link.error = userMessage(e) }
            closing = false
        } }) { Text("Close terminal", color = Beep.colors.error) } }, dismissButton = { TextButton(onClick = { confirming = false }) { Text("Cancel") } })
    Column(Modifier.fillMaxSize().background(Beep.colors.page).systemBarsPadding().imePadding()) {
        CcTop(link.terminal.name.ifEmpty { "Shell" }, back = onBack)
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            CcDot(online = phase == TerminalPhase.Attached || phase == TerminalPhase.ViewOnly || phase == TerminalPhase.Connecting)
            Text(when (phase) {
                TerminalPhase.Connecting -> "Connecting"
                TerminalPhase.Attached -> "Attached · ${emulator.cols}×${emulator.rows}"
                TerminalPhase.ViewOnly -> "View only"
                TerminalPhase.Disconnected -> "Disconnected"
                is TerminalPhase.Ended -> phase.text
            }, Modifier.weight(1f), fontSize = 12.sp, color = Beep.colors.muted, maxLines = 1)
            when (phase) {
                is TerminalPhase.Ended -> CcButton("Close", onClick = onBack)
                TerminalPhase.Disconnected -> CcButton("Retry") { attempt++ }
                TerminalPhase.ViewOnly -> CcButton("Take control") { if (!link.takeControl()) attempt++ }
                TerminalPhase.Attached -> CcButton("View only") { link.viewOnly() }
                TerminalPhase.Connecting -> {}
            }
            if (!ended && !closing) CcButton("Close…", CcButtonStyle.Danger) { confirming = true }
        }
        link.error?.let { Box(Modifier.padding(horizontal = 12.dp)) { WorkspaceError(it) } }
        Box(Modifier.weight(1f).fillMaxWidth().background(Color(TerminalEmulatorModel.BACKGROUND))) { TerminalEmulator(emulator, Modifier.fillMaxSize()) }
        if (!emulator.readOnly) TerminalKeyBar(emulator.mods, emulator::press, emulator::toggle)
    }
}

private fun terminalState(terminal: Terminal): String = when (val state = terminal.state) {
    com.flashmind.beep.proto.TerminalState.Running -> "Running"
    is com.flashmind.beep.proto.TerminalState.Exited -> state.code?.let { "Exited ($it)" } ?: "Exited"
    com.flashmind.beep.proto.TerminalState.Stopped -> "Stopped"
    com.flashmind.beep.proto.TerminalState.Offline -> "Offline"
    com.flashmind.beep.proto.TerminalState.Gone -> "Gone"
}
