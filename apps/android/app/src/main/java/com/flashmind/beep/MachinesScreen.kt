package com.flashmind.beep

import android.text.format.DateUtils
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.Machine
import com.flashmind.beep.proto.MachineEnv
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The machines running beep-node on the account, for admins. A machine shows
 * here once it has connected; linking one is the same as linking a computer,
 * with the link `beep-node link` prints pasted as a code.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun MachinesScreen(repository: ChatRepository, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var machines by remember { mutableStateOf<List<Machine>?>(null) }
    var error by remember { mutableStateOf("") }
    var open by rememberSaveable { mutableStateOf<String?>(null) }
    fun refresh() = scope.launch {
        runCatching { withContext(Dispatchers.IO) { repository.listMachines() } }
            .onSuccess { machines = it; error = "" }
            .onFailure { error = userMessage(it) }
    }
    LaunchedEffect(Unit) { refresh() }
    val codes = rememberLinkCodes(repository, onLinked = { refresh() }, onJoined = { refresh() })
    LinkCodeSheets(codes)

    val shown = machines?.firstOrNull { it.id == open }
    BackHandler(enabled = shown != null) { open = null }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText(shown?.name ?: "Machines"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = { if (shown != null) open = null else onBack() }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        if (shown != null) {
            MachineDetail(shown, Modifier.padding(padding))
            return@Scaffold
        }
        Column(Modifier.padding(padding).fillMaxSize().padding(horizontal = 16.dp, vertical = 8.dp)) {
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (error.isNotEmpty()) Text(error, color = Beep.colors.error, fontSize = 12.sp)
                val list = machines
                when {
                    list == null && error.isEmpty() -> Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    }
                    list.isNullOrEmpty() -> Text("No machines yet.\nRun beep-node link on a Mac or Linux box, then paste its link here.",
                        fontSize = 13.sp, lineHeight = 20.sp, color = Beep.colors.muted, textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 40.dp, horizontal = 12.dp))
                    else -> {
                        list.forEach { machine -> MachineCard(machine) { open = machine.id } }
                        Text("Beep runs commands and coding agents on these when you ask. Each one runs beep-node and is linked like a computer.",
                            fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted, modifier = Modifier.padding(horizontal = 4.dp))
                    }
                }
            }
            Button(onClick = { codes.typing = true }, modifier = Modifier.fillMaxWidth().height(52.dp),
                shape = RoundedCornerShape(16.dp),
                colors = ButtonDefaults.buttonColors(containerColor = Beep.colors.accent, contentColor = Beep.colors.onAccent)) {
                Icon(Icons.Outlined.Dns, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp))
                Text("Link a machine", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

/** "Online", or when it was last seen. */
private fun seen(machine: Machine): String =
    if (machine.online) "Online" else "Last seen " + DateUtils.getRelativeTimeSpanString(machine.seenAt.toEpochMilli())

private fun system(env: MachineEnv): String =
    listOf(env.osVersion.ifEmpty { env.os }, env.arch).filter { it.isNotEmpty() }.joinToString(", ")

/** The CLIs signed in, as "gh as build-user, claude signed in". */
internal fun signIns(env: MachineEnv): String =
    env.logins.filter { it.signedIn }.joinToString(", ") { login -> login.account?.let { "${login.tool} as $it" } ?: "${login.tool} signed in" }

@Composable
private fun StatusDot(online: Boolean) {
    Box(Modifier.size(8.dp).background(if (online) Beep.colors.ok else Beep.colors.dim, CircleShape))
}

@Composable
private fun MachineIcon(online: Boolean, size: Int) {
    Box(Modifier.size(size.dp).background(Beep.colors.tints[if (online) 1 else 2], RoundedCornerShape((size / 3.5).dp)),
        contentAlignment = Alignment.Center) {
        Icon(Icons.Outlined.Dns, null, Modifier.size((size / 2).dp), tint = Beep.colors.ink)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MachineCard(machine: Machine, onClick: () -> Unit) {
    BeepCard(Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
            MachineIcon(machine.online, 40)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(machine.name, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.ink)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    StatusDot(machine.online); Spacer(Modifier.width(6.dp))
                    Text(seen(machine), fontSize = 12.sp, color = Beep.colors.muted)
                }
                val env = machine.env
                if (env == null) Text("No report yet", fontSize = 12.sp, color = Beep.colors.dim)
                else {
                    Text(system(env), fontSize = 12.sp, color = Beep.colors.muted)
                    if (env.tools.isNotEmpty()) FlowRow(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        env.tools.take(4).forEach { ToolTag(it.name) }
                        if (env.tools.size > 4) Text("+${env.tools.size - 4}", fontSize = 11.sp, color = Beep.colors.muted,
                            modifier = Modifier.align(Alignment.CenterVertically))
                    }
                    signIns(env).takeIf { it.isNotEmpty() }?.let { Text(it, fontSize = 12.sp, color = Beep.colors.muted) }
                }
            }
        }
    }
}

@Composable
private fun ToolTag(name: String) {
    Text(name, fontSize = 11.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink,
        modifier = Modifier.border(1.dp, Beep.colors.line, RoundedCornerShape(50)).padding(horizontal = 8.dp, vertical = 2.dp))
}

/** Everything a machine reported, in sections. */
@Composable
private fun MachineDetail(machine: Machine, modifier: Modifier) {
    Column(modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            MachineIcon(machine.online, 52)
            Spacer(Modifier.width(14.dp))
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    StatusDot(machine.online); Spacer(Modifier.width(6.dp))
                    Text(if (machine.online) "Online" else "Offline, " + seen(machine).lowercase(),
                        fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                }
                Text("beep-node ${machine.version}", fontSize = 12.sp, color = Beep.colors.muted)
            }
        }
        val env = machine.env
        if (env == null) {
            Text("It has not reported what it has yet.", fontSize = 13.sp, color = Beep.colors.muted, modifier = Modifier.padding(4.dp))
            return@Column
        }
        Section("System")
        Facts(listOf("OS" to env.osVersion.ifEmpty { env.os }, "Chip" to env.arch, "CPUs" to env.cpus.toString(),
            "Memory" to "${env.memoryMb / 1024u} GB", "Host" to env.hostname, "Home" to env.home).filter { it.second.isNotEmpty() },
            mono = setOf("Home"))
        if (env.logins.isNotEmpty()) {
            Section("Signed in")
            Facts(env.logins.map { it.tool to (if (!it.signedIn) "Not signed in" else it.account ?: "Signed in") })
        }
        if (env.tools.isNotEmpty()) {
            Section("Tools")
            Facts(env.tools.map { it.name to it.version.orEmpty() })
        }
        if (env.avds.isNotEmpty()) {
            Section("Android emulators")
            Facts(env.avds.map { it to "" })
        }
    }
}

@Composable
private fun Section(title: String) {
    Text(title, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted,
        modifier = Modifier.padding(start = 4.dp, top = 8.dp))
}

@Composable
private fun Facts(rows: List<Pair<String, String>>, mono: Set<String> = emptySet()) {
    BeepCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 14.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            rows.forEach { (label, value) ->
                Row {
                    Text(label, fontSize = 13.sp, color = if (value.isEmpty()) Beep.colors.ink else Beep.colors.muted,
                        modifier = if (value.isEmpty()) Modifier else Modifier.width(96.dp))
                    if (value.isNotEmpty()) Text(value, fontSize = 13.sp, color = Beep.colors.ink,
                        fontFamily = if (label in mono) FontFamily.Monospace else null)
                }
            }
        }
    }
}
