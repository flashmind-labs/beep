package com.flashmind.beep

import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.flashmind.beep.proto.Research
import com.flashmind.beep.proto.ResearchAgent
import com.flashmind.beep.proto.ResearchEntry
import com.flashmind.beep.proto.ResearchEntryBody
import com.flashmind.beep.proto.ResearchStatus
import com.flashmind.beep.proto.ResearchSummary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

// A research is a chat with its lead agent. The chat holds the plan and the result; this file draws the rest:
// its status in the chat, the workspace (paper, notes, sources, team) and the list of every research.

private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

/** How often a research is asked for news while its team works, and otherwise. */
private const val POLL_BUSY_MS = 2_500L
private const val POLL_IDLE_MS = 10_000L
private val RUNNING = setOf(ResearchStatus.RESEARCHING, ResearchStatus.REVIEWING, ResearchStatus.WRITING)
private val scoutColors: List<Color> @Composable get() = with(Beep.colors) { listOf(brand, ok, warn, error, muted) }

/** A research as last fetched, without its log, and every log entry so far. */
internal data class ResearchLive(val research: Research, val entries: List<ResearchEntry>) {
    val status get() = research.status
    val running get() = research.status in RUNNING
    val round get() = research.round.toInt().coerceAtLeast(1)
    fun agent(id: String) = research.agents.find { it.id == id }
        ?: ResearchAgent(id = id, name = id, role = "", model = "", round = 0u)
}

/**
 * Follows the research chat [sessionId] while the screen is started: its status, team and log, fetched again
 * every few seconds while the team works. Null until the server has made the chat a research. When the status
 * changes the chat is reloaded too, since the lead posts the result there.
 */
@Composable
internal fun rememberResearch(repository: ChatRepository, sessionId: String, enabled: Boolean = true): ResearchLive? {
    var live by remember(sessionId) { mutableStateOf<ResearchLive?>(null) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(sessionId, enabled) {
        if (!enabled) return@LaunchedEffect
        var after = 0L
        var entries = emptyList<ResearchEntry>()
        var status: ResearchStatus? = null
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                runCatching { withContext(Dispatchers.IO) { repository.research(sessionId, after) } }.getOrNull()?.let { research ->
                    entries = entries + research.entries
                    research.entries.lastOrNull()?.let { after = it.seq }
                    if (status != null && status != research.status) repository.reloadChat(sessionId)
                    status = research.status
                    live = ResearchLive(research.copy(entries = emptyList()), entries)
                }
                val busy = status == null || status == ResearchStatus.PLANNING || status in RUNNING
                delay(if (busy) POLL_BUSY_MS else POLL_IDLE_MS)
            }
        }
    }
    return live
}

// ---------- Words ----------

private fun minutes(from: Long, to: Long?) = "${((to ?: System.currentTimeMillis()) - from).div(60_000).coerceAtLeast(1)} min"

private fun kind(id: String) = if (id.startsWith("scout-")) "scout" else id

/** A scout of an earlier round, done while a later one works. */
private fun ResearchLive.earlier(id: String) = kind(id) == "scout" && agent(id).round.toInt().coerceAtLeast(1) < round

/** When the newest round started: its scouts' first entry, else the research's start. */
private fun ResearchLive.roundStart(): Long {
    if (round == 1) return research.startedAt
    return entries.firstOrNull { kind(it.agent) == "scout" && agent(it.agent).round.toInt() == round }?.timestamp ?: research.startedAt
}

private enum class Doing { RUN, OK, WAIT, NONE }

/** What agent [id] is doing, in a line, and how far along it is. */
private fun ResearchLive.doing(id: String): Pair<String, Doing> {
    val st = status
    val mine = entries.filter { it.agent == id }
    val notes = mine.count { it.body is ResearchEntryBody.Note }
    val sources = research.sources.count { it.agent == id }
    val said = mine.count { (it.body as? ResearchEntryBody.Said)?.to != null }
    val summary = when (kind(id)) {
        "scout" -> "Done: $notes notes, $sources sources"
        "critic" -> "Done: $said challenges"
        "writer" -> "Done: the paper"
        else -> "Posted the result"
    }
    if (id == "lead") return when {
        st == ResearchStatus.PLANNING -> "Waiting for your go" to Doing.WAIT
        st == ResearchStatus.REVIEWING -> (if (round > 1) "Coordinating round $round" else "Coordinating") to Doing.RUN
        st in RUNNING -> "Watching the room" to Doing.RUN
        st == ResearchStatus.DONE -> summary to Doing.OK
        else -> "" to Doing.NONE
    }
    if (st == ResearchStatus.DONE) return summary to Doing.OK
    if (st !in RUNNING) return (if (mine.isEmpty()) "" else summary.removePrefix("Done: ")) to Doing.NONE
    if (earlier(id)) return summary to Doing.OK
    val active = when (st) {
        ResearchStatus.RESEARCHING -> listOf("scout")
        ResearchStatus.REVIEWING -> listOf("critic", "scout")
        else -> listOf("writer", "critic")
    }
    if (kind(id) in active) {
        val step = mine.lastOrNull { it.body is ResearchEntryBody.Step }?.body as? ResearchEntryBody.Step
        return (step?.label ?: "Starting") to Doing.RUN
    }
    val order = listOf(ResearchStatus.RESEARCHING, ResearchStatus.REVIEWING, ResearchStatus.WRITING)
    val starts = when (kind(id)) { "critic" -> ResearchStatus.REVIEWING; "writer" -> ResearchStatus.WRITING; else -> ResearchStatus.RESEARCHING }
    if (order.indexOf(starts) > order.indexOf(st)) return (if (kind(id) == "critic") "Starts after the scouts" else "Starts after review") to Doing.NONE
    return summary to Doing.OK
}

/** The research's state in a few words, for under its title. */
internal fun ResearchLive.headline(): String {
    val working = research.agents.count { it.id != "lead" && doing(it.id).second == Doing.RUN }
    return when (status) {
        ResearchStatus.PLANNING -> "Waiting for your go"
        ResearchStatus.RESEARCHING -> "${if (round > 1) "Round $round" else "Researching"}, $working agent${if (working == 1) "" else "s"} working"
        ResearchStatus.REVIEWING -> "Reviewing the findings"
        ResearchStatus.WRITING -> "Writing the paper"
        ResearchStatus.DONE -> "Done in ${minutes(roundStart(), research.finishedAt)}"
        ResearchStatus.FAILED -> "The team failed"
        ResearchStatus.STOPPED -> "Stopped"
        ResearchStatus.INTERRUPTED -> "Interrupted by a server restart"
    }
}

/** What the team is doing, for the row over the composer while it works. */
private fun ResearchLive.progress(): String {
    val took = minutes(roundStart(), null)
    return when (status) {
        ResearchStatus.RESEARCHING -> {
            val scouts = research.agents.filter { kind(it.id) == "scout" && !earlier(it.id) }
            val busy = scouts.count { doing(it.id).second == Doing.RUN }
            val n = "$busy of ${scouts.size} scout${if (scouts.size == 1) "" else "s"} working"
            if (round > 1) "Round $round: $n, $took" else "Researching: $n, $took"
        }
        ResearchStatus.REVIEWING -> "The critic and the lead are reviewing, $took"
        else -> if (round > 1) "The writer is revising the paper, $took" else "The writer is drafting, $took"
    }
}

/** The composer's hint in a research chat. */
internal fun ResearchLive?.hint(): String = when {
    this == null -> "Message the lead"
    status == ResearchStatus.PLANNING -> "Say go, or what to change"
    status == ResearchStatus.DONE -> "Ask a follow-up"
    else -> "Message the lead"
}

@Composable
private fun agentColor(id: String, ink: Color): Color = when {
    id == "lead" -> Beep.colors.accent
    id == "critic" -> Beep.colors.brand
    id == "writer" -> ink
    else -> scoutColors[((id.removePrefix("scout-").toIntOrNull() ?: 1) - 1).mod(scoutColors.size)]
}

private fun agentInitial(id: String) = when {
    id == "lead" -> "L"
    id == "critic" -> "R"
    id == "writer" -> "W"
    else -> id.removePrefix("scout-").ifEmpty { "1" }
}

@Composable
private fun AgentDot(id: String, size: Int = 22) {
    val color = agentColor(id, Beep.colors.ink)
    Box(Modifier.size(size.dp).clip(CircleShape).background(color), contentAlignment = Alignment.Center) {
        Text(agentInitial(id), fontSize = (size / 2).sp, fontWeight = FontWeight.Bold,
            color = if (id == "writer") Beep.colors.page else Beep.colors.page)
    }
}

@Composable
private fun DoingIcon(doing: Doing) = when (doing) {
    Doing.RUN -> CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = TextMuted)
    Doing.OK -> Icon(Icons.Outlined.Check, "Done", Modifier.size(16.dp), tint = Beep.colors.ok)
    Doing.WAIT -> Box(Modifier.size(8.dp).clip(CircleShape).background(Beep.colors.accent))
    Doing.NONE -> Unit
}

/** The research's state as the line under the chat's title: an icon and [ResearchLive.headline]. */
@Composable
internal fun ResearchHeadline(live: ResearchLive) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        when {
            live.running -> CircularProgressIndicator(Modifier.size(10.dp), strokeWidth = 1.5.dp, color = TextMuted)
            live.status == ResearchStatus.PLANNING -> Box(Modifier.size(7.dp).clip(CircleShape).background(Beep.colors.accent))
            live.status == ResearchStatus.DONE -> Icon(Icons.Outlined.Check, null, Modifier.size(12.dp), tint = Beep.colors.ok)
            else -> Unit
        }
        Text(live.headline(), fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

// ---------- In the chat ----------

/** Under the lead's newest reply: Start while it waits for a go, the paper once it is written. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ResearchActions(live: ResearchLive, onGo: () -> Unit, onPaper: () -> Unit, onShare: () -> Unit) {
    val chips: List<Pair<String, () -> Unit>> = when (live.status) {
        ResearchStatus.PLANNING -> listOf("Start" to onGo)
        ResearchStatus.DONE -> listOfNotNull("Open the paper" to onPaper, ("Share PDF" to onShare).takeIf { live.research.draft.isNotBlank() })
        else -> emptyList()
    }
    if (chips.isEmpty()) return
    FlowRow(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        chips.forEachIndexed { i, (label, action) ->
            val go = i == 0 && live.status == ResearchStatus.PLANNING
            ChunkyButton(onClick = action, modifier = Modifier.popIn("research-$label"),
                color = if (go) Beep.colors.user else Surface, contentColor = if (go) Beep.colors.onUser else Beep.colors.ink,
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 7.dp)) {
                Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

/** While the team works, what it is doing, over the composer. Tapping it opens the workspace. */
@Composable
internal fun ResearchProgressRow(live: ResearchLive, onOpen: () -> Unit, onStop: () -> Unit) {
    Row(Modifier.padding(horizontal = 14.dp).padding(top = 6.dp).fillMaxWidth()
        .beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface)
        .clickable(onClickLabel = "Open the workspace", onClick = onOpen).padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = TextMuted)
        Text(live.progress(), Modifier.weight(1f).padding(vertical = 12.dp), fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
        TextButton(onClick = onStop) { Text("Stop", color = Beep.colors.error, fontWeight = FontWeight.SemiBold) }
    }
}

/**
 * What the chat screen needs to show research in it. [isResearch] says whether the chat is one; [live] is
 * that research once the server has it.
 */
internal class ResearchChat(val isResearch: Boolean, val live: ResearchLive?, val onWorkspace: (WorkspaceTab?) -> Unit,
                            val onShare: () -> Unit, val onStop: () -> Unit)

// ---------- Sharing the paper ----------

/** Fetches the paper as a PDF and opens the share sheet. [onState] gets a line to show, or null once done. */
internal suspend fun shareResearchPdf(context: Context, repository: ChatRepository, sessionId: String,
                                      onState: (String?, Boolean) -> Unit) {
    onState("Typesetting the PDF", false)
    try {
        val file = withContext(Dispatchers.IO) {
            val dir = File(context.cacheDir, "sent/research-$sessionId").apply { mkdirs() }
            File(repository.saveResearchPdf(sessionId, dir.path))
        }
        onState(null, false)
        share(context, SentFiles.uriOf(context, file), "application/pdf")
    } catch (e: Exception) {
        onState(userMessage(e), true)
    }
}

/** A short line over the bottom of a screen, with a spinner while it waits on something. */
@Composable
internal fun Notice(text: String, bad: Boolean, modifier: Modifier = Modifier) {
    Row(modifier.clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(if (bad) Beep.colors.error else Beep.colors.ink)
        .padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (!bad) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = Beep.colors.page)
        Text(text, fontSize = 13.sp, color = if (bad) Color.White else Beep.colors.page, fontWeight = FontWeight.Medium)
    }
}

// ---------- The workspace ----------

internal enum class WorkspaceTab(val label: String) { PAPER("Paper"), NOTES("Notes"), SOURCES("Sources"), TEAM("Team") }

/** One agent's thread, or the room where they all talk, opened from the Team tab. */
private sealed interface Opened {
    data class Agent(val id: String) : Opened
    data object Room : Opened
}

/**
 * The team's work on the research chat [sessionId]: the paper, the notes, the sources and the team. The paper
 * and notes are selectable, and Quote in the selection toolbar sends the text to [onQuote] for the chat.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ResearchWorkspace(repository: ChatRepository, sessionId: String, start: WorkspaceTab?, onBack: () -> Unit,
                               onQuote: (String) -> Unit) {
    val live = rememberResearch(repository, sessionId)
    var chosen by rememberSaveable(sessionId) { mutableStateOf(start) }
    // Until one is picked, the tab follows the research: notes while it works, the paper once written.
    val tab = chosen ?: if (live?.research?.draft.isNullOrBlank()) WorkspaceTab.NOTES else WorkspaceTab.PAPER
    var opened by remember { mutableStateOf<Opened?>(null) }
    var notice by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val openLink = { url: String -> ExternalBrowser.open(context, url) }
    BackHandler { if (opened != null) opened = null else onBack() }
    LaunchedEffect(notice) { if (notice?.second == true) { delay(8_000); notice = null } }
    val agent = (opened as? Opened.Agent)?.let { live?.agent(it.id) }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = {
            Column {
                Text(when { agent != null -> agent.name; opened == Opened.Room -> "Room"; else -> live?.research?.topic ?: "Research" },
                    fontWeight = FontWeight.SemiBold, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                when {
                    agent != null -> Text(listOf(agent.role, agent.model).filter { it.isNotBlank() }.joinToString(" · "),
                        fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    opened == Opened.Room -> Text("Where the agents talk", fontSize = 12.sp, color = TextMuted)
                    live != null -> ResearchHeadline(live)
                }
            }
        }, navigationIcon = {
            IconButton(onClick = { if (opened != null) opened = null else onBack() }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") }
        }, actions = {
            if (opened == null && !live?.research?.draft.isNullOrBlank()) IconButton(enabled = notice?.second != false, onClick = {
                scope.launch { shareResearchPdf(context, repository, sessionId) { text, bad -> notice = text?.let { it to bad } } }
            }) { Icon(Icons.Outlined.Share, "Share PDF") }
        }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Box(Modifier.padding(padding).fillMaxSize()) {
            Column(Modifier.fillMaxSize()) {
                if (opened == null) Row(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = BeepSpace.page).padding(bottom = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    WorkspaceTab.entries.forEach { t ->
                        val label = if (t == WorkspaceTab.SOURCES && live?.research?.sources?.isNotEmpty() == true)
                            "Sources ${live.research.sources.size}" else t.label
                        FilterChip(selected = t == tab, onClick = { chosen = t }, label = { Text(label, fontSize = 13.sp) },
                            colors = FilterChipDefaults.filterChipColors(selectedContainerColor = Beep.colors.user,
                                selectedLabelColor = Beep.colors.onUser, containerColor = Surface),
                            shape = RoundedCornerShape(Beep.shapes.radius))
                    }
                }
                when {
                    live == null -> Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    }
                    agent != null -> AgentThread(live, agent.id)
                    opened == Opened.Room -> Room(live)
                    tab == WorkspaceTab.PAPER -> Paper(live, onQuote)
                    tab == WorkspaceTab.NOTES -> Notes(live, onQuote)
                    tab == WorkspaceTab.SOURCES -> Sources(live, openLink)
                    else -> Team(live, onAgent = { opened = Opened.Agent(it) }, onRoom = { opened = Opened.Room })
                }
            }
            notice?.let { (text, bad) -> Notice(text, bad, Modifier.align(Alignment.BottomCenter).padding(bottom = 24.dp, start = 16.dp, end = 16.dp)) }
        }
    }
}

@Composable
private fun Empty(text: String) =
    Text(text, fontSize = 13.sp, color = TextMuted, lineHeight = 20.sp, textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 40.dp, vertical = 60.dp))

@Composable
private fun Paper(live: ResearchLive, onQuote: (String) -> Unit) {
    val r = live.research
    if (r.draft.isBlank()) return Empty(when {
        live.status == ResearchStatus.PLANNING -> "The workspace fills in once the team starts."
        live.running -> "The writer starts after the critic's review."
        else -> "No paper was written."
    })
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 18.dp).padding(bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Written by ${live.agent("writer").name}, reviewed by ${live.agent("critic").name}. ${r.sources.size} sources.",
            fontSize = 12.sp, color = TextMuted)
        SelectableMessage(onQuote) { picker -> Box(picker.tracking) { MessageContent(r.draft, "assistant", compact = false, padded = false) } }
    }
}

@Composable
private fun Notes(live: ResearchLive, onQuote: (String) -> Unit) {
    val notes = live.entries.filter { it.body is ResearchEntryBody.Note }
    if (notes.isEmpty()) return Empty(if (live.status == ResearchStatus.PLANNING) "The workspace fills in once the team starts." else "No notes yet.")
    SelectableMessage(onQuote) { _ -> SelectionContainer {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp)) {
            items(notes, key = { it.seq }) { entry -> NoteRow(live, entry, live.agent(entry.agent).name) }
        }
    } }
}

@Composable
private fun NoteRow(live: ResearchLive, entry: ResearchEntry, label: String) {
    // The scout cites its sources in the text itself.
    val note = entry.body as? ResearchEntryBody.Note ?: return
    Row(Modifier.height(IntrinsicSize.Min)) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(agentColor(entry.agent, Beep.colors.ink)))
        Column(Modifier.padding(start = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(label, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)
            Text(note.content, fontSize = 14.sp, lineHeight = 21.sp)
        }
    }
}

@Composable
private fun Sources(live: ResearchLive, openLink: (String) -> Unit) {
    val sources = live.research.sources
    if (sources.isEmpty()) return Empty(if (live.status == ResearchStatus.PLANNING) "The workspace fills in once the team starts." else "No sources yet.")
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 32.dp)) {
        items(sources, key = { it.number.toInt() }) { s ->
            Row(Modifier.fillMaxWidth().clickable(onClickLabel = "Open the source") { openLink(s.url) }.padding(vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("${s.number}", Modifier.width(22.dp), fontWeight = FontWeight.SemiBold, color = Beep.colors.brand)
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(s.title.ifBlank { s.url }, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = Beep.colors.brand)
                    Text("${host(s.url)}, kept by ${live.agent(s.agent).name}", fontSize = 12.sp, color = TextMuted)
                    if (s.quote.isNotBlank()) Text("“${s.quote}”", fontSize = 13.sp, lineHeight = 19.sp, fontStyle = FontStyle.Italic, maxLines = 4, overflow = TextOverflow.Ellipsis)
                }
            }
            HorizontalDivider(color = Beep.colors.softLine)
        }
    }
}

private fun host(url: String) = runCatching { java.net.URI(url).host?.removePrefix("www.") }.getOrNull() ?: url

@Composable
private fun Team(live: ResearchLive, onAgent: (String) -> Unit, onRoom: () -> Unit) {
    val planning = live.status == ResearchStatus.PLANNING
    val agents = live.research.agents
    val later = agents.filter { it.round.toInt() > 1 }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        items(agents.filter { it.round.toInt() <= 1 && it.id != "critic" && it.id != "writer" }, key = { it.id }) { a ->
            AgentRow(live, a, enabled = !planning || a.id == "lead") { onAgent(a.id) }
        }
        for (n in 2..live.round) {
            val of = later.filter { it.round.toInt() == n }
            if (of.isEmpty()) continue
            item(key = "round-$n") { Text("Round $n", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted, modifier = Modifier.padding(top = 8.dp, start = 4.dp)) }
            items(of, key = { it.id }) { a -> AgentRow(live, a, enabled = true) { onAgent(a.id) } }
        }
        items(agents.filter { it.id == "critic" || it.id == "writer" }, key = { it.id }) { a ->
            AgentRow(live, a, enabled = !planning) { onAgent(a.id) }
        }
        item(key = "room") {
            val said = live.entries.count { it.body is ResearchEntryBody.Said }
            Row(Modifier.fillMaxWidth().alpha(if (planning) 0.5f else 1f).beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface)
                .clickable(enabled = !planning, onClickLabel = "Open the room", onClick = onRoom).padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Box(Modifier.size(22.dp).clip(CircleShape).background(Beep.colors.softLine), contentAlignment = Alignment.Center) {
                    Text("#", fontSize = 12.sp, fontWeight = FontWeight.Bold)
                }
                Column {
                    Text("Room", fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    Text(if (planning) "Opens when the team starts" else "$said message${if (said == 1) "" else "s"}", fontSize = 12.sp, color = TextMuted)
                }
            }
        }
    }
}

@Composable
private fun AgentRow(live: ResearchLive, agent: ResearchAgent, enabled: Boolean, onClick: () -> Unit) {
    val (now, state) = live.doing(agent.id)
    Row(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface)
        .clickable(enabled = enabled, onClickLabel = "Open ${agent.name}'s thread", onClick = onClick).padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        AgentDot(agent.id)
        Column(Modifier.weight(1f)) {
            Text(agent.name, fontSize = 15.sp, fontWeight = FontWeight.Medium)
            Text(now.ifBlank { agent.role }, fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (agent.model.isNotBlank()) Text(agent.model, fontSize = 11.sp, color = TextDim)
        }
        DoingIcon(state)
    }
}

@Composable
private fun Said(live: ResearchLive, entry: ResearchEntry) {
    val said = entry.body as? ResearchEntryBody.Said ?: return
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        AgentDot(entry.agent)
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(buildString {
                append(live.agent(entry.agent).name)
                said.to?.let { append(" to ${live.agent(it).name}") }
            }, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)
            Text(said.content, fontSize = 14.sp, lineHeight = 20.sp)
        }
    }
}

@Composable
private fun Step(step: ResearchEntryBody.Step, running: Boolean = false) {
    Row(Modifier.padding(start = 32.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        when {
            running -> CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 2.dp, color = TextMuted)
            step.success -> Icon(Icons.Outlined.Check, null, Modifier.size(14.dp), tint = Beep.colors.ok)
            else -> Icon(Icons.Outlined.Close, null, Modifier.size(14.dp), tint = Beep.colors.error)
        }
        Text(step.label, fontSize = 13.sp, color = TextMuted)
    }
}

@Composable
private fun AgentThread(live: ResearchLive, id: String) {
    val mine = live.entries.filter { it.agent == id || (it.body as? ResearchEntryBody.Said)?.to == id }
    val (now, state) = live.doing(id)
    val list = rememberLazyListState()
    LaunchedEffect(mine.size) { if (mine.isNotEmpty()) list.scrollToItem(mine.size) }
    if (mine.isEmpty() && state != Doing.RUN) return Empty("Nothing yet.")
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)) {
        items(mine, key = { it.seq }) { entry ->
            when (val body = entry.body) {
                is ResearchEntryBody.Said -> Said(live, entry)
                is ResearchEntryBody.Step -> if (entry.agent == id) Step(body)
                is ResearchEntryBody.Note -> NoteRow(live, entry, "Note")
            }
        }
        if (state == Doing.RUN) item(key = "now") { Step(ResearchEntryBody.Step(now, true), running = true) }
    }
}

@Composable
private fun Room(live: ResearchLive) {
    val said = live.entries.filter { it.body is ResearchEntryBody.Said }
    if (said.isEmpty()) return Empty("Nobody has said anything yet.")
    val list = rememberLazyListState()
    LaunchedEffect(said.size) { list.scrollToItem(said.lastIndex) }
    LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)) {
        items(said, key = { it.seq }) { Said(live, it) }
    }
}

// ---------- Every research ----------

/** Every research on the account, newest first, from any device. The field on top starts a new one. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ResearchListScreen(repository: ChatRepository, onBack: () -> Unit, onOpen: (String) -> Unit, onStart: (String) -> Unit) {
    var list by remember { mutableStateOf<List<ResearchSummary>?>(null) }
    var error by remember { mutableStateOf("") }
    var topic by rememberSaveable { mutableStateOf("") }
    LaunchedEffect(Unit) {
        while (true) {
            runCatching { withContext(Dispatchers.IO) { repository.listResearches() } }
                .onSuccess { list = it; error = "" }.onFailure { if (list == null) error = userMessage(it) }
            delay(if (list.orEmpty().any { it.status in RUNNING || it.status == ResearchStatus.PLANNING }) POLL_BUSY_MS * 2 else POLL_IDLE_MS * 3)
        }
    }
    fun start() { topic.trim().takeIf { it.isNotEmpty() }?.let { onStart(it); topic = "" } }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText("Research"), style = displayStyle(18.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(Modifier.padding(horizontal = BeepSpace.page).padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextField(topic, { topic = it }, Modifier.weight(1f).beepFrame(RoundedCornerShape(Beep.shapes.radius), Surface),
                    placeholder = { Text("What should the team research?", fontSize = 15.sp, color = TextDim) },
                    colors = TextFieldDefaults.colors(unfocusedContainerColor = Color.Transparent, focusedContainerColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent, focusedIndicatorColor = Color.Transparent),
                    maxLines = 4, keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Go),
                    keyboardActions = KeyboardActions(onGo = { start() }))
                ChunkyButton(onClick = ::start, modifier = Modifier.size(50.dp), shape = RoundedCornerShape(Beep.shapes.radius),
                    color = Beep.colors.user, contentColor = Beep.colors.onUser, contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.Outlined.ArrowUpward, "Research", Modifier.size(22.dp))
                }
            }
            if (error.isNotEmpty()) Text(error, color = Beep.colors.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            val shown = list
            when {
                shown == null -> if (error.isEmpty()) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                }
                shown.isEmpty() -> Empty("No research yet.\nA team of agents digs into a question and writes it up as a paper.")
                else -> LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(shown, key = { it.sessionId }) { r -> ResearchRow(r) { onOpen(r.sessionId) } }
                }
            }
        }
    }
}

@Composable
private fun ResearchRow(r: ResearchSummary, onClick: () -> Unit) {
    val off = r.status == ResearchStatus.STOPPED || r.status == ResearchStatus.FAILED || r.status == ResearchStatus.INTERRUPTED
    Row(Modifier.fillMaxWidth().alpha(if (off) 0.6f else 1f).beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface)
        .clickable(onClickLabel = "Open", onClick = onClick).padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.width(20.dp), contentAlignment = Alignment.Center) {
            when {
                r.status in RUNNING -> CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = TextMuted)
                r.status == ResearchStatus.PLANNING -> Box(Modifier.size(8.dp).clip(CircleShape).background(Beep.colors.accent))
                r.status == ResearchStatus.DONE -> Icon(Icons.Outlined.Check, null, Modifier.size(18.dp), tint = Beep.colors.ok)
                else -> Icon(Icons.Outlined.Search, null, Modifier.size(18.dp), tint = TextMuted)
            }
        }
        Column(Modifier.weight(1f)) {
            Text(r.topic, fontSize = 15.sp, fontWeight = FontWeight.Medium, lineHeight = 20.sp, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Text(summaryLine(r), fontSize = 12.sp, color = TextMuted, maxLines = 1)
        }
    }
}

/** "Round 2 · researching", "Done · 18 sources · Yesterday". */
private fun summaryLine(r: ResearchSummary): String {
    val round = if (r.round > 1u) "Round ${r.round} · " else ""
    val sources = if (r.sources > 0u) " · ${r.sources} source${if (r.sources == 1u) "" else "s"}" else ""
    return when (r.status) {
        ResearchStatus.PLANNING -> "Waiting for your go"
        ResearchStatus.RESEARCHING -> "${round}Researching$sources"
        ResearchStatus.REVIEWING -> "${round}Reviewing$sources"
        ResearchStatus.WRITING -> "${round}Writing$sources"
        ResearchStatus.DONE -> "Done$sources · ${day(r.finishedAt ?: r.startedAt)}"
        ResearchStatus.FAILED -> "Failed$sources · ${day(r.startedAt)}"
        ResearchStatus.STOPPED -> "Stopped$sources · ${day(r.startedAt)}"
        ResearchStatus.INTERRUPTED -> "Interrupted$sources · ${day(r.startedAt)}"
    }
}

private fun day(ms: Long): String {
    val date = Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()).toLocalDate()
    val today = LocalDate.now()
    return when (date) {
        today -> "Today"
        today.minusDays(1) -> "Yesterday"
        else -> date.format(DateTimeFormatter.ofPattern(if (date.year == today.year) "d MMM" else "d MMM yyyy", Locale.getDefault()))
    }
}

/** The workspace button in a research chat's bar. */
@Composable
internal fun WorkspaceButton(onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, shape = RoundedCornerShape(Beep.shapes.smallRadius),
        modifier = Modifier.padding(end = 12.dp).size(42.dp), contentPadding = PaddingValues(0.dp)) {
        Icon(Icons.Outlined.Description, "Workspace", Modifier.size(20.dp))
    }
}
