package com.flashmind.beep

import android.content.Context
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.supervisorScope

internal data class WorkspaceSignals(val github: Boolean = false, val projects: Boolean = false, val machines: Int = 0) {
    // The protocol has no laptop identity. Two machines establish that a lone laptop is not all the account uses.
    val expanded get() = github || projects || machines > 1
}

internal class AccountMode(private val context: Context, private val account: String) {
    private val prefs = context.getSharedPreferences("workspace-mode", Context.MODE_PRIVATE)
    var showEverything by mutableStateOf(prefs.getBoolean("show-$account", false))
        private set
    var unlocked by mutableStateOf(prefs.getBoolean("unlocked-$account", false))
        private set
    val expanded get() = showEverything || unlocked
    fun show(value: Boolean) {
        showEverything = value
        prefs.edit().putBoolean("show-$account", value).apply()
    }
    fun observe(signals: WorkspaceSignals) {
        if (!signals.expanded || unlocked) return
        unlocked = true
        prefs.edit().putBoolean("unlocked-$account", true).apply()
    }
}

@Composable
internal fun rememberAccountMode(repository: ChatRepository, refreshKey: Any): AccountMode {
    val context = LocalContext.current.applicationContext
    val account by repository.accountName.collectAsState()
    val connected by repository.connected.collectAsState()
    val settings by repository.settings.collectAsState()
    val mode = remember(account, settings.nodeId) { AccountMode(context, "${settings.nodeId}/$account") }
    val owner = LocalLifecycleOwner.current
    LaunchedEffect(mode, connected, refreshKey, owner) {
        if (!connected || account.isEmpty()) return@LaunchedEffect
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            do {
                supervisorScope {
                    val api = repository.workspaceApi()
                    val github = async { optionalSignal { api.githubAccounts().isNotEmpty() } }
                    val githubApp = async { optionalSignal { repository.listApps().any { it.slug.equals("github", true) && it.connected } } }
                    val projects = async { optionalSignal { api.listProjects().isNotEmpty() || api.listImportedProjects().isNotEmpty() } }
                    val machines = async { optionalSignal { repository.listMachines().size > 1 } }
                    mode.observe(WorkspaceSignals(github.await() || githubApp.await(), projects.await(), if (machines.await()) 2 else 0))
                }
                delay(30_000)
            } while (!mode.unlocked)
        }
    }
    return mode
}

private suspend fun optionalSignal(read: suspend () -> Boolean): Boolean = try { read() }
catch (e: CancellationException) { throw e }
catch (_: Exception) { false } // An unavailable optional list cannot establish an unlock signal.
