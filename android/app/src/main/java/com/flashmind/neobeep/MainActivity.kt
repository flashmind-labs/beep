package com.flashmind.neobeep

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.activity.compose.rememberLauncherForActivityResult
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private fun java.io.InputStream.readProfileBytes(): ByteArray {
    val buffer = ByteArray(65537)
    var count = 0
    while (count < buffer.size) {
        val read = read(buffer, count, buffer.size - count)
        if (read < 0) break
        count += read
    }
    return buffer.copyOf(count)
}

private val Ink = Color(0xFF0D1410)
private val Surface = Color(0xFF151D18)
private val Elevated = Color(0xFF1C2620)
private val Border = Color(0xFF2A3830)
private val Moss = Color(0xFF3B9B7A)
private val Lime = Color(0xFFC8F06A)
private val TextPrimary = Color(0xFFE2E8E0)
private val TextMuted = Color(0xFF7A8A80)
private val TextDim = Color(0xFF4A5A50)

open class MainActivity : ComponentActivity() {
    protected open val bubble = false
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (it) Bubbles.show(this) else Bubbles.settings(this)
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val repository = (application as BeepApplication).repository
        setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Moss, onPrimary = Ink,
                background = Ink, surface = Surface, onSurface = TextPrimary, secondaryContainer = Elevated,
                surfaceVariant = Elevated, onSurfaceVariant = TextMuted,
                secondary = Moss, onSecondaryContainer = TextPrimary, outline = Border,
                surfaceContainer = Surface, surfaceContainerHigh = Elevated,
                inverseSurface = TextPrimary, inverseOnSurface = Ink)) {
                BeepApp(repository, bubble, onFloat = {
                    if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    } else if (!Bubbles.show(this)) Bubbles.settings(this)
                }, onExpand = { startActivity(Intent(this, MainActivity::class.java)) }, onBubbleSettings = { Bubbles.settings(this) })
            }
        }
    }
}
class BubbleActivity : MainActivity() { override val bubble = true }

private enum class Screen { Chat, Memories, Crons }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BeepApp(repository: ChatRepository, bubble: Boolean, onFloat: () -> Unit, onExpand: () -> Unit, onBubbleSettings: () -> Unit) {
    val state by repository.state.collectAsStateWithLifecycle()
    val config by repository.settings.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    var settingsOpen by rememberSaveable { mutableStateOf(false) }
    var drawerOpen by remember { mutableStateOf(false) }
    var bubbleMenu by remember { mutableStateOf(false) }
    var historyQuery by rememberSaveable { mutableStateOf("") }
    var screen by rememberSaveable { mutableStateOf(Screen.Chat) }
    if (settingsOpen || !config.configured) {
        ConnectionScreen(config, canGoBack = config.configured, onBack = { settingsOpen = false }, onConnect = {
            withContext(Dispatchers.IO) { repository.connect(it) }; settingsOpen = false
        })
        return
    }
    if (screen == Screen.Memories && !bubble) { MemoriesScreen(repository, onBack = { screen = Screen.Chat }); return }
    if (screen == Screen.Crons && !bubble) { CronsScreen(repository, onBack = { screen = Screen.Chat }); return }
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    LaunchedEffect(drawerOpen) { if (drawerOpen) { repository.refreshHistory(); drawerState.open() } else drawerState.close() }
    LaunchedEffect(drawerState.currentValue) { drawerOpen = drawerState.currentValue == DrawerValue.Open }
    ModalNavigationDrawer(drawerState = drawerState, drawerContent = {
        ModalDrawerSheet(modifier = Modifier.widthIn(max = 320.dp).fillMaxWidth(0.9f), drawerContainerColor = Surface) {
            Spacer(Modifier.height(if (bubble) 8.dp else 20.dp))
            Text("Your conversations", Modifier.padding(horizontal = 20.dp, vertical = 12.dp), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            FilledTonalButton(onClick = { repository.newChat(); drawerOpen = false }, modifier = Modifier.padding(horizontal = 24.dp).fillMaxWidth()) {
                Icon(Icons.Outlined.Add, null); Spacer(Modifier.width(8.dp)); Text("New conversation")
            }
            OutlinedTextField(historyQuery, { historyQuery = it }, placeholder = { Text("Find a conversation", fontSize = 13.sp) },
                leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                shape = RoundedCornerShape(16.dp), modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp).fillMaxWidth())
            LazyColumn(Modifier.weight(1f)) {
                items(state.conversations.filter { it.title.contains(historyQuery, ignoreCase = true) }, key = { it.id }) { chat ->
                    NavigationDrawerItem(label = { Text(chat.title, maxLines = 2, fontSize = 14.sp, overflow = TextOverflow.Ellipsis) },
                        icon = { Icon(Icons.Outlined.ChatBubbleOutline, null, Modifier.size(18.dp)) },
                        colors = NavigationDrawerItemDefaults.colors(selectedContainerColor = Elevated, unselectedContainerColor = Surface),
                        selected = state.current.id == chat.id,
                        onClick = { repository.select(chat.id); drawerOpen = false },
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 2.dp))
                }
            }
            if (!bubble) {
                HorizontalDivider(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = Border)
                NavigationDrawerItem(label = { Text("Memories", fontSize = 14.sp) },
                    icon = { Icon(Icons.Outlined.Psychology, null, Modifier.size(18.dp)) },
                    colors = NavigationDrawerItemDefaults.colors(unselectedContainerColor = Surface),
                    selected = false, onClick = { screen = Screen.Memories; drawerOpen = false },
                    modifier = Modifier.padding(horizontal = 12.dp))
                NavigationDrawerItem(label = { Text("Automations", fontSize = 14.sp) },
                    icon = { Icon(Icons.Outlined.Schedule, null, Modifier.size(18.dp)) },
                    colors = NavigationDrawerItemDefaults.colors(unselectedContainerColor = Surface),
                    selected = false, onClick = { screen = Screen.Crons; drawerOpen = false },
                    modifier = Modifier.padding(horizontal = 12.dp))
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = onBubbleSettings, modifier = Modifier.padding(horizontal = 16.dp)) {
                    Icon(Icons.Outlined.BubbleChart, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Floating chat settings")
                }
            }
            Text(status, fontSize = 11.sp, color = Moss, modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp))
            TextButton(onClick = { settingsOpen = true; drawerOpen = false }, modifier = Modifier.padding(16.dp)) {
                Icon(Icons.Outlined.Settings, null); Spacer(Modifier.width(8.dp)); Text("Connection settings")
            }
        }
    }) {
        Scaffold(containerColor = Ink, topBar = {
            if (bubble) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Beep", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.width(8.dp))
                    Box(Modifier.size(5.dp).background(if (connected) Moss else Color(0xFFE85A4A), CircleShape))
                    Spacer(Modifier.weight(1f))
                    IconButton(onClick = onExpand) { Icon(Icons.Outlined.OpenInFull, "Open full app", Modifier.size(18.dp)) }
                    Box {
                        IconButton(onClick = { bubbleMenu = true }) { Icon(Icons.Outlined.MoreVert, "Chat options", Modifier.size(20.dp)) }
                        DropdownMenu(expanded = bubbleMenu, onDismissRequest = { bubbleMenu = false }) {
                            DropdownMenuItem(text = { Text("New conversation") }, onClick = { repository.newChat(); bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Conversations") }, onClick = { drawerOpen = true; bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Connection settings") }, onClick = { settingsOpen = true; bubbleMenu = false })
                        }
                    }
                }
            } else TopAppBar(title = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(38.dp))
                    Spacer(Modifier.width(10.dp))
                    Column {
                        Text("Beep", fontWeight = FontWeight.Bold, fontSize = 21.sp)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(6.dp).background(if (connected) Moss else Color(0xFFE85A4A), CircleShape))
                            Spacer(Modifier.width(5.dp))
                            Text(if (connected) "Here to help" else "Tap to reconnect", fontSize = 11.sp, color = Moss,
                                maxLines = 1, overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.clickable { repository.reconnect() })
                        }
                    }
                }
            }, navigationIcon = { IconButton(onClick = { drawerOpen = true }) { Icon(Icons.Outlined.Menu, "Conversations") } },
                actions = {
                    if (bubble) IconButton(onClick = onExpand) { Icon(Icons.Outlined.OpenInFull, "Open full app") }
                    else IconButton(onClick = onFloat) { Icon(Icons.Outlined.BubbleChart, "Float chat") }
                    IconButton(onClick = { settingsOpen = true }) { Icon(Icons.Outlined.Settings, "Connection settings") }
                }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
        }) { padding ->
            ChatScreen(state.current, repository, Modifier.padding(padding).fillMaxSize(), compact = bubble)
        }
    }

}

@Composable
private fun ChatScreen(chat: Conversation, repository: ChatRepository, modifier: Modifier, compact: Boolean) {
    var draft by rememberSaveable(chat.id) { mutableStateOf("") }
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    var followLatest by remember(chat.id) { mutableStateOf(true) }
    LaunchedEffect(list, chat.id) {
        snapshotFlow { list.isScrollInProgress to list.canScrollForward }.collect { (scrolling, canForward) ->
            if (scrolling) followLatest = !canForward
        }
    }
    LaunchedEffect(chat.id) { if (chat.messages.isNotEmpty()) list.scrollToItem(chat.messages.lastIndex) }
    LaunchedEffect(chat.messages.size, chat.messages.lastOrNull()?.text?.length, chat.status) {
        if (chat.messages.isNotEmpty() && followLatest) list.scrollToItem(chat.messages.size - 1)
    }
    fun submit() { if (draft.isNotBlank() && !chat.busy) { followLatest = true; repository.send(draft); draft = "" } }
    Column(modifier.imePadding()) {
        if (chat.messages.isEmpty() && compact) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.Center) {
                Text("Hey. What can I help with?", fontSize = 18.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(6.dp))
                Text("Ask a question or search the web.", fontSize = 13.sp, color = TextMuted)
            }
        } else if (chat.messages.isEmpty()) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 28.dp, vertical = 24.dp), verticalArrangement = Arrangement.Center) {
                Surface(shape = RoundedCornerShape(18.dp), color = Elevated) {
                    Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
                }
                Spacer(Modifier.height(24.dp))
                Text("A little help.\nRight here.", fontSize = 36.sp, lineHeight = 42.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp)
                Spacer(Modifier.height(12.dp))
                Text("Find an answer. Untangle a thought.\nKeep something worth remembering.", color = TextMuted, lineHeight = 23.sp)
                Spacer(Modifier.height(30.dp))
                Suggestion("Remember something", "Remember that I prefer short, practical answers.", Icons.Outlined.BookmarkBorder) { draft = it }
                Spacer(Modifier.height(10.dp))
                Suggestion("Search the web", "Search the web for ", Icons.Outlined.Search) { draft = it }
            }
        } else {
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(if (compact) 12.dp else 20.dp), verticalArrangement = Arrangement.spacedBy(if (compact) 12.dp else 22.dp)) {
                items(chat.messages, key = { it.id }) { message ->
                    Column(Modifier.fillMaxWidth(), horizontalAlignment = if (message.role == "user") Alignment.End else Alignment.Start) {
                        if (message.role == "assistant" && !compact) Text("BEEP", fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 2.sp, color = Moss, modifier = Modifier.padding(bottom = 7.dp))
                        Surface(color = if (message.role == "user") Elevated else Color.Transparent,
                            shape = RoundedCornerShape(20.dp, 20.dp, if (message.role == "user") 5.dp else 20.dp, 20.dp),
                            modifier = if (message.role == "user") Modifier.widthIn(max = 320.dp) else Modifier.fillMaxWidth()) {
                            SelectionContainer {
                                Text(remember(message.text) { formattedMessage(message.text) }, modifier = Modifier.padding(if (message.role == "user") (if (compact) 10.dp else 16.dp) else 0.dp), fontSize = if (compact) 14.sp else 16.sp, lineHeight = if (compact) 21.sp else 25.sp)
                            }
                        }
                        if (message.role == "assistant" && message.text.isNotBlank() && !chat.busy && !compact) CopyReply(message.text, compact)
                    }
                }
            }
        }
        if (!followLatest && chat.messages.isNotEmpty()) {
            TextButton(onClick = { scope.launch { list.animateScrollToItem(chat.messages.lastIndex); followLatest = true } }, modifier = Modifier.align(Alignment.CenterHorizontally)) {
                Icon(Icons.Outlined.ArrowDownward, null, Modifier.size(16.dp)); Spacer(Modifier.width(6.dp)); Text("Latest reply", fontSize = 12.sp)
            }
        }
        if (chat.status.isNotEmpty() || chat.tool.isNotEmpty()) Row(Modifier.padding(horizontal = 22.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 1.5.dp)
            Spacer(Modifier.width(9.dp)); Text(chat.tool.ifEmpty { chat.status }, color = Moss, fontSize = 12.sp, maxLines = 2)
        }
        if (chat.error.isNotEmpty()) Row(Modifier.padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(chat.error, fontSize = 12.sp, color = MaterialTheme.colorScheme.error, modifier = Modifier.weight(1f), maxLines = 4)
            if (chat.busy) TextButton(onClick = repository::retry) { Text("Retry") }
        }
        Row(Modifier.padding(horizontal = if (compact) 8.dp else 16.dp, vertical = if (compact) 4.dp else 10.dp).fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            OutlinedTextField(value = draft, onValueChange = { if (it.toByteArray().size <= 32768) draft = it },
                modifier = Modifier.weight(1f), placeholder = { Text("Ask anything…", fontSize = 14.sp) },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss),
                shape = RoundedCornerShape(if (compact) 20.dp else 26.dp), maxLines = if (compact) 3 else 5,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send), keyboardActions = KeyboardActions(onSend = { submit() }),
                trailingIcon = {
                    if (chat.busy) IconButton(onClick = repository::cancel) { Icon(Icons.Outlined.StopCircle, "Stop reply", tint = Moss) }
                    else FilledIconButton(onClick = { submit() }, enabled = draft.isNotBlank(), modifier = Modifier.padding(4.dp).size(48.dp)) {
                        Icon(Icons.AutoMirrored.Outlined.Send, "Send message", modifier = Modifier.size(19.dp))
                    }
                })
        }

    }
}

@Composable
private fun CopyReply(text: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember(text) { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    TextButton(onClick = {
        context.getSystemService(android.content.ClipboardManager::class.java)
            .setPrimaryClip(android.content.ClipData.newPlainText("Beep reply", text))
        copied = true
    }, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
        Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, if (compact) (if (copied) "Copied" else "Copy reply") else null,
            Modifier.size(14.dp), tint = Color(0xFF71816F))
        if (!compact) { Spacer(Modifier.width(6.dp)); Text(if (copied) "Copied" else "Copy reply", fontSize = 11.sp, color = TextMuted) }
    }
}

// A deliberately small, safe Markdown subset: links, emphasis, inline code and headings.
private fun formattedMessage(text: String) = buildAnnotatedString {
    val tokens = Regex("""\[([^\]]+)\]\((https?://[^\s)]+)\)|(https?://[^\s<>]+)|\*\*(.+?)\*\*|`([^`\n]+)`""")
    text.split('\n').forEachIndexed { index, line ->
        if (index > 0) append("\n")
        val heading = Regex("^#{1,4} +").find(line)
        val content = line.removePrefix(heading?.value.orEmpty()).replace(Regex("^[-*] +"), "• ")
        pushStyle(if (heading != null) SpanStyle(fontWeight = FontWeight.SemiBold, fontSize = 18.sp) else SpanStyle())
        var offset = 0
        for (match in tokens.findAll(content)) {
            append(content.substring(offset, match.range.first))
            when {
                match.groups[4] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(match.groups[4]!!.value) }
                match.groups[5] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = Elevated)) { append(match.groups[5]!!.value) }
                else -> {
                    val url = (match.groups[2]?.value ?: match.groups[3]!!.value).trimEnd('.', ',', ';')
                    withLink(LinkAnnotation.Url(url, TextLinkStyles(style = SpanStyle(color = Moss, textDecoration = TextDecoration.Underline)))) {
                        append(match.groups[1]?.value ?: url)
                    }
                    if (match.groups[3] != null) append(match.value.removePrefix(url))
                }
            }
            offset = match.range.last + 1
        }
        append(content.substring(offset)); pop()
    }
}

@Composable
private fun Suggestion(title: String, prompt: String, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: (String) -> Unit) {
    Surface(onClick = { onClick(prompt) }, shape = RoundedCornerShape(16.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = Moss, modifier = Modifier.size(20.dp)); Spacer(Modifier.width(12.dp))
            Text(title, fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
            Icon(Icons.Outlined.NorthEast, null, tint = Moss, modifier = Modifier.size(16.dp))
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemoriesScreen(repository: ChatRepository, onBack: () -> Unit) {
    var memories by remember { mutableStateOf<List<Pair<String, String>>>(emptyList()) }
    var query by rememberSaveable { mutableStateOf("") }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { memories = withContext(Dispatchers.IO) { repository.listMemories() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    val filtered = memories.filter { (k, v) -> query.isBlank() || k.contains(query, true) || v.contains(query, true) }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text("Memories", fontSize = 18.sp) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(query, { query = it }, placeholder = { Text("Search memories", fontSize = 13.sp) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.weight(1f),
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                Spacer(Modifier.width(12.dp))
                Text("${filtered.size}", fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (filtered.isEmpty()) Text(if (memories.isEmpty()) "No memories stored yet." else "No matches.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(filtered, key = { it.first }) { (key, content) ->
                    Surface(shape = RoundedCornerShape(10.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(key, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(content, fontSize = 12.sp, color = TextMuted, maxLines = 3, overflow = TextOverflow.Ellipsis, lineHeight = 18.sp)
                            }
                            IconButton(onClick = { scope.launch {
                                try { withContext(Dispatchers.IO) { repository.deleteMemory(key) }; refresh() }
                                catch (e: Exception) { error = e.message.orEmpty() }
                            } }, modifier = Modifier.size(36.dp)) {
                                Icon(Icons.Outlined.Close, "Delete", Modifier.size(16.dp), tint = TextDim)
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CronsScreen(repository: ChatRepository, onBack: () -> Unit) {
    var crons by remember { mutableStateOf<List<ChatRepository.Cron>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    var showCreate by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { crons = withContext(Dispatchers.IO) { repository.listCrons() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    if (showCreate) {
        CreateCronDialog(onDismiss = { showCreate = false }, onCreate = { label, prompt, schedule ->
            scope.launch {
                try { withContext(Dispatchers.IO) { repository.createCron(label, prompt, schedule) }; showCreate = false; refresh() }
                catch (e: Exception) { error = e.message.orEmpty() }
            }
        })
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text("Automations", fontSize = 18.sp) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { IconButton(onClick = { showCreate = true }) { Icon(Icons.Outlined.Add, "New automation") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (crons.isEmpty()) Text("No automations yet.\nSchedule tasks for Beep to run on a timer.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(crons, key = { it.id }) { cron ->
                    Surface(shape = RoundedCornerShape(10.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(cron.label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(cron.prompt, fontSize = 12.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 18.sp)
                                Spacer(Modifier.height(4.dp))
                                Text(cron.schedule, fontSize = 11.sp, color = TextDim, fontFamily = FontFamily.Monospace)
                            }
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Switch(checked = cron.enabled, onCheckedChange = { enabled ->
                                    scope.launch {
                                        try { withContext(Dispatchers.IO) { repository.toggleCron(cron.id, enabled) }; refresh() }
                                        catch (e: Exception) { error = e.message.orEmpty() }
                                    }
                                }, modifier = Modifier.height(28.dp), colors = SwitchDefaults.colors(checkedTrackColor = Moss, uncheckedTrackColor = Border))
                                IconButton(onClick = { scope.launch {
                                    try { withContext(Dispatchers.IO) { repository.deleteCron(cron.id) }; refresh() }
                                    catch (e: Exception) { error = e.message.orEmpty() }
                                } }, modifier = Modifier.size(32.dp)) {
                                    Icon(Icons.Outlined.Delete, "Delete", Modifier.size(16.dp), tint = TextDim)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun CreateCronDialog(onDismiss: () -> Unit, onCreate: (String, String, String) -> Unit) {
    var label by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var schedule by remember { mutableStateOf("") }
    AlertDialog(onDismissRequest = onDismiss, containerColor = Surface,
        title = { Text("New automation", fontSize = 18.sp, fontWeight = FontWeight.Medium) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(label, { label = it }, label = { Text("Name") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Morning briefing") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(schedule, { schedule = it }, label = { Text("Schedule (cron)") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("0 9 * * *") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(prompt, { prompt = it }, label = { Text("Prompt") }, maxLines = 4, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Give me a summary of...") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
        } },
        confirmButton = { Button(onClick = { if (label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) onCreate(label, prompt, schedule) },
            enabled = label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) { Text("Create") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ConnectionScreen(initial: ConnectionSettings, canGoBack: Boolean, onBack: () -> Unit, onConnect: suspend (ConnectionSettings) -> Unit) {
    var address by remember { mutableStateOf(initial.address) }
    var hostname by remember { mutableStateOf(initial.hostname) }
    var token by remember { mutableStateOf(initial.token) }
    var ca by remember { mutableStateOf(initial.ca) }
    var reveal by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    val profilePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                val config = withContext(Dispatchers.IO) {
                    val raw = context.contentResolver.openInputStream(uri)?.use { it.readProfileBytes() } ?: error("Cannot open profile")
                    require(raw.size <= 65536) { "Profile is too large" }; ConnectionSettings.fromJson(raw.toString(Charsets.UTF_8))
                }
                address = config.address; hostname = config.hostname; token = config.token; ca = config.ca; error = ""
            } catch (e: Exception) { error = e.message.orEmpty() }
        }
    }
    val certPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                ca = withContext(Dispatchers.IO) {
                    val raw = context.contentResolver.openInputStream(uri)?.use { it.readProfileBytes() } ?: error("Cannot open certificate")
                    require(raw.size <= 65536) { "Certificate is too large" }; raw.toString(Charsets.UTF_8)
                }
                require(ca.contains("-----BEGIN CERTIFICATE-----")) { "Choose a PEM certificate" }; error = ""
            } catch (e: Exception) { ca = ""; error = e.message.orEmpty() }
        }
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(if (canGoBack) "Connection" else "Welcome to Beep", fontSize = 18.sp) },
            navigationIcon = { if (canGoBack) IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize().imePadding(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(66.dp))
                Spacer(Modifier.height(24.dp))
                Text("Your assistant.\nYour space.", fontSize = 36.sp, lineHeight = 41.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp)
                Spacer(Modifier.height(12.dp))
                Text("Connect to your Beep server to bring your conversations and memory along.", color = TextMuted, lineHeight = 23.sp)
            }
            item { OutlinedButton(onClick = { profilePicker.launch(arrayOf("application/json", "text/*", "application/octet-stream")) }, enabled = !saving, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Outlined.FileOpen, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Import connection profile")
            } }
            item { OutlinedTextField(address, { address = it }, label = { Text("Server address") }, placeholder = { Text("[Rayfish IPv6]:4433") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving) }
            item { OutlinedTextField(hostname, { hostname = it }, label = { Text("TLS hostname") }, placeholder = { Text("beep.example.com") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving) }
            item { OutlinedTextField(token, { token = it }, label = { Text("App token (optional in testing)") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving,
                visualTransformation = if (reveal) VisualTransformation.None else PasswordVisualTransformation(),
                trailingIcon = { IconButton(onClick = { reveal = !reveal }) { Icon(if (reveal) Icons.Outlined.VisibilityOff else Icons.Outlined.Visibility, "Toggle token visibility") } }) }
            item {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = { certPicker.launch(arrayOf("*/*")) }, enabled = !saving) {
                        Icon(Icons.Outlined.VerifiedUser, null, Modifier.size(17.dp)); Spacer(Modifier.width(6.dp)); Text(if (ca.isBlank()) "Add private CA certificate" else "Private CA certificate added", fontSize = 12.sp)
                    }
                    if (ca.isNotBlank()) IconButton(onClick = { ca = "" }, enabled = !saving) { Icon(Icons.Outlined.Close, "Remove certificate", Modifier.size(17.dp)) }
                }
                Text("For a Rayfish address, connect the VPN on this device first. Testing servers do not require an app token.", fontSize = 12.sp, color = TextMuted, lineHeight = 18.sp)
                if (canGoBack) Text("Changing the server or account clears this device’s chat cache. Server history is kept.", modifier = Modifier.padding(top = 8.dp), fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) item { Text(error, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
            item { Button(onClick = {
                saving = true; error = ""
                scope.launch {
                    try { onConnect(ConnectionSettings(address.trim(), hostname.trim(), token.trim(), ca)) }
                    catch (e: Exception) { error = e.message ?: "Could not connect" }
                    finally { saving = false }
                }
            }, enabled = !saving && address.isNotBlank() && hostname.isNotBlank(), modifier = Modifier.fillMaxWidth().height(54.dp), shape = RoundedCornerShape(17.dp)) {
                if (saving) { CircularProgressIndicator(Modifier.size(18.dp), color = Color.White, strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                Text(if (saving) "Connecting…" else "Connect to Beep", fontWeight = FontWeight.SemiBold)
            } }
        }
    }
}
