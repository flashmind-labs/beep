package com.flashmind.beep

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Logout
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.flashmind.beep.proto.Group
import com.flashmind.beep.proto.SessionMatch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.time.Instant
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale

/** When a row in the chat list last changed: "09:41" today, then "Yesterday", "Thu" this week, "3 Oct". */
internal fun listTime(millis: Long, now: ZonedDateTime = ZonedDateTime.now()): String {
    val at = Instant.ofEpochMilli(millis).atZone(now.zone)
    val days = ChronoUnit.DAYS.between(at.toLocalDate(), now.toLocalDate())
    return when {
        days <= 0L -> at.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
        days == 1L -> "Yesterday"
        days < 7L -> at.format(DateTimeFormatter.ofPattern("EEE", Locale.getDefault()))
        at.year == now.year -> at.format(DateTimeFormatter.ofPattern("d MMM"))
        else -> at.format(DateTimeFormatter.ofPattern("d MMM yyyy"))
    }
}

/** The last thing said in a chat, as its row shows it. Empty while its turns are not on the phone. */
internal fun chatPreview(chat: Conversation): String {
    val last = chat.messages.lastOrNull { it.role == "user" || it.role == "assistant" } ?: return ""
    val text = last.text.lineSequence().map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("```") }.joinToString(" ")
    return if (last.role == "user") "You: $text" else text
}

/** A row in the chat list: a chat with Beep or a group. */
private sealed class HomeRow {
    abstract val key: String
    abstract val pinned: Boolean
    abstract val time: Long
    data class Chat(val chat: Conversation) : HomeRow() {
        override val key get() = chat.id
        override val pinned get() = chat.pinned
        override val time get() = chat.lastActive
    }
    data class Room(val group: Group) : HomeRow() {
        override val key get() = "group-" + group.id
        override val pinned get() = group.pinned
        override val time get() = group.updatedAt
    }
}

/** What the chat list can open. */
internal class HomeActions(
    val openChat: (String) -> Unit,
    val openGroup: (String) -> Unit,
    val newChat: () -> Unit,
    val newGroup: () -> Unit,
    val joinCode: () -> Unit,
    val invite: (Group) -> Unit,
    val open: (Screen) -> Unit,
)

/**
 * The app's home: Beep's chats and the groups like a messenger, pinned first, then newest.
 * Long press selects rows to pin, delete or leave; the menu holds everything else.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ChatListScreen(repository: ChatRepository, places: PlaceStore, actions: HomeActions) {
    val state by repository.state.collectAsStateWithLifecycle()
    val groups by repository.groups.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var menu by remember { mutableStateOf(false) }
    var newSheet by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<Set<String>>(emptySet()) }
    var confirmRemove by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<Conversation?>(null) }
    var sharing by remember { mutableStateOf<Conversation?>(null) }
    LaunchedEffect(Unit) { repository.refreshHistory(); repository.refreshGroups() }
    BackHandler(enabled = picked.isNotEmpty()) { picked = emptySet() }
    BackHandler(enabled = searching) { searching = false; query = "" }

    val rows = remember(state.conversations, groups) {
        val all = state.conversations.filter { !it.isUnused }.map { HomeRow.Chat(it) } + groups.map { HomeRow.Room(it) }
        all.sortedWith(compareByDescending<HomeRow> { it.pinned }.thenByDescending { it.time })
    }
    val byKey = rows.associateBy { it.key }
    fun toggle(key: String) { picked = if (key in picked) picked - key else picked + key }

    Scaffold(containerColor = Beep.colors.page, topBar = {
        when {
            picked.isNotEmpty() -> SelectionBar(picked.mapNotNull { byKey[it] }, onClear = { picked = emptySet() },
                onPin = { chosen ->
                    val pin = !chosen.all { it.pinned }
                    chosen.forEach { row -> when (row) {
                        is HomeRow.Chat -> repository.pinSession(row.chat.id, pin)
                        is HomeRow.Room -> repository.pinGroup(row.group.id, pin)
                    } }
                    picked = emptySet()
                },
                onRemove = { confirmRemove = true },
                onRename = { renaming = it; picked = emptySet() },
                onShare = { sharing = it; picked = emptySet() },
                onInvite = { actions.invite(it); picked = emptySet() })
            searching -> SearchBar(query, { query = it }, onClose = { searching = false; query = "" })
            else -> TopAppBar(title = {
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(displayText("Beep"), style = displayStyle(28.sp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        ConnectionDot(connected, status)
                        if (!connected) Text("Tap to reconnect", Modifier.padding(start = 6.dp).clickable { repository.reconnect() },
                            fontSize = 12.sp, color = Beep.colors.muted, fontWeight = FontWeight.Medium)
                    }
                }
            }, actions = {
                IconButton(onClick = { searching = true }) { Icon(Icons.Outlined.Search, "Search") }
                Box {
                    IconButton(onClick = { menu = true }) { Icon(Icons.Outlined.MoreVert, "More") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, containerColor = Beep.colors.surface) {
                        MenuItem("Places", Icons.Outlined.Place) { menu = false; actions.open(Screen.Places) }
                        MenuItem("Memories", Icons.Outlined.Psychology) { menu = false; actions.open(Screen.Memories) }
                        MenuItem("Routines", Icons.Outlined.Schedule) { menu = false; actions.open(Screen.Crons) }
                        MenuItem("Apps", Icons.Outlined.Extension) { menu = false; actions.open(Screen.Integrations) }
                        MenuItem("Settings", Icons.Outlined.Settings) { menu = false; actions.open(Screen.Settings) }
                    }
                }
            }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
        }
    }, floatingActionButton = {
        if (picked.isEmpty() && !searching) ChunkyButton(onClick = { newSheet = true }, color = Beep.colors.accent,
            shape = RoundedCornerShape(18.dp), modifier = Modifier.size(58.dp), contentPadding = PaddingValues(0.dp)) {
            Icon(Icons.Outlined.Add, "New", Modifier.size(26.dp))
        }
    }) { padding ->
        if (searching) SearchResults(repository, places, rows, query.trim(), Modifier.padding(padding), actions)
        else LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
            items(rows, key = { it.key }) { row ->
                val selecting = picked.isNotEmpty()
                HomeRowView(row, held = row.key in picked, query = "",
                    onClick = {
                        if (selecting) toggle(row.key)
                        else when (row) { is HomeRow.Chat -> actions.openChat(row.chat.id); is HomeRow.Room -> actions.openGroup(row.group.id) }
                    },
                    onLongClick = { toggle(row.key) })
            }
            if (rows.isEmpty()) item {
                Text("Your chats with Beep and your groups show up here.\nStart one with the + button.",
                    Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 40.dp), textAlign = TextAlign.Center,
                    color = Beep.colors.muted, lineHeight = 22.sp)
            }
        }
    }

    if (newSheet) NewSheet(onDismiss = { newSheet = false }, onChat = { newSheet = false; actions.newChat() },
        onGroup = { newSheet = false; actions.newGroup() }, onCode = { newSheet = false; actions.joinCode() })
    sharing?.let { chat -> ShareSheet(repository, chat, onDismiss = { sharing = null }) }
    renaming?.let { chat ->
        var text by remember(chat.id) { mutableStateOf(chat.title) }
        AlertDialog(onDismissRequest = { renaming = null },
            confirmButton = { TextButton(onClick = {
                val title = text.trim()
                if (title.isNotEmpty()) { repository.renameSession(chat.id, title); renaming = null }
            }) { Text("Rename") } },
            dismissButton = { TextButton(onClick = { renaming = null }) { Text("Cancel") } },
            title = { Text("Rename conversation") },
            text = { OutlinedTextField(text, { text = it }, singleLine = true, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) },
            containerColor = Beep.colors.surface)
    }
    if (confirmRemove) {
        val chosen = picked.mapNotNull { byKey[it] }
        val chatIds = chosen.filterIsInstance<HomeRow.Chat>().map { it.chat.id }.toSet()
        val leaving = chosen.filterIsInstance<HomeRow.Room>().map { it.group }
        val chats = when (chatIds.size) { 0 -> null; 1 -> "this conversation"; else -> "${chatIds.size} conversations" }
        val left = when (leaving.size) { 0 -> null; 1 -> leaving[0].name; else -> "${leaving.size} groups" }
        AlertDialog(onDismissRequest = { confirmRemove = false },
            confirmButton = { TextButton(onClick = {
                confirmRemove = false; picked = emptySet()
                repository.deleteSessions(chatIds)
                repository.leaveGroups(leaving.map { it.id }.toSet())
            }) { Text(if (chats == null) "Leave" else "Delete", color = Beep.colors.error) } },
            dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text("Cancel") } },
            title = { Text(listOfNotNull(chats?.let { "Delete $it" }, left?.let { "leave $it" })
                .joinToString(" and ").replaceFirstChar { it.uppercase() } + "?") },
            text = { Text(listOfNotNull(
                chats?.let { "The conversations and their messages are deleted for good." },
                when {
                    leaving.size == 1 && leaving[0].members.size <= 1 -> "You are the last member, so the group and its messages are deleted."
                    leaving.isNotEmpty() -> "You stop getting the group's messages. Someone in it can invite you again."
                    else -> null
                }).joinToString(" ")) },
            containerColor = Beep.colors.surface)
    }
}

@Composable
private fun MenuItem(label: String, icon: ImageVector, onClick: () -> Unit) =
    DropdownMenuItem(text = { Text(label) }, leadingIcon = { Icon(icon, null, Modifier.size(18.dp)) }, onClick = onClick)

/** Shown while rows are held: how many, then pin and delete or leave. One chat also renames or shares. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SelectionBar(chosen: List<HomeRow>, onClear: () -> Unit, onPin: (List<HomeRow>) -> Unit, onRemove: () -> Unit,
                         onRename: (Conversation) -> Unit, onShare: (Conversation) -> Unit, onInvite: (Group) -> Unit) {
    var more by remember { mutableStateOf(false) }
    val one = chosen.singleOrNull()
    TopAppBar(title = { Text("${chosen.size}", style = displayStyle(22.sp)) },
        navigationIcon = { IconButton(onClick = onClear) { Icon(Icons.Outlined.Close, "Clear selection") } },
        actions = {
            IconButton(onClick = { onPin(chosen) }) {
                Icon(Icons.Outlined.PushPin, if (chosen.all { it.pinned }) "Unpin" else "Pin")
            }
            val onlyGroups = chosen.all { it is HomeRow.Room }
            IconButton(onClick = onRemove) {
                if (onlyGroups) Icon(Icons.AutoMirrored.Outlined.Logout, "Leave group", tint = Beep.colors.error)
                else Icon(Icons.Outlined.Delete, "Delete", tint = Beep.colors.error)
            }
            if (one != null) Box {
                IconButton(onClick = { more = true }) { Icon(Icons.Outlined.MoreVert, "More") }
                DropdownMenu(expanded = more, onDismissRequest = { more = false }, containerColor = Beep.colors.surface) {
                    when (one) {
                        is HomeRow.Chat -> {
                            MenuItem("Rename", Icons.Outlined.Edit) { more = false; onRename(one.chat) }
                            MenuItem("Share", Icons.Outlined.Share) { more = false; onShare(one.chat) }
                        }
                        is HomeRow.Room -> MenuItem("Invite", Icons.Outlined.Link) { more = false; onInvite(one.group) }
                    }
                }
            }
        }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.user))
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SearchBar(query: String, onChange: (String) -> Unit, onClose: () -> Unit) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    TopAppBar(title = {
        OutlinedTextField(query, onChange, placeholder = { Text("Search chats and places", fontSize = 14.sp) }, singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            shape = RoundedCornerShape(24.dp), modifier = Modifier.fillMaxWidth().padding(end = 12.dp).focusRequester(focus),
            colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Beep.colors.surface, focusedContainerColor = Beep.colors.surface,
                unfocusedBorderColor = Beep.colors.softLine, focusedBorderColor = Beep.colors.brand))
    }, navigationIcon = { IconButton(onClick = onClose) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Close search") } },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
}

/** Chats whose name or last message matches, what was written in other chats, then places. */
@Composable
private fun SearchResults(repository: ChatRepository, places: PlaceStore, rows: List<HomeRow>, query: String, modifier: Modifier, actions: HomeActions) {
    var hits by remember { mutableStateOf<List<SessionMatch>>(emptyList()) }
    var saved by remember { mutableStateOf<List<Place>>(emptyList()) }
    LaunchedEffect(Unit) { saved = runCatching { withContext(Dispatchers.IO) { places.list() } }.getOrDefault(emptyList()) }
    // Names match at once; what was written comes from the server after a pause in typing.
    LaunchedEffect(query) {
        hits = emptyList()
        if (query.length < 2) return@LaunchedEffect
        delay(350)
        hits = runCatching { withContext(Dispatchers.IO) { repository.searchChats(query) } }.getOrDefault(emptyList())
    }
    val matches = if (query.isEmpty()) emptyList() else rows.filter { row ->
        when (row) {
            is HomeRow.Chat -> row.chat.title.contains(query, true) || chatPreview(row.chat).contains(query, true)
            is HomeRow.Room -> row.group.name.contains(query, true) || groupSummary(row.group).contains(query, true)
        }
    }
    val shown = matches.mapNotNull { (it as? HomeRow.Chat)?.chat?.id }.toSet()
    val known = rows.filterIsInstance<HomeRow.Chat>().associateBy { it.chat.id }
    val written = hits.filter { it.session.id !in shown && it.session.id in known }
    val placeHits = if (query.isEmpty()) emptyList() else saved.filter { it.name.contains(query, true) || it.address.contains(query, true) }
    LazyColumn(modifier.fillMaxSize()) {
        if (matches.isNotEmpty()) item(key = "chats") { SectionLabel("Chats") }
        items(matches, key = { it.key }) { row ->
            HomeRowView(row, held = false, query = query, onLongClick = {}, onClick = {
                when (row) { is HomeRow.Chat -> actions.openChat(row.chat.id); is HomeRow.Room -> actions.openGroup(row.group.id) }
            })
        }
        if (written.isNotEmpty()) item(key = "written") { SectionLabel("In messages") }
        items(written, key = { "hit-" + it.session.id }) { hit ->
            val chat = known.getValue(hit.session.id).chat
            ListRow(avatar = { BeepFace() }, name = AnnotatedString(chat.title), time = listTime(chat.lastActive),
                preview = highlighted((if (hit.fromUser) "You: " else "") + hit.snippet, query), group = false, pinned = false,
                held = false, onClick = { actions.openChat(chat.id) }, onLongClick = {})
        }
        if (placeHits.isNotEmpty()) item(key = "places") { SectionLabel("Places") }
        items(placeHits, key = { "place-" + it.id }) { place ->
            ListRow(avatar = { Box(Modifier.size(48.dp).background(Beep.colors.tints[0], CircleShape), contentAlignment = Alignment.Center) {
                    Icon(Icons.Outlined.Place, null, Modifier.size(22.dp), tint = Beep.colors.ink) } },
                name = highlighted(place.name, query), time = "${place.radiusMeters} m", preview = AnnotatedString(place.address),
                group = false, pinned = false, held = false, onClick = { actions.open(Screen.Places) }, onLongClick = {})
        }
        if (query.isNotEmpty() && matches.isEmpty() && written.isEmpty() && placeHits.isEmpty()) item {
            Text("Nothing matches \"$query\".", Modifier.fillMaxWidth().padding(40.dp), textAlign = TextAlign.Center, color = Beep.colors.muted)
        }
    }
}

@Composable
private fun SectionLabel(text: String) =
    Text(text, Modifier.padding(start = 20.dp, end = 20.dp, top = 14.dp, bottom = 4.dp),
        fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)

@Composable
private fun highlighted(text: String, query: String): AnnotatedString = buildAnnotatedString {
    val mark = SpanStyle(background = Beep.colors.brand.copy(alpha = 0.25f), color = Beep.colors.ink)
    var from = 0
    while (query.isNotEmpty()) {
        val at = text.indexOf(query, from, ignoreCase = true)
        if (at < 0) break
        append(text.substring(from, at))
        withStyle(mark) { append(text.substring(at, at + query.length)) }
        from = at + query.length
    }
    append(text.substring(from))
}

@Composable
private fun BeepFace() = GroupAvatar("b", Beep.colors.brand, 48.dp, beep = true)

@Composable
private fun HomeRowView(row: HomeRow, held: Boolean, query: String, onClick: () -> Unit, onLongClick: () -> Unit) {
    when (row) {
        is HomeRow.Chat -> ListRow(avatar = { BeepFace() }, name = highlighted(row.chat.title, query),
            time = row.time.takeIf { it > 0 }?.let { listTime(it) } ?: "", preview = highlighted(chatPreview(row.chat), query),
            group = false, pinned = row.pinned, held = held, onClick = onClick, onLongClick = onLongClick)
        is HomeRow.Room -> ListRow(avatar = {
                val initials = row.group.name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.take(1).uppercase() }
                GroupAvatar(initials.ifEmpty { "?" }, Beep.colors.tints[row.group.id.hashCode().mod(Beep.colors.tints.size)], 48.dp)
            }, name = highlighted(row.group.name, query), time = row.time.takeIf { it > 0 }?.let { listTime(it) } ?: "",
            preview = highlighted(groupSummary(row.group), query), group = true, pinned = row.pinned, held = held,
            onClick = onClick, onLongClick = onLongClick)
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ListRow(avatar: @Composable () -> Unit, name: AnnotatedString, time: String, preview: AnnotatedString, group: Boolean,
                    pinned: Boolean, held: Boolean, onClick: () -> Unit, onLongClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().background(if (held) Beep.colors.user else Color.Transparent)
        .combinedClickable(onClick = onClick, onLongClick = onLongClick).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        if (held) Box(Modifier.size(48.dp).background(Beep.colors.ok, CircleShape), contentAlignment = Alignment.Center) {
            Icon(Icons.Outlined.Check, "Selected", tint = Color.White)
        } else avatar()
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (group) Icon(Icons.Outlined.Groups, "Group", Modifier.size(15.dp).padding(end = 2.dp), tint = Beep.colors.muted)
                Text(name, Modifier.weight(1f), fontWeight = FontWeight.SemiBold, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    color = if (held) Beep.colors.onUser else Beep.colors.ink)
                Text(time, fontSize = 12.sp, color = Beep.colors.muted, modifier = Modifier.padding(start = 8.dp))
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(preview, Modifier.weight(1f), fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    color = if (held) Beep.colors.onUser.copy(alpha = 0.75f) else Beep.colors.muted)
                if (pinned) Icon(Icons.Filled.PushPin, "Pinned", Modifier.padding(start = 8.dp).size(15.dp), tint = Beep.colors.muted)
            }
        }
    }
}

/** The + button's choices: a chat with Beep, a new group, or joining one. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NewSheet(onDismiss: () -> Unit, onChat: () -> Unit, onGroup: () -> Unit, onCode: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.page) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            NewOption(Beep.colors.accent, Beep.colors.surface, null, "New chat with Beep", "A fresh conversation, same memories", onChat)
            NewOption(Beep.colors.surface, Beep.colors.tints[2], Icons.Outlined.Groups, "New group", "Chat with people, and Beep helps when it can", onGroup)
            NewOption(Beep.colors.surface, Beep.colors.tints[1], Icons.Outlined.QrCode, "Join with a code", "Enter or scan the code someone sent you", onCode)
        }
    }
}

@Composable
private fun NewOption(color: Color, logo: Color, icon: ImageVector?, title: String, subtitle: String, onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, color = color, shape = RoundedCornerShape(Beep.shapes.radius), modifier = Modifier.fillMaxWidth(),
        contentPadding = PaddingValues(14.dp)) {
        Box(Modifier.size(40.dp).background(logo, RoundedCornerShape(12.dp)), contentAlignment = Alignment.Center) {
            if (icon != null) Icon(icon, null, Modifier.size(20.dp), tint = Beep.colors.ink)
            else Text("b", fontWeight = FontWeight.Bold, fontSize = 18.sp, color = Beep.colors.ink)
        }
        Column(Modifier.weight(1f).padding(start = 6.dp)) {
            Text(title, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            Text(subtitle, fontSize = 12.5.sp, color = LocalContentColor.current.copy(alpha = 0.75f))
        }
    }
}
