package com.flashmind.beep

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.proto.Group
import com.flashmind.beep.proto.GroupAuthor
import com.flashmind.beep.proto.GroupChat
import com.flashmind.beep.proto.GroupMessage
import com.flashmind.beep.proto.GroupPreview
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.UUID

/** The name text of members, by the order they joined; their avatars use the theme's tints. */
private val nameColors = listOf(Color(0xFFB8574B), Color(0xFF2F7D5B), Color(0xFF8A6A12))

/** The name the user last went by in a group, offered again for the next one. */
private fun lastMemberName(context: Context): String =
    context.getSharedPreferences("groups", Context.MODE_PRIVATE).getString("member_name", "") ?: ""

private fun keepMemberName(context: Context, name: String) =
    context.getSharedPreferences("groups", Context.MODE_PRIVATE).edit().putString("member_name", name.trim()).apply()

/** "Sam, Ali and Beep", with the caller as "You". */
internal fun groupPeople(group: Group): String {
    val names = group.members.sortedByDescending { it.you }.map { if (it.you) "You" else it.name } + "Beep"
    return names.dropLast(1).joinToString(", ") + " and " + names.last()
}

/** The last message as the chat list shows it: "Sam: text", or that Beep is typing. */
internal fun groupSummary(group: Group): String = when {
    group.beepTyping -> "Beep is typing…"
    else -> group.last?.let { "${authorName(it.author)}: ${it.content}" } ?: groupPeople(group)
}

private fun authorName(author: GroupAuthor) = when (author) {
    is GroupAuthor.Member -> if (author.you) "You" else author.name
    is GroupAuthor.Beep -> "Beep"
}

/** Bolds each @beep, so a mention stands out in the bubble. */
private fun withMentions(text: String) = Regex("(?i)(?<![\\w*])@beep\\b").replace(text) { "**${it.value}**" }

@Composable
internal fun GroupAvatar(initial: String, color: Color, size: Dp = 28.dp, beep: Boolean = false) {
    Box(Modifier.size(size).background(color, CircleShape).border(1.5.dp, Beep.colors.page, CircleShape), contentAlignment = Alignment.Center) {
        Text(initial, fontSize = (size.value * 0.4f).sp, fontWeight = FontWeight.Bold,
            color = if (beep) Color.White else Beep.colors.ink)
    }
}

@Composable
private fun memberTint(index: Int) = Beep.colors.tints[index.mod(Beep.colors.tints.size)]

private fun memberIndex(group: Group, name: String): Int =
    group.members.indexOfFirst { it.name.equals(name, ignoreCase = true) }.takeIf { it >= 0 } ?: name.hashCode()

/** A group row in the drawer's chat list. */
@Composable
internal fun GroupRow(group: Group, highlighted: Boolean, onClick: () -> Unit) {
    Row(Modifier.padding(horizontal = 12.dp, vertical = 2.dp).fillMaxWidth().heightIn(min = 56.dp)
        .clip(RoundedCornerShape(Beep.shapes.smallRadius))
        .background(if (highlighted) Beep.colors.user else Color.Transparent)
        .clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(group.name, maxLines = 1, fontSize = 14.sp, overflow = TextOverflow.Ellipsis,
                color = if (highlighted) Beep.colors.onUser else Color.Unspecified)
            Text(groupSummary(group), fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                color = if (highlighted) Beep.colors.onUser.copy(alpha = 0.7f) else Beep.colors.muted)
        }
        Icon(Icons.Outlined.Groups, "Group", Modifier.size(16.dp), tint = Beep.colors.brand)
    }
}

/** A message being sent, shown until the server has it. */
private data class Outgoing(val requestId: String, val text: String)

/** A group chat: members' messages, Beep's replies and a composer that can @mention. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GroupChatPane(repository: ChatRepository, groupId: String, onMenu: () -> Unit, onInfo: () -> Unit,
                           onInvite: (Group) -> Unit, onGone: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var chat by remember(groupId) { mutableStateOf<GroupChat?>(null) }
    var outgoing by remember(groupId) { mutableStateOf(listOf<Outgoing>()) }
    val listed by repository.groups.collectAsState()
    suspend fun load() {
        runCatching { withContext(Dispatchers.IO) { repository.group(groupId) } }
            .onSuccess { chat = it }
            .onFailure { if (it is BeepException.NotFound) onGone() }
    }
    DisposableEffect(groupId) {
        repository.groupShown(groupId)
        onDispose { repository.groupShown(null) }
    }
    LaunchedEffect(groupId) {
        load()
        repository.groupChanges.collect { if (it == groupId) load() }
    }
    // A notice says when Beep is done; this covers a notice that never comes.
    val typing = chat?.group?.beepTyping == true
    LaunchedEffect(typing) {
        while (typing) { delay(5000); load() }
    }
    val group = chat?.group ?: listed.firstOrNull { it.id == groupId }
    val messages = chat?.messages.orEmpty()
    val sent = messages.size
    val listState = rememberLazyListState()
    LaunchedEffect(sent, outgoing.size, typing) {
        val count = messages.size + outgoing.size + if (typing) 1 else 0
        if (count > 0) listState.animateScrollToItem(count - 1)
    }
    var field by remember(groupId) { mutableStateOf(TextFieldValue(repository.draft("group:$groupId").text)) }
    LaunchedEffect(field.text) { repository.setDraft("group:$groupId", repository.draft("group:$groupId").copy(text = field.text)) }
    fun send() {
        val text = field.text.trim()
        if (text.isEmpty()) return
        val out = Outgoing(UUID.randomUUID().toString(), text)
        outgoing = outgoing + out
        field = TextFieldValue("")
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.sendToGroup(groupId, out.requestId, text) } }
                .onSuccess { load() }
                .onFailure {
                    Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show()
                    if (field.text.isEmpty()) field = TextFieldValue(text, TextRange(text.length))
                }
            outgoing = outgoing - out
        }
    }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = {
            Column(Modifier.clickable(enabled = group != null, onClick = onInfo)) {
                Text(displayText(group?.name ?: "Group"), style = displayStyle(22.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                group?.let {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Row(horizontalArrangement = Arrangement.spacedBy((-6).dp)) {
                            it.members.sortedByDescending { m -> m.you }.take(4).forEach { m ->
                                GroupAvatar(m.name.take(1).uppercase(), memberTint(memberIndex(it, m.name)), 18.dp)
                            }
                            GroupAvatar("b", Beep.colors.brand, 18.dp, beep = true)
                        }
                        Spacer(Modifier.width(6.dp))
                        Text(groupPeople(it), fontSize = 12.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }, navigationIcon = {
            ChunkyButton(onClick = onMenu, shape = RoundedCornerShape(Beep.shapes.smallRadius),
                modifier = Modifier.padding(start = 12.dp, end = 10.dp).size(42.dp), contentPadding = PaddingValues(0.dp)) {
                Icon(Icons.Outlined.Menu, "Conversations", Modifier.size(20.dp))
            }
        }, actions = {
            IconButton(onClick = { group?.let(onInvite) }, enabled = group != null, modifier = Modifier.padding(end = 6.dp)) {
                Icon(Icons.Outlined.Link, "Invite", tint = Beep.colors.muted)
            }
        }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().imePadding()) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                val loaded = chat
                when {
                    loaded == null -> CircularProgressIndicator(Modifier.align(Alignment.Center).size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                    messages.isEmpty() && outgoing.isEmpty() -> Text("No messages yet.\nSay hi, or write @beep to ask Beep something.",
                        Modifier.align(Alignment.Center).padding(32.dp), textAlign = TextAlign.Center, color = Beep.colors.muted, lineHeight = 22.sp)
                    else -> {
                        val days = remember(messages) { groupDays(messages) }
                        LazyColumn(Modifier.fillMaxSize().padding(horizontal = 14.dp), state = listState,
                            verticalArrangement = Arrangement.spacedBy(10.dp), contentPadding = PaddingValues(vertical = 12.dp)) {
                            items(messages, key = { it.seq }) { message ->
                                Column {
                                    days[message.seq]?.let {
                                        Text(it, Modifier.fillMaxWidth().padding(bottom = 10.dp), fontSize = 11.sp,
                                            color = Beep.colors.dim, textAlign = TextAlign.Center)
                                    }
                                    GroupBubble(loaded.group, message.author, message.content)
                                }
                            }
                            items(outgoing, key = { it.requestId }) { out ->
                                Box(Modifier.alpha(0.6f)) { GroupBubble(loaded.group, GroupAuthor.Member("", you = true), out.text) }
                            }
                            if (typing) item(key = "typing") {
                                Row(Modifier.padding(start = 36.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Text("Beep is typing", fontSize = 12.sp, color = Beep.colors.muted)
                                    Spacer(Modifier.width(6.dp))
                                    CircularProgressIndicator(Modifier.size(10.dp), strokeWidth = 1.5.dp, color = Beep.colors.muted)
                                }
                            }
                        }
                    }
                }
            }
            group?.let { MentionPicker(it, field) { field = it } }
            GroupComposer(field, onChange = { if (it.text.toByteArray().size <= 8192) field = it }, onSend = ::send)
        }
    }
}

/** The first message of each day, with that day's label. */
private fun groupDays(messages: List<GroupMessage>): Map<Long, String> {
    val zone = ZoneId.systemDefault()
    val labels = mutableMapOf<Long, String>()
    var current: LocalDate? = null
    for (m in messages) {
        val day = Instant.ofEpochMilli(m.timestamp).atZone(zone).toLocalDate()
        if (day != current) labels[m.seq] = dayLabel(day)
        current = day
    }
    return labels
}

@Composable
private fun GroupBubble(group: Group, author: GroupAuthor, content: String) {
    val radius = Beep.shapes.radius
    when {
        author is GroupAuthor.Member && author.you -> Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
            Box(Modifier.padding(start = 48.dp).beepFrame(RoundedCornerShape(radius, radius, 6.dp, radius), Beep.colors.user)) {
                MessageContent(withMentions(content), "user", compact = false)
            }
        }
        else -> {
            val beep = author is GroupAuthor.Beep
            val name = if (author is GroupAuthor.Member) author.name else "Beep"
            val index = memberIndex(group, name)
            Row(Modifier.fillMaxWidth().padding(end = 24.dp), verticalAlignment = Alignment.Top) {
                GroupAvatar(if (beep) "b" else name.take(1).uppercase(), if (beep) Beep.colors.brand else memberTint(index), beep = beep)
                Spacer(Modifier.width(8.dp))
                Column(Modifier.beepFrame(RoundedCornerShape(6.dp, radius, radius, radius), if (beep) Beep.colors.tints[1] else Beep.colors.surface)) {
                    Text(name, Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp), fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                        color = if (beep) Beep.colors.brand else nameColors[index.mod(nameColors.size)])
                    MessageContent(withMentions(content), "assistant", compact = false, padded = true)
                }
            }
        }
    }
}

/** The word being typed when it starts with @, so the picker can complete it. */
private fun mentionQuery(field: TextFieldValue): IntRange? {
    val end = field.selection.end
    if (!field.selection.collapsed || end == 0) return null
    val start = field.text.lastIndexOf('@', end - 1).takeIf { it >= 0 } ?: return null
    if (start > 0 && !field.text[start - 1].isWhitespace()) return null
    if (field.text.substring(start + 1, end).any { it.isWhitespace() }) return null
    return start until end
}

/** Lists Beep and the other members while an @word is being typed, and completes it on tap. */
@Composable
private fun MentionPicker(group: Group, field: TextFieldValue, onPick: (TextFieldValue) -> Unit) {
    val range = mentionQuery(field) ?: return
    val typed = field.text.substring(range.first + 1, range.last + 1)
    data class Choice(val name: String, val note: String, val beep: Boolean)
    val choices = (listOf(Choice("beep", "Ask Beep to reply here", true)) +
        group.members.filter { !it.you }.map { Choice(it.name, "Member", false) })
        .filter { it.name.startsWith(typed, ignoreCase = true) }
    if (choices.isEmpty()) return
    Column(Modifier.padding(horizontal = 14.dp).fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface)
        .padding(vertical = 6.dp)) {
        choices.forEach { choice ->
            Row(Modifier.fillMaxWidth().clickable {
                val text = field.text.replaceRange(range.first, range.last + 1, "@${choice.name} ")
                onPick(TextFieldValue(text, TextRange(range.first + choice.name.length + 2)))
            }.padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                GroupAvatar(if (choice.beep) "b" else choice.name.take(1).uppercase(),
                    if (choice.beep) Beep.colors.brand else memberTint(memberIndex(group, choice.name)), 30.dp, beep = choice.beep)
                Spacer(Modifier.width(12.dp))
                Column {
                    Text(if (choice.beep) "Beep" else choice.name, fontWeight = if (choice.beep) FontWeight.SemiBold else FontWeight.Medium)
                    Text(choice.note, fontSize = 12.sp, color = Beep.colors.muted)
                }
            }
        }
    }
}

@Composable
private fun GroupComposer(field: TextFieldValue, onChange: (TextFieldValue) -> Unit, onSend: () -> Unit) {
    val size = 50.dp
    val shape = RoundedCornerShape(Beep.shapes.radius)
    Row(Modifier.padding(start = 14.dp, end = 16.dp, top = 8.dp, bottom = 12.dp).fillMaxWidth(),
        verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.weight(1f).heightIn(min = size).beepFrame(shape, Beep.colors.surface, pressableLook()),
            verticalAlignment = Alignment.CenterVertically) {
            TextField(field, onChange, Modifier.weight(1f),
                placeholder = { Text("Message, or @beep to ask", fontSize = 15.sp, color = Beep.colors.dim) },
                textStyle = LocalTextStyle.current.copy(fontSize = 15.sp),
                colors = TextFieldDefaults.colors(unfocusedContainerColor = Color.Transparent, focusedContainerColor = Color.Transparent,
                    unfocusedIndicatorColor = Color.Transparent, focusedIndicatorColor = Color.Transparent, cursorColor = Beep.colors.ink),
                maxLines = 6, keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences))
        }
        ChunkyButton(onClick = onSend, enabled = field.text.isNotBlank(), modifier = Modifier.size(size), shape = shape,
            color = Beep.colors.accent, contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(0.dp)) {
            Icon(Icons.AutoMirrored.Outlined.Send, "Send message", Modifier.size(20.dp))
        }
    }
}

@Composable
private fun SheetHeader(icon: ImageVector, tint: Color, title: String, subtitle: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(52.dp).background(tint, RoundedCornerShape(14.dp)), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(24.dp), tint = Beep.colors.ink)
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(displayText(title), style = displayStyle(24.sp), color = Beep.colors.ink)
            Text(subtitle, fontSize = 13.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun NameField(label: String, value: String, onChange: (String) -> Unit, error: String? = null, hint: String? = null) {
    OutlinedTextField(value, onChange, Modifier.fillMaxWidth(), label = { Text(label) }, singleLine = true, isError = error != null,
        supportingText = (error ?: hint)?.let { { Text(it) } }, shape = RoundedCornerShape(12.dp),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words),
        colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Beep.colors.page, focusedContainerColor = Beep.colors.page,
            unfocusedBorderColor = Beep.colors.softLine, focusedBorderColor = Beep.colors.brand))
}

/** Makes a group with the user in it, then hands it over so its invite link can be sent. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun NewGroupSheet(repository: ChatRepository, onDismiss: () -> Unit, onCreated: (Group) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var memberName by remember { mutableStateOf(lastMemberName(context)) }
    var busy by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SheetHeader(Icons.Outlined.Groups, Beep.colors.tints[2], "New group", "Chat with people, and @beep when you want it")
            NameField("Group name", name, { if (it.length <= 60) name = it })
            NameField("Your name in the group", memberName, { if (it.length <= 30) memberName = it })
            Text("You get a link to send to people. Anyone with Beep who opens it can join.",
                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onDismiss) { Text("Cancel") }
                Spacer(Modifier.width(8.dp))
                ChunkyButton(onClick = {
                    busy = true
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.createGroup(name.trim(), memberName.trim()) } }
                            .onSuccess { keepMemberName(context, memberName); onCreated(it) }
                            .onFailure { busy = false; Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                    }
                }, enabled = !busy && name.isNotBlank() && memberName.isNotBlank(), color = Beep.colors.accent,
                    contentColor = Beep.colors.onAccent, shape = RoundedCornerShape(12.dp)) {
                    Text("Create", fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

/** The group's invite link, to copy, send or replace. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GroupInviteSheet(repository: ChatRepository, groupId: String, groupName: String, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var url by remember(groupId) { mutableStateOf<String?>(null) }
    var reset by remember(groupId) { mutableStateOf(false) }
    LaunchedEffect(groupId) {
        runCatching { withContext(Dispatchers.IO) { repository.groupInvite(groupId) } }
            .onSuccess { url = it.url }
            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show(); onDismiss() }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            SheetHeader(Icons.Outlined.Link, Beep.colors.tints[1], "Invite people", groupName)
            Box(Modifier.fillMaxWidth().height(46.dp).background(Beep.colors.page, RoundedCornerShape(12.dp)).padding(horizontal = 14.dp),
                contentAlignment = Alignment.CenterStart) {
                val link = url
                if (link == null) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                else Text(link, fontFamily = FontFamily.Monospace, fontSize = 12.sp, color = Beep.colors.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ChunkyButton(onClick = {
                    (context as? MainActivity)?.suppressNextClipboard = true
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Beep group", url))
                    Toast.makeText(context, "Link copied", Toast.LENGTH_SHORT).show()
                }, enabled = url != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(12.dp)) {
                    Icon(Icons.Outlined.ContentCopy, null, Modifier.size(18.dp)); Text("Copy", fontWeight = FontWeight.SemiBold)
                }
                ChunkyButton(onClick = {
                    val text = "Join $groupName on Beep\n$url"
                    context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null))
                }, enabled = url != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(12.dp),
                    color = Beep.colors.accent, contentColor = Beep.colors.onAccent) {
                    Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)); Text("Send", fontWeight = FontWeight.SemiBold)
                }
            }
            Text((if (reset) "New link made. The old one no longer opens. " else "") +
                "Anyone with Beep who opens the link joins and sees the whole chat. Beep uses what it knows about each of you " +
                "to help, but never shows one person's memories to the others.",
                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = {
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.groupInvite(groupId, reset = true) } }
                            .onSuccess { url = it.url; reset = true }
                            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                    }
                }, enabled = url != null) { Text("Reset link", color = Beep.colors.error) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onDismiss) { Text("Done") }
            }
        }
    }
}

@Composable
private fun PrivacyLine(icon: ImageVector, text: String) {
    Row(verticalAlignment = Alignment.Top) {
        Icon(icon, null, Modifier.size(18.dp), tint = Beep.colors.brand)
        Spacer(Modifier.width(10.dp))
        Text(text, fontSize = 13.sp, lineHeight = 19.sp, color = Beep.colors.ink)
    }
}

/** What a `beep://group/` link opens: the group's name and members, and a name to join as. */
@Composable
internal fun GroupJoinScreen(repository: ChatRepository, token: String, onJoined: (String) -> Unit, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var preview by remember(token) { mutableStateOf<GroupPreview?>(null) }
    var expired by remember(token) { mutableStateOf(false) }
    var name by remember(token) { mutableStateOf(lastMemberName(context)) }
    var taken by remember(token) { mutableStateOf<String?>(null) }
    var busy by remember(token) { mutableStateOf(false) }
    LaunchedEffect(token) {
        runCatching { withContext(Dispatchers.IO) { repository.peekGroup(token) } }
            .onSuccess { if (it.joined) onJoined(it.id) else preview = it }
            .onFailure { if (it is BeepException.NotFound) expired = true else { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show(); onBack() } }
    }
    Column(Modifier.fillMaxSize().background(Beep.colors.page).systemBarsPadding().imePadding().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically)) {
        val group = preview
        when {
            expired -> {
                Box(Modifier.align(Alignment.CenterHorizontally).size(64.dp).background(Beep.colors.tints[0], RoundedCornerShape(18.dp)),
                    contentAlignment = Alignment.Center) { Icon(Icons.Outlined.LinkOff, null, Modifier.size(28.dp), tint = Beep.colors.ink) }
                Text(displayText("This link no longer works"), style = displayStyle(30.sp), textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
                Text("Whoever sent it reset the group's link. Ask them for a new one.", color = Beep.colors.muted,
                    textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
                ChunkyButton(onClick = onBack, modifier = Modifier.fillMaxWidth().height(52.dp), color = Beep.colors.accent,
                    contentColor = Beep.colors.onAccent, shape = RoundedCornerShape(16.dp)) { Text("Back to Beep", fontWeight = FontWeight.SemiBold) }
            }
            group == null -> CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally).size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
            else -> {
                Row(Modifier.align(Alignment.CenterHorizontally), horizontalArrangement = Arrangement.spacedBy((-14).dp)) {
                    group.members.take(4).forEachIndexed { i, m -> GroupAvatar(m.take(1).uppercase(), memberTint(i), 56.dp) }
                    GroupAvatar("b", Beep.colors.brand, 56.dp, beep = true)
                }
                Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(displayText("Join ${group.name}?"), style = displayStyle(32.sp), textAlign = TextAlign.Center)
                    val people = group.members + "Beep"
                    Text(people.dropLast(1).joinToString(", ") + " and Beep are in it.", color = Beep.colors.muted, textAlign = TextAlign.Center)
                }
                Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    PrivacyLine(Icons.Outlined.Psychology, "Beep can use your memories to help the group. It never shows or quotes them to others.")
                    PrivacyLine(Icons.Outlined.Shield, "When you @beep, it runs with your apps and your account, never someone else's.")
                    PrivacyLine(Icons.Outlined.Notifications, "You get a notification for each new message.")
                }
                NameField("Your name in the group", name, { if (it.length <= 30) { name = it; taken = null } },
                    error = taken?.let { "Someone in the group already goes by $it" }, hint = "The others see this name next to your messages")
                ChunkyButton(onClick = {
                    busy = true
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.joinGroup(token, name.trim()) } }
                            .onSuccess { keepMemberName(context, name); onJoined(it.id) }
                            .onFailure {
                                busy = false
                                when (it) {
                                    is BeepException.Conflict -> taken = name.trim()
                                    is BeepException.NotFound -> expired = true
                                    else -> Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show()
                                }
                            }
                    }
                }, enabled = !busy && taken == null && name.isNotBlank(), modifier = Modifier.fillMaxWidth().height(52.dp),
                    color = Beep.colors.accent, contentColor = Beep.colors.onAccent, shape = RoundedCornerShape(16.dp)) {
                    Text("Join group", fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                }
                TextButton(onClick = onBack, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text("Not now") }
            }
        }
    }
}

/** Who is in a group, what Beep does with their memories, and leaving it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GroupInfoScreen(repository: ChatRepository, groupId: String, onBack: () -> Unit, onInvite: (Group) -> Unit, onLeft: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val groups by repository.groups.collectAsState()
    var fetched by remember(groupId) { mutableStateOf<Group?>(null) }
    LaunchedEffect(groupId) {
        runCatching { withContext(Dispatchers.IO) { repository.group(groupId) } }.onSuccess { fetched = it.group }
    }
    val group = fetched ?: groups.firstOrNull { it.id == groupId }
    var confirmLeave by remember { mutableStateOf(false) }
    val joinedFormat = remember { DateTimeFormatter.ofPattern("d MMM") }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Group info"), style = displayStyle(22.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (group == null) {
                CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally).padding(top = 40.dp).size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                return@Column
            }
            Text("Members", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted, modifier = Modifier.padding(start = 4.dp, top = 4.dp))
            Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(horizontal = 14.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)) {
                group.members.forEachIndexed { i, m ->
                    val joined = Instant.ofEpochMilli(m.joinedAt).atZone(ZoneId.systemDefault()).format(joinedFormat)
                    MemberLine({ GroupAvatar(m.name.take(1).uppercase(), memberTint(i), 36.dp) },
                        if (m.you) "${m.name} (you)" else m.name, if (m.creator) "Created the group" else "Joined $joined")
                }
                MemberLine({ GroupAvatar("b", Beep.colors.brand, 36.dp, beep = true) }, "Beep", "Replies when someone writes @beep")
                ChunkyButton(onClick = { onInvite(group) }, modifier = Modifier.fillMaxWidth()) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(18.dp)); Text("Invite with a link", fontWeight = FontWeight.SemiBold)
                }
            }
            Text("Privacy", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted, modifier = Modifier.padding(start = 4.dp, top = 8.dp))
            Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(14.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp)) {
                PrivacyLine(Icons.Outlined.Psychology, "Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.")
                PrivacyLine(Icons.Outlined.Shield, "Beep's thinking and tool calls are hidden in groups. Members only see its replies.")
            }
            TextButton(onClick = { confirmLeave = true }) { Text("Leave group", color = Beep.colors.error) }
        }
    }
    if (confirmLeave) group?.let { g ->
        AlertDialog(onDismissRequest = { confirmLeave = false },
            confirmButton = { TextButton(onClick = {
                confirmLeave = false
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { repository.leaveGroup(g.id) } }
                        .onSuccess { onLeft() }
                        .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                }
            }) { Text("Leave", color = Beep.colors.error) } },
            dismissButton = { TextButton(onClick = { confirmLeave = false }) { Text("Cancel") } },
            title = { Text("Leave ${g.name}?") },
            text = { Text(if (g.members.size <= 1) "You are the last member, so the group and its messages are deleted."
                else "You stop getting its messages. Someone in it can invite you again.") },
            containerColor = Beep.colors.surface)
    }
}

@Composable
private fun MemberLine(avatar: @Composable () -> Unit, name: String, note: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        avatar()
        Spacer(Modifier.width(12.dp))
        Column {
            Text(name, fontWeight = FontWeight.Medium)
            Text(note, fontSize = 12.sp, color = Beep.colors.muted)
        }
    }
}
