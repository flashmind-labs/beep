package com.flashmind.beep.center

import android.text.format.DateUtils
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.flashmind.beep.Beep
import com.flashmind.beep.ChatRepository
import com.flashmind.beep.WorkspaceError
import com.flashmind.beep.WorkspaceLoading
import com.flashmind.beep.bytes
import com.flashmind.beep.ffi.MachineCell
import com.flashmind.beep.ffi.MobileGitItem
import com.flashmind.beep.ffi.MobileUsage
import com.flashmind.beep.ffi.Need
import com.flashmind.beep.proto.AgentStatus
import com.flashmind.beep.proto.MachineSession
import com.flashmind.beep.workspaceLabel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.Locale

private enum class HomeTab { Overview, Prs, Machines }

internal fun agentName(agent: String): String = when (agent) {
    "claude" -> "Claude Code"
    "codex" -> "Codex"
    "gemini" -> "Gemini"
    else -> agent.replaceFirstChar { it.uppercase() }
}

internal fun ago(then: java.time.Instant): String =
    DateUtils.getRelativeTimeSpanString(then.toEpochMilli(), System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS, DateUtils.FORMAT_ABBREV_RELATIVE).toString()

private fun place(session: MachineSession): String =
    listOf(session.cwd.trimEnd('/').substringAfterLast('/'), session.machineName).filter { it.isNotEmpty() }.joinToString(" · ")

private fun money(value: Double) = String.format(Locale.ROOT, "$%.2f", value)

/** The first tab: machines, what waits on the user, what runs, open PRs and issues. */
@Composable
internal fun HomeScreen(repository: ChatRepository, model: HomeModel, openBell: () -> Unit, openSettings: () -> Unit,
                        openMachines: () -> Unit, openSession: (MachineSession) -> Unit, openProject: (String) -> Unit, startAgent: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var tab by rememberSaveable { mutableStateOf(HomeTab.Overview) }
    var usage by remember { mutableStateOf<MobileUsage?>(null) }
    var showUsage by remember { mutableStateOf(false) }
    val owner = LocalLifecycleOwner.current

    suspend fun load() = coroutineScope {
        val refreshed = async { model.refresh() }
        val spend = async { try { repository.workspaceApi().codingUsage() } catch (cancelled: CancellationException) { throw cancelled } catch (_: Exception) { null } }
        refreshed.await()
        spend.await()?.let { usage = it }
        AskCenter.keepAsks(context, model.waiting)
    }
    LaunchedEffect(owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) { load(); delay(10_000) }
        }
    }

    val data = model.home
    Column(Modifier.fillMaxSize()) {
        CcTop("Home") {
            usage?.let { CcPill({ showUsage = true }) {
                Text(money(it.costUsd), fontSize = 12.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                Text("today", fontSize = 12.sp, color = Beep.colors.muted)
            } }
            CcIconButton(Icons.Outlined.Notifications, "Notifications", model.badge, openBell)
            CcIconButton(Icons.Outlined.Settings, "Settings", onClick = openSettings)
        }
        CcBody {
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                data?.machines.orEmpty().forEach { CcStripCell(it.name, stripDetail(it), it.online, openMachines) }
            }
            CcSegmented(listOf(Triple(HomeTab.Overview, "Overview", null), Triple(HomeTab.Prs, "PRs and issues", data?.let { it.pulls.size + it.issues.size }),
                Triple(HomeTab.Machines, "Machines", null)), tab) { tab = it }
            if (data == null) {
                if (model.error.isEmpty()) WorkspaceLoading() else WorkspaceError(model.error) { scope.launch { load() } }
            } else {
                WorkspaceError((listOf(model.error) + data.errors).filter { it.isNotEmpty() }.joinToString("\n").ifEmpty { null })
                when (tab) {
                    HomeTab.Overview -> Overview(model, openSession, openProject, startAgent)
                    HomeTab.Prs -> Pulls(data.pulls, data.issues)
                    HomeTab.Machines -> MachineTable(data.machines)
                }
            }
        }
    }
    if (showUsage) usage?.let { UsageSheet(it) { showUsage = false } }
}

private fun stripDetail(machine: MachineCell): String {
    if (!machine.online) return "offline"
    val agents = if (machine.agents == 0u) "idle" else "${machine.agents} agent${if (machine.agents == 1u) "" else "s"}"
    return machine.freeBytes?.let { "$agents · ${bytes(it)} free" } ?: agents
}

@Composable
private fun Overview(model: HomeModel, openSession: (MachineSession) -> Unit, openProject: (String) -> Unit, startAgent: () -> Unit) {
    val uri = LocalUriHandler.current
    val needs = model.needs
    if (needs.isEmpty()) CcPanel { CcEmpty("Nothing needs you", "Agents ask here when they need an answer.") }
    else CcSection("Needs you", count = needs.size) {
        CcPanel { needs.forEach { need -> key(need.key) { NeedRow(model, need, openSession, openProject, uri::openUri) } } }
    }
    val running = model.home?.running.orEmpty()
    CcSection("Running", count = running.size, action = "Start an agent", onAction = startAgent) {
        CcPanel {
            if (running.isEmpty()) CcEmpty("Nothing running", "Start an agent on one of your machines.")
            running.forEach { session ->
                CcRow(session.title ?: agentName(session.agent), place(session), chevron = true, onClick = { openSession(session) },
                    lead = { CcAgentTile(session.agent) }) {
                    val doing = session.doing
                    when {
                        session.status == AgentStatus.ASKING -> CcTag("Asks", CcKind.Wait)
                        doing != null -> Text(doing, fontSize = 11.5.sp, color = Beep.colors.muted, maxLines = 1)
                        else -> CcTag(session.status.workspaceLabel())
                    }
                }
            }
        }
    }
}

@Composable
private fun NeedRow(model: HomeModel, need: Need, openSession: (MachineSession) -> Unit, openProject: (String) -> Unit, open: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    when (need) {
        is Need.Ask -> {
            val session = need.session
            val question = session.question ?: return
            val primary = question.options.firstOrNull { it.allow }?.id
            Column {
                CcRow(question.text, place(session) + " · " + ago(session.lastActivity), chevron = true, onClick = { openSession(session) },
                    lead = { CcAgentTile(session.agent) })
                Column(Modifier.padding(start = 11.dp, end = 11.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    question.options.forEach { option ->
                        CcButton(option.name, if (option.id == primary) CcButtonStyle.Primary else CcButtonStyle.Plain, leading = true) {
                            scope.launch { model.answer(session, option) }
                        }
                    }
                }
            }
        }
        is Need.Pull -> {
            val item = need.item
            val reason = pullReason(item)
            CcRow("#${item.number} ${item.title}", "${item.repo} · ${reason.first.lowercase()}", lead = { CcKindTile("PR", reason.second) },
                right = { CcButton("Review") { open(item.url) } })
        }
        is Need.Conflict -> CcRow("${need.path} changed on ${need.machines.size} machines", "${need.project} · ${need.machines.joinToString(" and ")}",
            lead = { CcKindTile("⇄", CcKind.Bad) }, right = { CcButton("Resolve") { openProject(need.project) } })
    }
}

private fun pullReason(item: MobileGitItem): Pair<String, CcKind> = when {
    item.changesRequested -> "Changes requested" to CcKind.Bad
    item.checksFailing -> "Checks failing" to CcKind.Bad
    item.reviewRequested -> "Review" to CcKind.Wait
    item.draft -> "Draft" to CcKind.Plain
    else -> "Open" to CcKind.Plain
}

@Composable
private fun Pulls(pulls: List<MobileGitItem>, issues: List<MobileGitItem>) {
    val uri = LocalUriHandler.current
    var showPulls by rememberSaveable { mutableStateOf(true) }
    var filter by rememberSaveable { mutableStateOf("All") }
    CcSegmented(listOf(Triple(true, "Pull requests", pulls.size), Triple(false, "Issues", issues.size)), showPulls) { showPulls = it; filter = "All" }
    CcChips(if (showPulls) listOf("All", "Waiting on you", "Yours", "Failing") else listOf("All") + issues.flatMap { it.labels }.distinct().sorted(), filter) { filter = it }
    val shown = if (showPulls) pulls.filter {
        when (filter) { "Waiting on you" -> it.reviewRequested; "Yours" -> it.authored; "Failing" -> it.checksFailing; else -> true }
    } else issues.filter { filter == "All" || filter in it.labels }
    CcPanel {
        if (shown.isEmpty()) CcEmpty(if (showPulls) "No pull requests" else "No issues", "Nothing open here.")
        shown.forEach { item ->
            val reason = pullReason(item)
            CcRow("#${item.number} ${item.title}",
                if (showPulls) "${item.repo} · ${if (item.authored) "yours" else "by others"}" else (listOf(item.repo) + item.labels).joinToString(" · "),
                chevron = true, onClick = { uri.openUri(item.url) },
                lead = { CcKindTile(if (showPulls) "PR" else "#", if (showPulls) reason.second else CcKind.Plain) },
                right = { if (showPulls) CcTag(reason.first, reason.second) })
        }
    }
    CcNote("Across GitHub and Forgejo.")
}

@Composable
private fun MachineTable(machines: List<MachineCell>) {
    CcPanel {
        Row(Modifier.fillMaxWidth().padding(horizontal = 11.dp, vertical = 7.dp)) {
            listOf("Machine" to 1.6f, "Agents" to 0.8f, "Disk free" to 1f, "Memory" to 1.2f).forEach { (label, weight) ->
                Text(label, Modifier.weight(weight), fontSize = 11.5.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
            }
        }
        machines.forEach { machine ->
            val line = Beep.colors.softLine
            androidx.compose.material3.HorizontalDivider(color = line)
            Row(Modifier.fillMaxWidth().padding(horizontal = 11.dp, vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1.6f), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    CcDot(machine.online)
                    Text(machine.name, fontSize = 12.sp, fontWeight = FontWeight.Medium, maxLines = 1)
                }
                Text(if (machine.online) machine.agents.toString() else "-", Modifier.weight(0.8f), fontSize = 12.sp)
                Text(if (machine.online) machine.freeBytes?.let(::bytes) ?: "-" else "-", Modifier.weight(1f), fontSize = 12.sp)
                Text(if (machine.online) memory(machine) else "-", Modifier.weight(1.2f), fontSize = 12.sp)
            }
        }
    }
}

private fun memory(machine: MachineCell): String {
    val used = machine.memUsed ?: return "-"
    val total = machine.memTotal ?: return "-"
    return "${used / 1_000_000_000uL}/${total / 1_000_000_000uL} GB"
}

/** Plan limits and today's spend. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun UsageSheet(usage: MobileUsage, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.padding(horizontal = 14.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Usage", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            if (usage.limits.isNotEmpty()) CcSection("Plan limits") {
                CcPanel {
                    usage.limits.forEach { limit ->
                        Row(Modifier.fillMaxWidth().heightIn(min = 46.dp).padding(horizontal = 11.dp, vertical = 9.dp), verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                                Text("${agentName(limit.agent)} · ${window(limit.minutes)}", fontSize = 13.sp, fontWeight = FontWeight.Medium)
                                CcBar(limit.usedPercent / 100, warn = limit.usedPercent >= 80)
                            }
                            Text(String.format(Locale.ROOT, "%.0f%%", limit.usedPercent), fontSize = 11.5.sp,
                                color = if (limit.usedPercent >= 80) Beep.colors.warn else Beep.colors.muted)
                        }
                    }
                }
            }
            CcSection("Spend") {
                CcPanel {
                    pair("Today", money(usage.costUsd))
                    pair("Tokens today", usage.tokens.toString())
                }
            }
        }
    }
}

@Composable
private fun pair(key: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 11.dp, vertical = 8.dp)) {
        Text(key, Modifier.weight(1f), fontSize = 13.sp, color = Beep.colors.muted)
        Text(value, fontSize = 13.sp)
    }
}

private fun window(minutes: UInt): String = when {
    minutes == 300u -> "5 hours"
    minutes == 10080u -> "week"
    minutes % 60u == 0u -> "${minutes / 60u} hours"
    else -> "$minutes min"
}
