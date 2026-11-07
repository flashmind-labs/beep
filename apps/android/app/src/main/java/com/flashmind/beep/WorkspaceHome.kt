package com.flashmind.beep

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier

private enum class PhoneTab { Projects, Agents, Machines, Terminals, Devices }

@Composable
internal fun WorkspaceHome(repository: ChatRepository) {
    var tab by rememberSaveable { mutableStateOf(PhoneTab.Projects) }
    var loaded by remember { mutableStateOf(false) }
    LaunchedEffect(repository) {
        tab = if (runCatching { repository.workspaceApi().workspaceLinked() }.getOrDefault(false)) PhoneTab.Projects else PhoneTab.Devices
        loaded = true
    }
    Column(Modifier.fillMaxSize()) {
        Box(Modifier.weight(1f)) {
            if (!loaded) WorkspaceLoading()
            else when (tab) {
                PhoneTab.Projects -> ProjectsScreen(repository, onBack = { tab = PhoneTab.Devices })
                PhoneTab.Agents -> CodingAgentsScreen(repository, onBack = { tab = PhoneTab.Projects })
                PhoneTab.Machines -> MachinesScreen(repository, onBack = { tab = PhoneTab.Projects })
                PhoneTab.Terminals -> TerminalsScreen(repository, onBack = { tab = PhoneTab.Projects })
                PhoneTab.Devices -> LinkedDevicesScreen(repository, onBack = { tab = PhoneTab.Projects }, onWorkspaceLinked = { tab = PhoneTab.Projects })
            }
        }
        NavigationBar(containerColor = Beep.colors.page) {
            PhoneTab.entries.forEach { item ->
                NavigationBarItem(selected = tab == item, onClick = { tab = item }, label = { Text(item.name) }, icon = {
                    Icon(when (item) {
                        PhoneTab.Projects -> Icons.Outlined.Folder
                        PhoneTab.Agents -> Icons.Outlined.Code
                        PhoneTab.Machines -> Icons.Outlined.Dns
                        PhoneTab.Terminals -> Icons.Outlined.Terminal
                        PhoneTab.Devices -> Icons.Outlined.Devices
                    }, contentDescription = item.name)
                })
            }
        }
    }
}
