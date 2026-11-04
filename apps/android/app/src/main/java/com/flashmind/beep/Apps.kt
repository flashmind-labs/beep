package com.flashmind.beep

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.selection.selectable
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.flashmind.beep.proto.AppAccount
import com.flashmind.beep.proto.AppIntegration
import com.flashmind.beep.proto.AppAction
import com.flashmind.beep.proto.AppKind
import com.flashmind.beep.proto.AppPermissions
import com.flashmind.beep.proto.McpConnect
import com.flashmind.beep.proto.McpServer
import com.flashmind.beep.proto.McpTools
import com.flashmind.beep.proto.PermissionLevel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.HttpURLConnection
import java.net.URL

/**
 * The apps Beep can work in. Each one connects with a single tap: Composio apps
 * open their sign-in page in the browser, and the list refreshes when you come back.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AppsScreen(repository: ChatRepository, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val cached = remember { repository.cachedApps() }
    var apps by remember { mutableStateOf(cached.orEmpty()) }
    var loading by remember { mutableStateOf(cached == null) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf<String?>(null) }
    var open by remember { mutableStateOf<AppIntegration?>(null) }
    // Null until the server answers; a server too old to know them never does, and the section stays hidden.
    var servers by remember { mutableStateOf<List<McpServer>?>(null) }
    var openServer by remember { mutableStateOf<McpServer?>(null) }
    var adding by remember { mutableStateOf(false) }
    // Fastmail's accounts; null while unknown or from a server too old to list them.
    var accounts by remember { mutableStateOf<List<AppAccount>?>(null) }

    fun refresh() { scope.launch {
        try {
            apps = withContext(Dispatchers.IO) { repository.listApps() }
            val fastmail = apps.firstOrNull { it.kind == AppKind.FASTMAIL }
            accounts = when {
                fastmail == null -> null
                !fastmail.connected -> emptyList()
                else -> withContext(Dispatchers.IO) { runCatching { repository.listAppAccounts(fastmail.slug) }.getOrNull() }
            }
            open = open?.let { o -> apps.firstOrNull { it.slug == o.slug && it.connected } }
            servers = withContext(Dispatchers.IO) { runCatching { repository.listMcpServers() }.getOrNull() } ?: servers
            openServer = openServer?.let { o -> servers?.firstOrNull { it.id == o.id } }
            error = ""
        }
        catch (e: Exception) { error = userMessage(e) }
        finally { loading = false; busy = null }
    } }
    fun browse(url: String) =
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    /** Opens the server's sign-in when it needs one; the link back finishes it. */
    fun connected(result: McpConnect) {
        result.authUrl?.let { browse(it) } ?: refresh()
    }
    fun signIn(server: McpServer) { scope.launch {
        busy = server.id
        try { connected(withContext(Dispatchers.IO) { repository.connectMcpServer(server.id) }) }
        catch (e: Exception) { error = userMessage(e); busy = null }
    } }
    fun removeServer(server: McpServer) { scope.launch {
        busy = server.id
        try { withContext(Dispatchers.IO) { repository.removeMcpServer(server.id) }; refresh() }
        catch (e: Exception) { error = userMessage(e); busy = null }
    } }
    fun connect(app: AppIntegration, add: Boolean = false) { scope.launch {
        busy = app.slug
        if (app.kind == AppKind.FASTMAIL) {
            val appContext = context.applicationContext
            repository.authorizeApp(app.slug, add) { url ->
                appContext.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
            return@launch
        }
        try {
            val url = withContext(Dispatchers.IO) { repository.connectApp(app.slug) }
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (e: Exception) { error = userMessage(e); busy = null }
    } }
    fun removeAccount(app: AppIntegration, account: AppAccount) { scope.launch {
        busy = "account:" + account.id
        try { withContext(Dispatchers.IO) { repository.removeAppAccount(app.slug, account.id) }; refresh() }
        catch (e: Exception) { error = userMessage(e); busy = null }
    } }
    fun disconnect(app: AppIntegration) { scope.launch {
        busy = app.slug
        try { withContext(Dispatchers.IO) { repository.disconnectApp(app.slug) }; refresh() }
        catch (e: Exception) { error = userMessage(e); busy = null }
    } }

    // A localhost sign-in finishes after the browser step, often after ON_RESUME already ran.
    LaunchedEffect(Unit) {
        repository.appAuthResults.collect { result ->
            if (result == null) return@collect
            result.error?.let { error = it }
            repository.appAuthResults.value = null
            refresh()
        }
    }

    LaunchedEffect(Unit) {
        repository.mcpAuthResults.collect { result ->
            if (result == null) return@collect
            error = result.error ?: ""
            repository.mcpAuthResults.value = null
            refresh()
        }
    }

    // Coming back from the browser after signing in is the cue to look again.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) refresh() }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    open?.let { app ->
        BackHandler { open = null }
        val appAccounts = accounts.takeIf { app.kind == AppKind.FASTMAIL }
        AppPermissionsScreen(repository, app, onBack = { open = null },
            onDisconnect = { open = null; disconnect(app) },
            accounts = appAccounts?.let { list ->
                AccountsSection(list, busy, onAdd = { connect(app, add = true) }, onRemove = { removeAccount(app, it) })
            })
        return
    }
    openServer?.let { server ->
        // Back refreshes the list, whose tool count the toggles may have changed.
        BackHandler { openServer = null; refresh() }
        McpServerScreen(repository, server, busy == server.id, error, onBack = { openServer = null; refresh() },
            onSignIn = { signIn(server) }, onRemove = { openServer = null; removeServer(server) })
        return
    }
    if (adding) AddMcpServerDialog(repository, onDismiss = { adding = false }, onAdded = { result ->
        adding = false
        connected(result)
        refresh()
    })

    val t = Beep.colors
    Scaffold(containerColor = t.page, topBar = {
        TopAppBar(title = { Text(displayText("Apps"), style = displayStyle(18.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.page))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)) {
            item {
                Text("Connect an app once, then just ask. \"What's on my calendar?\" or \"Add this to Notion\" work from any chat.",
                    color = t.muted, fontSize = 14.sp, lineHeight = 21.sp, modifier = Modifier.padding(bottom = 6.dp))
            }
            if (error.isNotEmpty()) item { Text(error, color = t.error, fontSize = 13.sp) }
            if (apps.isNotEmpty() && apps.none { it.kind == AppKind.COMPOSIO && (it.configured || it.connected) }) item {
                Text("Connecting apps needs a Composio API key on the server.", color = t.warn, fontSize = 13.sp)
            }
            if (loading) item {
                Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = t.ok)
                }
            }
            val (connected, available) = apps.partition { it.connected }
            if (connected.isNotEmpty()) {
                item { SectionLabel("Connected") }
                items(connected, key = { "c-" + it.slug }) { app ->
                    val addresses = accounts?.takeIf { app.kind == AppKind.FASTMAIL && it.isNotEmpty() }
                        ?.joinToString(", ") { it.address ?: "Fastmail account" }
                    AppRow(app, busy == app.slug, onConnect = { connect(app) }, onOpen = { open = app }, detail = addresses)
                }
            }
            if (available.isNotEmpty()) {
                item { SectionLabel(if (connected.isEmpty()) "Available" else "More apps") }
                items(available, key = { "a-" + it.slug }) { app ->
                    AppRow(app, busy == app.slug, onConnect = { connect(app) }, onOpen = { open = app })
                }
            }
            val servers = servers
            if (!loading && servers != null) {
                item { SectionLabel("Your servers") }
                if (servers.isEmpty()) item {
                    Text("Add any remote MCP server by its address. Beep asks before every action it takes there.",
                        color = t.muted, fontSize = 13.sp, lineHeight = 18.sp)
                }
                items(servers, key = { "m-" + it.id }) { server ->
                    McpServerRow(server, busy == server.id, onSignIn = { signIn(server) }, onOpen = { openServer = server })
                }
                item {
                    ChunkyButton(onClick = { adding = true }, color = t.surface, contentColor = t.ink,
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp)) {
                        Text("+ Add MCP server", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(text.uppercase(), color = Beep.colors.dim, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
        letterSpacing = 1.2.sp, modifier = Modifier.padding(top = 10.dp, bottom = 2.dp))
}

@Composable
private fun AppRow(app: AppIntegration, busy: Boolean, onConnect: () -> Unit, onOpen: () -> Unit, detail: String? = null) {
    val t = Beep.colors
    BeepCard(Modifier.fillMaxWidth().then(if (app.connected) Modifier.clickable(onClick = onOpen) else Modifier)) {
        Row(Modifier.padding(BeepSpace.row), verticalAlignment = Alignment.CenterVertically) {
            AppLogo(app)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(app.name, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(2.dp))
                Text(detail ?: app.description, fontSize = 12.sp, color = t.muted, lineHeight = 16.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(10.dp))
            when {
                busy -> CircularProgressIndicator(Modifier.padding(horizontal = 18.dp).size(20.dp), strokeWidth = 2.dp, color = t.ok)
                app.connected -> ConnectedChip()
                !app.configured -> Text("Unavailable",
                    fontSize = 12.sp, color = t.dim, modifier = Modifier.width(76.dp))
                else -> ChunkyButton(onClick = onConnect, color = t.accent, contentColor = t.onAccent,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
                    Text("Connect", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

/** The host a server URL names, for the row under its name. */
private fun hostOf(url: String): String = Uri.parse(url).host ?: url

/** A tool's name as the server gave it, such as `search_pages`, in words. */
private fun mcpToolLabel(name: String): String =
    name.replace('_', ' ').replace('-', ' ').trim().replaceFirstChar { it.uppercase() }

@Composable
private fun McpServerRow(server: McpServer, busy: Boolean, onSignIn: () -> Unit, onOpen: () -> Unit) {
    val t = Beep.colors
    BeepCard(Modifier.fillMaxWidth().clickable(onClick = onOpen)) {
        Row(Modifier.padding(BeepSpace.row), verticalAlignment = Alignment.CenterVertically) {
            AppLogo(server.id, server.name)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(server.name, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(2.dp))
                Text(hostOf(server.url), fontSize = 12.sp, color = t.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(10.dp))
            when {
                busy -> CircularProgressIndicator(Modifier.padding(horizontal = 18.dp).size(20.dp), strokeWidth = 2.dp, color = t.ok)
                server.connected -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(if (server.tools.size == 1) "1 tool" else "${server.tools.size} tools",
                        fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
                    Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(16.dp), tint = t.muted)
                }
                else -> ChunkyButton(onClick = onSignIn, color = t.accent, contentColor = t.onAccent,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
                    Text(if (server.signIn) "Sign in" else "Retry", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

/** Name and URL of a new server. Adding it connects, and returns the sign-in page when it needs one. */
@Composable
private fun AddMcpServerDialog(repository: ChatRepository, onDismiss: () -> Unit, onAdded: (McpConnect) -> Unit) {
    val t = Beep.colors
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var working by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    fun add() { scope.launch {
        working = true
        error = ""
        try { onAdded(withContext(Dispatchers.IO) { repository.addMcpServer(name.trim(), url.trim()) }) }
        catch (e: Exception) { error = userMessage(e) }
        finally { working = false }
    } }
    val fieldColors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = t.surface, focusedContainerColor = t.surface)
    AlertDialog(onDismissRequest = { if (!working) onDismiss() }, containerColor = t.surface,
        title = { Text(displayText("Add MCP server"), style = displayStyle(18.sp)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(name, { name = it.take(60) }, label = { Text("Name") }, singleLine = true, enabled = !working,
                modifier = Modifier.fillMaxWidth(), placeholder = { Text("Team wiki") }, colors = fieldColors,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next))
            OutlinedTextField(url, { url = it.take(2000) }, label = { Text("Server URL") }, singleLine = true, enabled = !working,
                modifier = Modifier.fillMaxWidth(), placeholder = { Text("https://example.com/mcp") }, colors = fieldColors,
                textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp),
                isError = error.isNotEmpty(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done))
            when {
                working -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = t.ok)
                    Text("Connecting to the server…", fontSize = 13.sp, color = t.muted)
                }
                error.isNotEmpty() -> Text(error, fontSize = 13.sp, color = t.error, lineHeight = 18.sp)
                else -> Text("Use the server's https address. If it needs an account, Beep opens its sign-in in the browser.",
                    fontSize = 12.sp, color = t.muted, lineHeight = 17.sp)
            }
        } },
        confirmButton = { ChunkyButton(onClick = ::add, color = t.accent, contentColor = t.onAccent,
            enabled = !working && name.isNotBlank() && url.isNotBlank()) {
            Text(if (error.isNotEmpty()) "Try again" else "Add", fontWeight = FontWeight.SemiBold)
        } },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !working) { Text("Cancel") } })
}

/** One added server: its address, whether it works, its tools with a switch each, and Remove. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun McpServerScreen(repository: ChatRepository, server: McpServer, busy: Boolean, error: String,
                            onBack: () -> Unit, onSignIn: () -> Unit, onRemove: () -> Unit) {
    val t = Beep.colors
    val scope = rememberCoroutineScope()
    // Null until the server answers; a server too old to list them leaves the plain list of names.
    var tools by remember(server.id) { mutableStateOf<McpTools?>(null) }
    var toolError by remember(server.id) { mutableStateOf("") }
    // The tool waiting on the warning about going past the limit.
    var asking by remember(server.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(server.id, server.connected) {
        if (server.connected) tools = withContext(Dispatchers.IO) { runCatching { repository.listMcpTools(server.id) }.getOrNull() }
    }
    fun set(tool: String, enabled: Boolean) { scope.launch {
        val before = tools ?: return@launch
        tools = before.copy(tools = before.tools.map { if (it.name == tool) it.copy(enabled = enabled) else it })
        toolError = ""
        try { tools = withContext(Dispatchers.IO) { repository.setMcpTool(server.id, tool, enabled) } }
        catch (e: Exception) { tools = before; toolError = userMessage(e) }
    } }
    fun toggle(tool: String, enabled: Boolean) {
        val list = tools ?: return
        if (enabled && list.tools.count { it.enabled } >= list.limit.toInt()) asking = tool else set(tool, enabled)
    }
    val on = tools?.tools?.count { it.enabled } ?: server.tools.size
    val total = tools?.tools?.size ?: server.tools.size
    asking?.let { tool ->
        val limit = tools?.limit ?: 25u
        AlertDialog(onDismissRequest = { asking = null }, containerColor = t.surface,
            title = { Text(displayText("Turn on more than $limit tools?"), style = displayStyle(18.sp)) },
            text = { Text("Beep works best with $limit tools or fewer from one server. With more, replies get slower and Beep is more likely to pick the wrong tool.",
                fontSize = 14.sp, color = t.muted, lineHeight = 20.sp) },
            confirmButton = { ChunkyButton(onClick = { asking = null; set(tool, true) }, color = t.accent, contentColor = t.onAccent) {
                Text("Turn on", fontWeight = FontWeight.SemiBold)
            } },
            dismissButton = { TextButton(onClick = { asking = null }) { Text("Cancel") } })
    }
    Scaffold(containerColor = t.page, topBar = {
        TopAppBar(title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                AppLogo(server.id, server.name, size = 32.dp)
                Spacer(Modifier.width(12.dp))
                Text(server.name, fontSize = 20.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }, navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.page))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)) {
            item {
                BeepCard(Modifier.fillMaxWidth()) {
                    Text(server.url, fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = t.ink,
                        maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp))
                }
            }
            if (error.isNotEmpty() || toolError.isNotEmpty()) item { Text(error.ifEmpty { toolError }, color = t.error, fontSize = 13.sp) }
            item {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(if (server.connected) t.ok else t.error))
                    Text(when {
                        server.connected && on == total -> if (on == 1) "Connected, 1 tool" else "Connected, $on tools"
                        server.connected -> "Connected, $on of $total tools on"
                        server.signIn -> "Sign-in expired"
                        else -> "Not connected"
                    }, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
                }
            }
            val list = tools
            if (server.connected && list != null && list.tools.isNotEmpty()) {
                item {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) { SectionLabel("Tools") }
                        Text("$on of ${list.limit} on", fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                            color = if (on > list.limit.toInt()) t.error else t.muted)
                    }
                }
                item {
                    BeepCard(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) {
                            list.tools.forEachIndexed { i, tool ->
                                if (i > 0) HorizontalDivider(color = t.softLine)
                                Row(Modifier.fillMaxWidth().heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Column(Modifier.weight(1f).padding(vertical = 6.dp)) {
                                        Text(mcpToolLabel(tool.name), fontSize = 14.sp, color = if (tool.usable) t.ink else t.muted)
                                        if (!tool.usable) Text("Too large for Beep to use", fontSize = 11.sp, color = t.muted)
                                    }
                                    Spacer(Modifier.width(10.dp))
                                    Switch(checked = tool.enabled, onCheckedChange = { toggle(tool.name, it) }, enabled = tool.usable,
                                        modifier = Modifier.semantics { contentDescription = "Use ${mcpToolLabel(tool.name)}" },
                                        colors = SwitchDefaults.colors(checkedTrackColor = t.ok))
                                }
                            }
                        }
                    }
                }
                item {
                    Text("Beep uses up to ${list.limit} tools from a server, so turn on the ones you need. It asks before every tool on a server you add. Tap Always when it asks to stop asking for this server.",
                        fontSize = 12.sp, color = t.muted, lineHeight = 17.sp, modifier = Modifier.padding(horizontal = 4.dp))
                }
            } else if (server.connected && server.tools.isNotEmpty()) {
                item { SectionLabel("Tools") }
                item {
                    BeepCard(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) {
                            server.tools.forEachIndexed { i, tool ->
                                if (i > 0) HorizontalDivider(color = t.softLine)
                                Row(Modifier.fillMaxWidth().heightIn(min = 44.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Text(mcpToolLabel(tool), fontSize = 14.sp, color = t.ink, modifier = Modifier.weight(1f))
                                    Text("Asks", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = t.ink,
                                        modifier = Modifier.clip(CircleShape).background(t.tints[2]).padding(horizontal = 8.dp, vertical = 3.dp))
                                }
                            }
                        }
                    }
                }
                item {
                    Text("Beep asks before every tool on a server you add. Tap Always when it asks to stop asking for this server.",
                        fontSize = 12.sp, color = t.muted, lineHeight = 17.sp, modifier = Modifier.padding(horizontal = 4.dp))
                }
            }
            if (!server.connected) item {
                Text(if (server.signIn) "The server stopped accepting Beep's sign-in. Sign in again to use its tools."
                    else "Beep could not reach this server. It tries again when you ask.",
                    fontSize = 13.sp, color = t.muted, lineHeight = 18.sp)
            }
            if (!server.connected) item {
                if (busy) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = t.ok)
                else ChunkyButton(onClick = onSignIn, color = t.accent, contentColor = t.onAccent) {
                    Text(if (server.signIn) "Sign in again" else "Try again", fontWeight = FontWeight.SemiBold)
                }
            }
            item {
                Box(Modifier.fillMaxWidth().padding(top = 12.dp), contentAlignment = Alignment.Center) {
                    TextButton(onClick = onRemove) { Text("Remove server", color = t.error, fontSize = 14.sp) }
                }
            }
        }
    }
}

/** A connected app's state; tapping the row opens its permissions. */
@Composable
private fun ConnectedChip() {
    val t = Beep.colors
    Row(Modifier.clip(CircleShape).background(t.tints[1]).padding(start = 10.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Icon(Icons.Outlined.Check, null, Modifier.size(14.dp), tint = t.ok)
        Text("Connected", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(16.dp), tint = t.muted)
    }
}

private val levels = listOf(
    Triple(PermissionLevel.ASK, "Ask first", "Beep shows what it is about to do and waits for you."),
    Triple(PermissionLevel.ALLOW, "Always allow", "Beep sends, changes and deletes without asking."),
    Triple(PermissionLevel.READ_ONLY, "Read only", "Beep can search and read, never change anything."),
)

/** How much Beep may do in one connected app: a level, and under Ask first, actions that never ask. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AppPermissionsScreen(repository: ChatRepository, app: AppIntegration, onBack: () -> Unit, onDisconnect: () -> Unit,
                                 accounts: AccountsSection? = null) {
    val t = Beep.colors
    val scope = rememberCoroutineScope()
    var permissions by remember { mutableStateOf<AppPermissions?>(null) }
    var error by remember { mutableStateOf("") }
    LaunchedEffect(app.slug) {
        try { permissions = withContext(Dispatchers.IO) { repository.appPermissions(app.slug) } }
        catch (e: Exception) { error = userMessage(e) }
    }
    fun save(next: AppPermissions) {
        val before = permissions
        permissions = next
        scope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.setAppPermissions(app.slug, next.level, next.actions.filter { it.allowed }.map { it.tool })
                }
                error = ""
            } catch (e: Exception) { permissions = before; error = userMessage(e) }
        }
    }

    Scaffold(containerColor = t.page, topBar = {
        TopAppBar(title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                AppLogo(app.slug, app.name, app.logoUrl, size = 32.dp)
                Spacer(Modifier.width(12.dp))
                Text(app.name, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
            }
        }, navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.page))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(horizontal = 18.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (error.isNotEmpty()) item { Text(error, color = t.error, fontSize = 13.sp) }
            if (accounts != null) item { AccountsCard(app, accounts) }
            val current = permissions
            if (current == null) {
                if (error.isEmpty()) item {
                    Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = t.ok)
                    }
                }
            } else {
                item { SectionLabel("When Beep acts in ${app.name}") }
                items(levels, key = { it.first.name }) { (level, title, detail) ->
                    val selected = current.level == level
                    BeepCard(Modifier.fillMaxWidth().selectable(selected, role = Role.RadioButton) {
                        if (!selected) save(current.copy(level = level))
                    }) {
                        Row(Modifier.padding(BeepSpace.row), verticalAlignment = Alignment.CenterVertically) {
                            RadioButton(selected, onClick = null, colors = RadioButtonDefaults.colors(selectedColor = t.ok))
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
                                Text(detail, fontSize = 12.sp, color = t.muted, lineHeight = 17.sp)
                            }
                        }
                    }
                }
                if (current.actions.isNotEmpty()) {
                    item { SectionLabel("Actions") }
                    item {
                        BeepCard(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) {
                                current.actions.forEachIndexed { i, action ->
                                    if (i > 0) HorizontalDivider(color = t.softLine)
                                    ActionRow(action, current.level) { allowed ->
                                        save(current.copy(actions = current.actions.map { if (it.tool == action.tool) it.copy(allowed = allowed) else it }))
                                    }
                                }
                            }
                        }
                    }
                }
                item { Text("Searching and reading is always allowed.", fontSize = 12.sp, color = t.muted, modifier = Modifier.padding(horizontal = 4.dp)) }
            }
            item {
                Box(Modifier.fillMaxWidth().padding(top = 12.dp), contentAlignment = Alignment.Center) {
                    val all = if ((accounts?.accounts?.size ?: 0) > 1) " (all accounts)" else ""
                    TextButton(onClick = onDisconnect) { Text("Disconnect ${app.name}$all", color = t.error, fontSize = 14.sp) }
                }
            }
        }
    }
}

/** An app's connected accounts and what can be done with them, for its settings screen. */
private class AccountsSection(
    val accounts: List<AppAccount>,
    val busy: String?,
    val onAdd: () -> Unit,
    val onRemove: (AppAccount) -> Unit,
)

/** Each connected account with Remove, and a button that signs in another. The first one is Beep's default. */
@Composable
private fun AccountsCard(app: AppIntegration, section: AccountsSection) {
    val t = Beep.colors
    var removing by remember { mutableStateOf<AppAccount?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel("Accounts")
        BeepCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) {
                section.accounts.forEachIndexed { i, account ->
                    if (i > 0) HorizontalDivider(color = t.softLine)
                    Row(Modifier.fillMaxWidth().heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(account.address ?: "${app.name} account", fontSize = 14.sp, color = t.ink,
                                maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (i == 0 && section.accounts.size > 1) Text("Default", fontSize = 11.sp, color = t.muted)
                        }
                        if (section.busy == "account:" + account.id) {
                            CircularProgressIndicator(Modifier.padding(horizontal = 12.dp).size(18.dp), strokeWidth = 2.dp, color = t.ok)
                        } else {
                            TextButton(onClick = { removing = account }) { Text("Remove", color = t.error, fontSize = 13.sp) }
                        }
                    }
                }
            }
        }
        if (section.busy == app.slug) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = t.ok)
                Text("Waiting for the browser sign-in…", fontSize = 13.sp, color = t.muted)
            }
        } else {
            ChunkyButton(onClick = section.onAdd, color = t.surface, contentColor = t.ink,
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp)) {
                Text("+ Add another account", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Text("Beep uses the first account unless you name another, like \"check my work inbox\". " +
            "To add a different account, sign out of ${app.name} in the browser first.",
            fontSize = 12.sp, color = t.muted, lineHeight = 17.sp, modifier = Modifier.padding(horizontal = 4.dp))
    }
    removing?.let { account ->
        val last = section.accounts.size == 1
        AlertDialog(onDismissRequest = { removing = null }, containerColor = t.surface,
            title = { Text("Remove ${account.address ?: "this account"}?") },
            text = { Text(if (last) "This disconnects ${app.name}. Beep can no longer read or send its mail."
                else "Beep stops using this account. Your other ${app.name} accounts stay connected.",
                fontSize = 14.sp, color = t.muted) },
            confirmButton = { TextButton(onClick = { removing = null; section.onRemove(account) }) {
                Text("Remove", color = t.error, fontWeight = FontWeight.SemiBold)
            } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } })
    }
}

@Composable
private fun ActionRow(action: AppAction, level: PermissionLevel, onAllowed: (Boolean) -> Unit) {
    val t = Beep.colors
    val (tag, tint) = when {
        level == PermissionLevel.READ_ONLY -> "Off" to t.raised
        level == PermissionLevel.ALLOW || action.allowed -> "Allowed" to t.tints[1]
        else -> "Asks" to t.tints[2]
    }
    Row(Modifier.fillMaxWidth().heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(action.label, fontSize = 14.sp, color = t.ink, modifier = Modifier.weight(1f))
        Text(tag, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = t.ink,
            modifier = Modifier.clip(CircleShape).background(tint).padding(horizontal = 8.dp, vertical = 3.dp))
        if (level == PermissionLevel.ASK) {
            Spacer(Modifier.width(10.dp))
            Switch(checked = action.allowed, onCheckedChange = onAllowed, modifier = Modifier.semantics { contentDescription = "Always allow ${action.label}" },
                colors = SwitchDefaults.colors(checkedTrackColor = t.ok))
        }
    }
}

/** Bundled brand icons for the curated apps. */
internal val bundledLogos = mapOf(
    "gmail" to R.drawable.logo_gmail, "googlecalendar" to R.drawable.logo_googlecalendar,
    "notion" to R.drawable.logo_notion, "googledrive" to R.drawable.logo_googledrive,
    "outlook" to R.drawable.logo_outlook, "microsoft_teams" to R.drawable.logo_microsoft_teams,
    "one_drive" to R.drawable.logo_one_drive, "excel" to R.drawable.logo_excel,
    "share_point" to R.drawable.logo_share_point,
    "slack" to R.drawable.logo_slack, "github" to R.drawable.logo_github,
    "linear" to R.drawable.logo_linear, "todoist" to R.drawable.logo_todoist,
    "fastmail" to R.drawable.logo_fastmail,
)

/**
 * The app's bundled icon, else the server's logo when it is a bitmap, else its initial on a tint.
 * Logos sit on white so dark marks like GitHub's stay visible in dark themes.
 */
@Composable
fun AppLogo(slug: String, name: String, logoUrl: String? = null, size: Dp = 42.dp) {
    val t = Beep.colors
    val bundled = bundledLogos[slug]
    val remote by produceState<Bitmap?>(null, logoUrl, bundled) {
        value = if (bundled == null) logoUrl?.let { Logos.load(it) } else null
    }
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    val hasLogo = bundled != null || remote != null
    Box(Modifier.size(size).clip(shape).background(if (hasLogo) Color.White else t.tints[name.hashCode().mod(t.tints.size)]),
        contentAlignment = Alignment.Center) {
        val image = remote
        when {
            bundled != null -> Image(painterResource(bundled), null, Modifier.size(size * 0.6f))
            image != null -> Image(image.asImageBitmap(), null, Modifier.size(size * 0.7f), contentScale = ContentScale.Fit)
            else -> Text(name.take(1).uppercase(), fontSize = (size.value * 0.43f).sp, fontWeight = FontWeight.Bold, color = t.ink)
        }
    }
}

/**
 * The bundled logo of the app a tool belongs to: Composio tools are `GMAIL_SEND_EMAIL`, MCP tools `fastmail_x`.
 * Some slugs have an underscore themselves, like `MICROSOFT_TEAMS_SEND_MESSAGE`.
 */
fun appLogoForTool(name: String): Int? {
    val lower = name.lowercase()
    return bundledLogos.entries.firstOrNull { lower.startsWith(it.key + "_") }?.value
}

@Composable
private fun AppLogo(app: AppIntegration) = AppLogo(app.slug, app.name, app.logoUrl)

private object Logos {
    private val cache = LruCache<String, Bitmap>(32)

    suspend fun load(url: String): Bitmap? = cache.get(url) ?: withContext(Dispatchers.IO) {
        runCatching {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 8000
            connection.readTimeout = 8000
            connection.setRequestProperty("User-Agent", "Beep for Android")
            try { connection.inputStream.use(BitmapFactory::decodeStream) } finally { connection.disconnect() }
        }.getOrNull()?.also { cache.put(url, it) }
    }
}
