package com.flashmind.beep.center

import android.Manifest
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.flashmind.beep.Beep
import com.flashmind.beep.ChatRepository
import com.flashmind.beep.CodingAgentsScreen
import com.flashmind.beep.LinkedDevicesScreen
import com.flashmind.beep.LocalTabRoot
import com.flashmind.beep.MachinesScreen
import com.flashmind.beep.ProjectsScreen
import com.flashmind.beep.TerminalsScreen
import com.flashmind.beep.WorkspaceError
import com.flashmind.beep.WorkspaceLoading
import com.flashmind.beep.WorkspacePage
import com.flashmind.beep.proto.MachineSession
import com.flashmind.beep.userMessage
import kotlinx.coroutines.CancellationException

private enum class CenterTab(val label: String, val icon: ImageVector) {
    Home("Home", Icons.Outlined.Home), Projects("Projects", Icons.Outlined.Folder), Agents("Agents", Icons.Outlined.Code),
    Terminals("Terminals", Icons.Outlined.Terminal), Machines("Machines", Icons.Outlined.Dns)
}

/** A screen pushed over the tabs. */
private sealed interface Pushed {
    data object Bell : Pushed
    data object Settings : Pushed
    data object Appearance : Pushed
    data object Notifications : Pushed
    data object Devices : Pushed
    data object StartAgent : Pushed
    data class Session(val session: MachineSession) : Pushed
    data class SessionById(val machine: String, val agent: String, val id: String) : Pushed
    data class Project(val name: String) : Pushed
}

/** What a linked phone opens on: the five tabs of the command center. */
@Composable
internal fun CenterRoot(repository: ChatRepository) {
    val context = LocalContext.current
    var tab by rememberSaveable { mutableStateOf(CenterTab.Home) }
    val stack = remember { mutableStateListOf<Pushed>() }
    val model = remember(repository) {
        HomeModel(load = { repository.workspaceApi().home() },
            send = { machine, agent, session, question, reply -> repository.workspaceApi().answerCoding(machine, agent, session, question, reply) })
    }
    fun push(screen: Pushed) { stack.add(screen) }
    fun pop() { if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) }

    // The notification permission is asked for when the user turns on a setting or the first ask arrives.
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    val wants by AskCenter.wantsPermission.collectAsState()
    LaunchedEffect(wants) {
        if (wants) { AskCenter.permissionAsked(); permission.launch(Manifest.permission.POST_NOTIFICATIONS) }
    }

    // The feed is open while the app shows, unless the service holds it. Only one feed runs.
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> {
                    AskCenter.start(context)
                    if (NotifyPrefs.on(context, NotifyPrefs.STAY)) StayConnectedService.start(context)
                }
                Lifecycle.Event.ON_STOP -> AskCenter.stop()
                else -> {}
            }
        }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer); AskCenter.stop() }
    }

    BackHandler(enabled = stack.isEmpty() && tab != CenterTab.Home) { tab = CenterTab.Home }
    val top = stack.lastOrNull()
    if (top != null) {
        Box(Modifier.fillMaxSize().background(Beep.colors.page)) {
            when (top) {
                Pushed.Bell -> Padded(::pop) { BellScreen(::pop) { notice ->
                    if (notice.machineId.isNotEmpty()) push(Pushed.SessionById(notice.machineId, notice.agent, notice.session))
                } }
                Pushed.Settings -> Padded(::pop) { CenterSettings(::pop, { push(Pushed.Appearance) }, { push(Pushed.Notifications) }, { push(Pushed.Devices) }) }
                Pushed.Appearance -> Padded(::pop) { AppearanceSettings(::pop) }
                Pushed.Notifications -> Padded(::pop) { NotifySettings(::pop) { permission.launch(Manifest.permission.POST_NOTIFICATIONS) } }
                Pushed.Devices -> LinkedDevicesScreen(repository, onBack = ::pop, onWorkspaceLinked = ::pop)
                Pushed.StartAgent -> CodingAgentsScreen(repository, startImmediately = true, onBack = ::pop)
                is Pushed.Session -> CodingAgentsScreen(repository, initialSession = top.session, onBack = ::pop)
                is Pushed.SessionById -> SessionById(repository, top, ::pop)
                is Pushed.Project -> ProjectsScreen(repository, initialProject = top.name, onBack = ::pop)
            }
        }
        return
    }
    Column(Modifier.fillMaxSize().background(Beep.colors.page)) {
        Box(Modifier.weight(1f)) {
            CompositionLocalProvider(LocalTabRoot provides true) {
                when (tab) {
                    CenterTab.Home -> Box(Modifier.statusBarsPadding()) { HomeScreen(repository, model, openBell = { push(Pushed.Bell) }, openSettings = { push(Pushed.Settings) },
                        openMachines = { tab = CenterTab.Machines }, openSession = { push(Pushed.Session(it)) },
                        openProject = { push(Pushed.Project(it)) }, startAgent = { push(Pushed.StartAgent) }) }
                    CenterTab.Projects -> ProjectsScreen(repository, onBack = { tab = CenterTab.Home })
                    CenterTab.Agents -> CodingAgentsScreen(repository, onBack = { tab = CenterTab.Home })
                    CenterTab.Terminals -> TerminalsScreen(repository, onBack = { tab = CenterTab.Home })
                    CenterTab.Machines -> MachinesScreen(repository, onBack = { tab = CenterTab.Home })
                }
            }
        }
        NavigationBar(containerColor = Beep.colors.surface, tonalElevation = 0.dp) {
            CenterTab.entries.forEach { item ->
                NavigationBarItem(selected = tab == item, onClick = { tab = item },
                    label = { Text(item.label, fontSize = 11.5.sp) },
                    colors = NavigationBarItemDefaults.colors(selectedIconColor = Beep.colors.ink, selectedTextColor = Beep.colors.ink,
                        unselectedIconColor = Beep.colors.muted, unselectedTextColor = Beep.colors.muted,
                        indicatorColor = Beep.colors.accent.copy(alpha = 0.28f)),
                    icon = {
                        BadgedBox(badge = { if (item == CenterTab.Home && tab != CenterTab.Home && model.badge > 0) Badge(containerColor = Beep.colors.warn) }) {
                            Icon(item.icon, item.label)
                        }
                    })
            }
        }
    }
}

/** The session a notification names, found among the sessions of the mesh. */
@Composable
private fun SessionById(repository: ChatRepository, key: Pushed.SessionById, back: () -> Unit) {
    var found by remember(key) { mutableStateOf<MachineSession?>(null) }
    var error by remember(key) { mutableStateOf<String?>(null) }
    LaunchedEffect(key) {
        try {
            found = repository.workspaceApi().codingSessions(null).firstOrNull { it.machineId == key.machine && it.agent == key.agent && it.id == key.id }
            if (found == null) error = "This session is no longer there."
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failed: Exception) {
            error = userMessage(failed)
        }
    }
    val session = found
    if (session != null) CodingAgentsScreen(repository, initialSession = session, onBack = back)
    else WorkspacePage("Session", back) {
        Column(Modifier.padding(16.dp)) { if (error == null) WorkspaceLoading() else WorkspaceError(error) }
    }
}

/** A screen drawn with the command center's own top bar, so it keeps clear of the system bars. */
@Composable
private fun Padded(back: () -> Unit, content: @Composable () -> Unit) {
    BackHandler(onBack = back)
    Box(Modifier.fillMaxSize().systemBarsPadding()) { content() }
}
