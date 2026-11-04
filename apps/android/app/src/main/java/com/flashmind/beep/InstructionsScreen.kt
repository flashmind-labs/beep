package com.flashmind.beep

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.InstructionsVersion
import com.flashmind.beep.proto.MachineAgentFiles
import kotlinx.coroutines.launch

@Composable
internal fun InstructionsScreen(repository: ChatRepository, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val current = workspaceLoad(repository) { api.instructions() }
    var text by rememberSaveable { mutableStateOf("") }
    var version by rememberSaveable { mutableStateOf<String?>(null) }
    var loaded by rememberSaveable { mutableStateOf(false) }
    var saved by rememberSaveable { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var history by remember { mutableStateOf<List<InstructionsVersion>?>(null) }
    var files by remember { mutableStateOf<List<MachineAgentFiles>?>(null) }
    var leave by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(current.loading, current.value) {
        if (!loaded && !current.loading && current.error == null) {
            version = current.value?.version?.toString(); text = current.value?.text.orEmpty(); saved = text; loaded = true
        }
    }
    fun back() { if (text != saved) leave = true else onBack() }
    fun act(block: suspend () -> Unit) { scope.launch {
        busy = true
        workspaceAction({ error = it }, block)
        busy = false
    } }
    if (leave) AlertDialog(onDismissRequest = { leave = false }, title = { Text("Discard changes?") },
        confirmButton = { TextButton(onClick = onBack) { Text("Discard") } }, dismissButton = { TextButton(onClick = { leave = false }) { Text("Keep editing") } })
    if (history != null) {
        WorkspacePage("Instructions history", { history = null }) {
            WorkspaceBody {
                history!!.forEach { version -> BeepCard {
                    WorkspaceRow(Icons.Outlined.History, "Version ${version.version}", version.note, onClick = { text = version.text; history = null })
                    WorkspaceCode(version.text)
                } }
                if (history!!.isEmpty()) WorkspaceEmpty("No saved versions")
            }
        }
        return
    }
    if (files != null) {
        WorkspacePage("Import instructions", { files = null }) {
            WorkspaceBody {
                files!!.forEach { machine ->
                    WorkspaceHeading(machine.machineName)
                    WorkspaceError(machine.error)
                    BeepCard { machine.files.forEach { file -> WorkspaceRow(Icons.AutoMirrored.Outlined.InsertDriveFile,
                        file.agent, file.path, onClick = if (file.text == null) null else { { text = file.text.orEmpty(); files = null } }) } }
                }
                if (files!!.isEmpty()) WorkspaceEmpty("No agent instruction files found")
            }
        }
        return
    }
    WorkspacePage("Instructions", ::back, current::refresh, actions = {
        TextButton(onClick = { act { history = api.instructionsHistory() } }, enabled = !busy) { Text("History", fontSize = 12.sp) }
    }) {
        WorkspaceBody {
            Text("Your rules for every agent Beep runs, on every machine.", fontSize = 13.sp, color = Beep.colors.muted)
            WorkspaceError(current.error, current::refresh)
            WorkspaceError(error)
            if (current.loading && !loaded) WorkspaceLoading()
            Text(version?.let { "Version $it" } ?: "No saved version", fontSize = 12.sp, color = Beep.colors.muted)
            OutlinedTextField(text, { if (it.toByteArray().size <= 16 * 1024) text = it }, Modifier.fillMaxWidth(),
                minLines = 13, textStyle = LocalTextStyle.current.copy(fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 18.sp), enabled = loaded && !busy)
            Text("${text.toByteArray().size} of 16384 bytes", fontSize = 12.sp, color = Beep.colors.muted)
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedButton(onClick = { act { files = api.readAgentFiles() } }, enabled = !busy && loaded) { Text("Import") }
                Button(onClick = { act {
                    val result = api.saveInstructions(text, version?.toUIntOrNull())
                    version = result.version.toString(); saved = result.text; text = result.text
                    current.refresh()
                } }, enabled = loaded && !busy && text != saved) { Text(if (busy) "Saving..." else "Save") }
            }
        }
    }
}
