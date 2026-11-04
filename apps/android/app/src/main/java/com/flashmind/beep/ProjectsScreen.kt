package com.flashmind.beep

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.automirrored.outlined.CallMerge
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.MobileProjectLists
import com.flashmind.beep.ffi.MobileRun
import com.flashmind.beep.proto.*
import kotlinx.coroutines.launch
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit

@Composable
internal fun ProjectsScreen(repository: ChatRepository, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val projects = workspaceLoad(repository) { api.listProjects() }
    val imports = workspaceLoad(repository) { api.listImportedProjects() }
    val ids = projects.value.orEmpty().map { it.projectId }
    val counts = workspaceLoad(ids) {
        coroutineScope {
            val limit = Semaphore(3)
            ids.map { id -> async { limit.withPermit {
                val list = api.projectLists(id, false)
                check(list.prsError == null && list.issuesError == null) { list.prsError ?: list.issuesError.orEmpty() }
                list
            } } }.awaitAll()
        }
    }
    val sessions = workspaceLoad(repository) { api.codingSessions(null) }
    val machines = workspaceLoad(repository) { repository.listMachines() }
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    var allAgents by rememberSaveable { mutableStateOf(false) }
    var recentSession by remember { mutableStateOf<MachineSession?>(null) }
    if (allAgents || recentSession != null) {
        CodingAgentsScreen(repository, initialSession = recentSession, onBack = { allAgents = false; recentSession = null })
        return
    }
    var adding by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    if (adding) { AddProjectScreen(repository, onBack = { adding = false }) {
        adding = false; projects.refresh(); imports.refresh(); selected = it
    }; return }
    val name = selected
    if (name != null) { ProjectScreen(repository, name, onBack = { selected = null; projects.refresh() }); return }
    fun refresh() { projects.refresh(); imports.refresh(); counts.refresh(); sessions.refresh(); machines.refresh() }
    val names = (projects.value.orEmpty().map { it.name } + imports.value.orEmpty().map { it.project }).distinct().sorted()
    WorkspacePage("Projects", onBack, ::refresh, actions = { TextButton(onClick = { allAgents = true }) { Text("Agents", fontSize = 12.sp) } }) {
        WorkspaceBody {
            WorkspaceSearch(query, "Filter projects") { query = it }
            WorkspaceHeading("Repositories", "Add") { adding = true }
            WorkspaceError(projects.error, projects::refresh)
            WorkspaceError(imports.error, imports::refresh)
            if (projects.loading && projects.value == null) WorkspaceLoading()
            if (!projects.loading && names.isEmpty()) WorkspaceEmpty("No projects yet. Add a repository to get started.")
            BeepCard(Modifier.fillMaxWidth()) {
                names.filter { it.contains(query, true) }.forEach { name ->
                    val project = projects.value?.find { it.name == name }
                    WorkspaceRow(Icons.Outlined.Folder, name.substringAfterLast('/'), name,
                        project?.openWorkers?.takeIf { it > 0u }?.let { "$it agents" }, onClick = { selected = name })
                }
            }
            if (names.isNotEmpty() && names.none { it.contains(query, true) }) WorkspaceEmpty("No matching projects")
            projects.value?.takeIf { it.isNotEmpty() }?.let { list ->
                WorkspaceHeading("Workspace")
                WorkspaceFacts(listOf(
                    "Open PRs" to (counts.value?.sumOf { it.pullRequests.size }?.toString() ?: "Unavailable"),
                    "Issues" to (counts.value?.sumOf { it.issues.size }?.toString() ?: "Unavailable"),
                    "Running agents" to (sessions.value?.count { it.status != AgentStatus.ENDED }?.toString() ?: "Unavailable"),
                    "Machines" to (machines.value?.let { "${it.count { m -> m.online }} online" } ?: "Unavailable")))
                WorkspaceError(counts.error, counts::refresh)
                WorkspaceError(sessions.error, sessions::refresh)
                WorkspaceError(machines.error, machines::refresh)
                WorkspaceHeading("Recent activity")
                BeepCard { sessions.value.orEmpty().sortedByDescending { it.lastActivity }.take(4).forEach { session ->
                    WorkspaceRow(Icons.Outlined.Code, session.title ?: session.agent, session.machineName,
                        session.status.workspaceLabel(), onClick = { recentSession = session })
                } }
            }
        }
    }
}

@Composable
private fun AddProjectScreen(repository: ChatRepository, onBack: () -> Unit, onAdded: (String) -> Unit) {
    val api = repository.workspaceApi()
    val machines = workspaceLoad(repository) { repository.listMachines() }
    val choices = machines.value.orEmpty().filter { it.online }
    var machineId by rememberSaveable { mutableStateOf("") }
    var repo by rememberSaveable { mutableStateOf("") }
    var folder by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val machine = choices.find { it.id == machineId } ?: choices.firstOrNull()
    WorkspacePage("Add project", onBack) {
        WorkspaceBody {
            OutlinedTextField(repo, { repo = it }, Modifier.fillMaxWidth(), label = { Text("Repository") }, placeholder = { Text("owner/repository or URL") }, singleLine = true)
            WorkspaceChoice("Machine", machine?.name.orEmpty(), choices.map { it.name }) { machineId = choices[it].id }
            OutlinedTextField(folder, { folder = it }, Modifier.fillMaxWidth(), label = { Text("Folder (optional)") }, singleLine = true)
            WorkspaceError(machines.error, machines::refresh)
            if (choices.isEmpty() && !machines.loading) WorkspaceEmpty("Link an online machine in Settings to add a project.")
            WorkspaceError(error)
            Button(onClick = { scope.launch {
                busy = true
                workspaceAction({ error = it }) {
                    val imported = api.importProject(repo.trim(), machine!!.id, folder.trim().ifEmpty { null })
                    api.projectAgent(imported.project)
                    onAdded(imported.project)
                }
                busy = false
            } }, enabled = !busy && repo.isNotBlank() && machine != null, modifier = Modifier.fillMaxWidth()) {
                Text(if (busy) "Adding project..." else "Add project")
            }
        }
    }
}

@Composable
private fun ProjectScreen(repository: ChatRepository, name: String, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val project = workspaceLoad(name) { api.projectAgent(name) }
    var tab by rememberSaveable(name) { mutableStateOf("Overview") }
    var selectedSession by remember { mutableStateOf<MachineSession?>(null) }
    var startingAgent by rememberSaveable(name) { mutableStateOf(false) }
    var activityFilter by rememberSaveable(name) { mutableStateOf("All") }
    if (selectedSession != null || startingAgent) {
        CodingAgentsScreen(repository, name, startImmediately = startingAgent, initialSession = selectedSession,
            onBack = { selectedSession = null; startingAgent = false })
        return
    }
    var thread by rememberSaveable(name) { mutableStateOf<String?>(null) }
    var run by rememberSaveable(name) { mutableStateOf<String?>(null) }
    val id = project.value?.projectId
    if (thread != null && id != null) { ProjectThreadScreen(repository, name, id, thread!!.toULong(), onBack = { thread = null }); return }
    if (run != null && id != null) { ProjectRunScreen(repository, id, run!!.toULong(), onBack = { run = null }); return }
    if (tab == "Agents") { CodingAgentsScreen(repository, name, onBack = { tab = "Overview" }); return }
    if (tab == "Terminals") { TerminalsScreen(repository, name, onBack = { tab = "Overview" }); return }
    var refreshHost by remember(name) { mutableStateOf(false) }
    val lists = workspaceLoad(id) { id?.let { api.projectLists(it, refreshHost) } }
    val runs = workspaceLoad(id) { id?.let { api.projectRuns(it, refreshHost) } }
    val branches = workspaceLoad(name) { api.projectBranches(name) }
    val machines = workspaceLoad(name) { api.projectMachines(name) }
    val sessions = workspaceLoad(name) { api.codingSessions(name) }
    var query by rememberSaveable(tab) { mutableStateOf("") }
    fun refresh() { refreshHost = true; project.refresh(); lists.refresh(); runs.refresh(); branches.refresh(); machines.refresh(); sessions.refresh() }
    WorkspacePage(name.substringAfterLast('/'), onBack, ::refresh,
        tabs = { WorkspaceTabs(tab, listOf("Overview", "PRs", "Issues", "CI runs", "Agents", "Terminals", "Activity")) { tab = it } },
        actions = { TextButton(onClick = { startingAgent = true }) { Text("Start agent", fontSize = 12.sp) } }) {
        WorkspaceBody {
            WorkspaceError(project.error, project::refresh)
            if (project.loading && project.value == null) WorkspaceLoading()
            when (tab) {
                "Overview" -> {
                    WorkspaceFacts(listOf("Branch" to (branches.value?.defaultBranch ?: "Unavailable"),
                        "Checkout" to (machines.value?.firstOrNull { it.folder != null }?.machineName ?: "Unavailable"),
                        "Pull requests" to (lists.value?.pullRequests?.size?.toString() ?: "Unavailable"),
                        "Issues" to (lists.value?.issues?.size?.toString() ?: "Unavailable")))
                    WorkspaceError(branches.error, branches::refresh)
                    WorkspaceHeading("Coding agents", "Start") { startingAgent = true }
                    WorkspaceError(sessions.error, sessions::refresh)
                    BeepCard(Modifier.fillMaxWidth()) { sessions.value.orEmpty().take(3).forEach { session ->
                        WorkspaceRow(Icons.Outlined.Code, session.title ?: session.agent, session.machineName, session.status.workspaceLabel(), onClick = { selectedSession = session })
                    } }
                    if (sessions.value?.isEmpty() == true) WorkspaceEmpty("No coding sessions")
                    WorkspaceHeading("Pull requests", "View all") { tab = "PRs" }
                    ProjectPullRows(lists.value, "", { thread = it.toString() }, limit = 3)
                    WorkspaceError(lists.error, lists::refresh)
                    WorkspaceHeading("Latest run")
                    WorkspaceError(runs.error, runs::refresh)
                    runs.value?.let { found ->
                        WorkspaceError(found.error)
                        found.runs.firstOrNull()?.let { latest -> BeepCard { RunRow(latest) { run = latest.id.toString() } } }
                        if (found.runs.isEmpty()) WorkspaceEmpty("No CI runs")
                    }
                    WorkspaceHeading("Checkout")
                    WorkspaceError(machines.error, machines::refresh)
                    BeepCard { machines.value.orEmpty().forEach { machine ->
                        WorkspaceRow(Icons.Outlined.Computer, machine.machineName, machine.folder ?: "No checkout",
                            if (machine.online) "Online" else "Offline")
                    } }
                }
                "PRs" -> {
                    WorkspaceSearch(query, "Filter pull requests") { query = it }
                    WorkspaceError(lists.error, lists::refresh)
                    if (lists.loading && lists.value == null) WorkspaceLoading()
                    ProjectPullRows(lists.value, query, { thread = it.toString() })
                }
                "Issues" -> {
                    WorkspaceSearch(query, "Filter issues") { query = it }
                    WorkspaceError(lists.error, lists::refresh)
                    WorkspaceError(lists.value?.issuesError)
                    if (lists.loading && lists.value == null) WorkspaceLoading()
                    val issues = lists.value?.issues.orEmpty().filter { (it.title + " #" + it.number + " " + it.labels.joinToString()).contains(query, true) }
                    WorkspaceHeading("Open (${issues.size})")
                    BeepCard { issues.forEach { issue -> WorkspaceRow(Icons.Outlined.ErrorOutline, issue.title,
                        "#${issue.number} · ${issue.labels.joinToString(", ")}", "${issue.comments} comments", onClick = { thread = issue.number.toString() }) } }
                    if (issues.isEmpty() && !lists.loading && lists.error == null && lists.value?.issuesError == null) WorkspaceEmpty("No matching open issues")
                }
                "CI runs" -> {
                    WorkspaceSearch(query, "Filter runs") { query = it }
                    WorkspaceHeading("Recent runs")
                    WorkspaceError(runs.error, runs::refresh)
                    WorkspaceError(runs.value?.error)
                    if (runs.loading && runs.value == null) WorkspaceLoading()
                    val filtered = runs.value?.runs.orEmpty().filter { (it.workflow + it.branch + it.number).contains(query, true) }
                    BeepCard { filtered.forEach { item -> RunRow(item) { run = item.id.toString() } } }
                    if (filtered.isEmpty() && !runs.loading && runs.error == null && runs.value?.error == null) WorkspaceEmpty("No matching CI runs")
                }
                "Activity" -> {
                    WorkspaceTabs(activityFilter, listOf("All", "Agents", "Git")) { activityFilter = it }
                    WorkspaceHeading("Recent activity")
                    WorkspaceError(sessions.error, sessions::refresh)
                    if (activityFilter != "Git") BeepCard { sessions.value.orEmpty().sortedByDescending { it.lastActivity }.take(20).forEach { item ->
                        WorkspaceRow(Icons.Outlined.Code, item.title ?: item.agent, item.doing ?: item.machineName, item.status.workspaceLabel(), onClick = { selectedSession = item })
                    } }
                    WorkspaceError(runs.error, runs::refresh)
                    if (activityFilter != "Agents") BeepCard { runs.value?.runs.orEmpty().take(10).forEach { item -> RunRow(item) { run = item.id.toString() } } }
                    if (sessions.value?.isEmpty() == true && runs.value?.runs?.isEmpty() == true) WorkspaceEmpty("No recent activity")
                }
            }
        }
    }
}

@Composable
private fun ProjectPullRows(lists: MobileProjectLists?, query: String, onOpen: (ULong) -> Unit, limit: Int = Int.MAX_VALUE) {
    WorkspaceError(lists?.prsError)
    if (lists == null) return
    val filtered = lists.pullRequests.filter { (it.title + " #" + it.number).contains(query, true) }.take(limit)
    BeepCard { filtered.forEach { pr -> WorkspaceRow(Icons.AutoMirrored.Outlined.CallMerge, pr.title, "#${pr.number} · ${pr.author}",
        if (pr.draft) "Draft" else "Open", onClick = { onOpen(pr.number) }) } }
    if (filtered.isEmpty() && lists.prsError == null) WorkspaceEmpty("No matching open pull requests")
}

@Composable
private fun ProjectThreadScreen(repository: ChatRepository, project: String, projectId: String, number: ULong, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val thread = workspaceLoad("$projectId/$number") { api.threadDetail(projectId, number) }
    var showDiff by rememberSaveable { mutableStateOf(false) }
    var start by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val uri = LocalUriHandler.current
    var error by remember { mutableStateOf<String?>(null) }
    if (showDiff) { PullDiffScreen(repository, project, projectId, number, onBack = { showDiff = false }); return }
    if (start) { CodingAgentsScreen(repository, project, initialPrompt = "Work on issue #$number: ${thread.value?.title.orEmpty()}", startImmediately = true, onBack = { start = false }); return }
    val data = thread.value
    WorkspacePage(if (data?.kind == GitItemKind.PULL_REQUEST) "PR #$number" else "Issue #$number", onBack, thread::refresh) {
        WorkspaceBody {
            WorkspaceError(thread.error, thread::refresh)
            WorkspaceError(error)
            if (thread.loading && data == null) WorkspaceLoading()
            data?.let { item ->
                WorkspaceHeading(item.title)
                WorkspaceFacts(listOf("Status" to item.state.workspaceLabel(), "Author" to item.author,
                    "Labels" to item.labels.joinToString(", ").ifEmpty { "None" }, "Review" to (item.review.workspaceLabel())))
                item.checks?.let { WorkspaceTag("Checks: ${it.workspaceLabel()}") }
                BeepCard { WorkspaceProse(item.body.ifEmpty { "No description" }) }
                if (item.kind == GitItemKind.PULL_REQUEST) Button(onClick = { showDiff = true }) { Text("Review diff") }
                else Button(onClick = { start = true }) { Text("Start agent") }
                WorkspaceHeading("Activity")
                if (item.earlier > 0u) Text("${item.earlier} earlier comments omitted", fontSize = 12.sp, color = Beep.colors.muted)
                item.comments.forEach { comment -> BeepCard {
                    Text(comment.author, Modifier.padding(start = 12.dp, top = 12.dp), fontSize = 12.sp, fontWeight = FontWeight.Medium)
                    WorkspaceProse(comment.body)
                } }
                OutlinedButton(onClick = { scope.launch { workspaceAction({ error = it }) { uri.openUri(item.url) } } }) { Text("Open in browser") }
            }
        }
    }
}

@Composable
private fun PullDiffScreen(repository: ChatRepository, project: String, id: String, number: ULong, onBack: () -> Unit) {
    val api = repository.workspaceApi()
    val diff = workspaceLoad("$id/$number") {
        val head = api.pullRequestHead(id, number)
        check(!head.fork) { "This pull request comes from a fork. Open it in your browser to review the diff." }
        api.branchDiff(project, head.branch)
    }
    WorkspacePage("PR #$number diff", onBack, diff::refresh) {
        WorkspaceBody { WorkspaceError(diff.error, diff::refresh); if (diff.loading && diff.value == null) WorkspaceLoading(); diff.value?.let { WorkspaceDiff(it) } }
    }
}

@Composable
internal fun WorkspaceDiff(diff: WorkDiff) {
    var query by rememberSaveable { mutableStateOf("") }
    WorkspaceSearch(query, "Filter files") { query = it }
    WorkspaceFacts(listOf("Base" to diff.base, "Changes" to "+${diff.additions} -${diff.deletions}"))
    if (diff.more > 0u) WorkspaceEmpty("${diff.more} additional files omitted")
    diff.files.filter { it.path.contains(query, true) }.forEach { file ->
        var expanded by remember(file.path) { mutableStateOf(file.additions + file.deletions <= 400u && file.status != DiffStatus.DELETED) }
        BeepCard(Modifier.fillMaxWidth()) {
            WorkspaceRow(Icons.AutoMirrored.Outlined.InsertDriveFile, file.path, file.oldPath.orEmpty(), "+${file.additions} -${file.deletions}", onClick = { expanded = !expanded })
            if (expanded) {
                file.leftOut?.let { Text("Lines omitted: ${it.workspaceLabel()}", Modifier.padding(12.dp), fontSize = 12.sp, color = Beep.colors.muted) }
                file.hunks.forEach { hunk ->
                    Text("@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@ ${hunk.section}",
                        Modifier.padding(12.dp), fontSize = 11.sp, color = Beep.colors.muted)
                    val added = Beep.colors.ok
                    val removed = Beep.colors.error
                    val ink = Beep.colors.ink
                    val lines = remember(hunk, added, removed, ink) {
                        androidx.compose.ui.text.buildAnnotatedString {
                            hunk.lines.forEach { line ->
                                val prefix = when (line.kind) { DiffLineKind.ADDED -> "+"; DiffLineKind.REMOVED -> "-"; else -> " " }
                                pushStyle(androidx.compose.ui.text.SpanStyle(color = when (line.kind) {
                                    DiffLineKind.ADDED -> added; DiffLineKind.REMOVED -> removed; else -> ink
                                }))
                                append("$prefix ${line.text}\n")
                                pop()
                            }
                        }
                    }
                    androidx.compose.foundation.text.selection.SelectionContainer {
                        Text(lines, Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                            fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace, fontSize = 11.sp, lineHeight = 18.sp)
                    }
                }
            }
        }
    }
    if (diff.files.isEmpty()) WorkspaceEmpty("No changes")
}

@Composable
private fun RunRow(run: MobileRun, onClick: () -> Unit) {
    WorkspaceRow(Icons.Outlined.PlayCircleOutline, run.workflow, "#${run.number} · ${run.branch}", run.state.workspaceLabel(), onClick,
        when (run.state) { RunState.PASSED -> Beep.colors.ok; RunState.FAILED -> Beep.colors.error; RunState.RUNNING -> Beep.colors.brand; else -> Beep.colors.muted })
}

@Composable
private fun ProjectRunScreen(repository: ChatRepository, projectId: String, runId: ULong, onBack: () -> Unit) {
    val data = workspaceLoad("$projectId/$runId", poll = true) { repository.workspaceApi().projectRun(projectId, runId) }
    WorkspacePage("CI run", onBack, data::refresh) {
        WorkspaceBody {
            WorkspaceError(data.error, data::refresh)
            if (data.loading && data.value == null) WorkspaceLoading()
            data.value?.let { detail ->
                WorkspaceHeading(detail.run.workflow)
                WorkspaceFacts(listOf("Branch" to detail.run.branch, "Status" to detail.run.state.workspaceLabel(), "Trigger" to detail.run.event, "Run" to "#${detail.run.number}"))
                WorkspaceError(detail.error)
                WorkspaceHeading("Jobs")
                detail.jobs.forEach { job -> BeepCard {
                    WorkspaceRow(Icons.Outlined.CheckCircleOutline, job.name, tag = job.state.workspaceLabel())
                    job.steps.forEach { step -> Text("${step.name} · ${step.state.workspaceLabel()}", Modifier.padding(horizontal = 12.dp, vertical = 6.dp), fontSize = 12.sp, color = Beep.colors.muted) }
                } }
                detail.log?.let { WorkspaceHeading("Log"); BeepCard { WorkspaceCode(it) } }
            }
        }
    }
}
