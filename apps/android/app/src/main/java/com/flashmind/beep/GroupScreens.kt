package com.flashmind.beep

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
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
import com.flashmind.beep.ffi.formatGroupCode
import com.flashmind.beep.ffi.groupCode
import com.flashmind.beep.ffi.groupToken
import com.flashmind.beep.proto.Group
import com.flashmind.beep.proto.GroupAuthor
import com.flashmind.beep.proto.GroupChat
import com.flashmind.beep.proto.GroupLink
import com.flashmind.beep.proto.GroupMessage
import com.flashmind.beep.proto.GroupPreview
import com.flashmind.beep.proto.GroupReminder
import com.flashmind.beep.proto.ImageRef
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
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
    else -> group.last?.let { "${authorName(it.author)}: ${splitQuote(it.content).body}" } ?: groupPeople(group)
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

/** A message that opens with "> " lines: what it quotes, and what it says. */
internal data class Quoted(val quote: String?, val body: String)

internal fun splitQuote(content: String): Quoted {
    val lines = content.trim().lines()
    val quoted = lines.takeWhile { it.startsWith(">") }
    val body = lines.drop(quoted.size).joinToString("\n").trim()
    if (quoted.isEmpty() || body.isEmpty()) return Quoted(null, content)
    return Quoted(quoted.joinToString("\n") { it.removePrefix(">").trimStart() }.trim(), body)
}

/** The newest message before [before] whose text holds the quote's first line, so a tap on the quote can go there. */
internal fun quotedIndex(contents: List<String>, before: Int, quote: String): Int? {
    val first = quote.lines().first().trim()
    if (first.isEmpty()) return null
    return (before - 1 downTo 0).firstOrNull { contents[it].contains(first) }
}

/** Puts [quote] above [text] as "> " lines, the way a reply is sent. */
internal fun withQuote(quote: String, text: String) = quote.trim().lines().joinToString("\n") { "> $it" } + "\n\n" + text

private fun sameAuthor(a: GroupAuthor, b: GroupAuthor) = when {
    a is GroupAuthor.Beep && b is GroupAuthor.Beep -> true
    a is GroupAuthor.Member && b is GroupAuthor.Member -> a.you == b.you && a.name == b.name
    else -> false
}

/** Who a reply is to and the words it quotes. */
private data class ReplyTo(val name: String, val text: String)

/** A message being sent, shown until the server has it. */
private data class Outgoing(val requestId: String, val text: String)

/** A group chat: members' messages, Beep's replies and a composer that can @mention. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GroupChatPane(repository: ChatRepository, groupId: String, onBack: () -> Unit, onInfo: () -> Unit,
                           onInvite: (Group) -> Unit, onGone: () -> Unit, onNewChat: () -> Unit, onPrevious: () -> Unit,
                           onExpand: (() -> Unit)? = null) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var chat by remember(groupId) { mutableStateOf<GroupChat?>(null) }
    var outgoing by remember(groupId) { mutableStateOf(listOf<Outgoing>()) }
    var pictures by remember(groupId) { mutableStateOf(mapOf<Long, List<ImageRef>>()) }
    val listed by repository.groups.collectAsState()
    suspend fun load() {
        runCatching { withContext(Dispatchers.IO) { repository.group(groupId) } }
            .onSuccess { chat = it }
            .onFailure { if (it is BeepException.NotFound) onGone() }
        pictures = withContext(Dispatchers.IO) { repository.groupPictures(groupId) }
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
    var replyTo by remember(groupId) { mutableStateOf<ReplyTo?>(null) }
    // The message a tapped quote jumped to, lit up for a moment.
    var flashing by remember(groupId) { mutableStateOf<Long?>(null) }
    LaunchedEffect(flashing) { if (flashing != null) { delay(1800); flashing = null } }
    LaunchedEffect(field.text) { repository.setDraft("group:$groupId", repository.draft("group:$groupId").copy(text = field.text)) }
    fun send() {
        val typed = field.text.trim()
        if (typed.isEmpty()) return
        val reply = replyTo
        val text = reply?.let { withQuote(it.text, typed) } ?: typed
        val out = Outgoing(UUID.randomUUID().toString(), text)
        outgoing = outgoing + out
        field = TextFieldValue("")
        replyTo = null
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.sendToGroup(groupId, out.requestId, text) } }
                .onSuccess { load() }
                .onFailure {
                    Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show()
                    if (field.text.isEmpty()) { field = TextFieldValue(typed, TextRange(typed.length)); replyTo = reply }
                }
            outgoing = outgoing - out
        }
    }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = {
            Column(Modifier.clickable(enabled = group != null, onClick = onInfo)) {
                Text(displayText(group?.name ?: "Group"), style = displayStyle(24.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
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
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Chats") }
        }, actions = {
            IconButton(onClick = { group?.let(onInvite) }, enabled = group != null, modifier = Modifier.padding(end = 6.dp)) {
                Icon(Icons.Outlined.Link, "Invite", tint = Beep.colors.muted)
            }
        }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().imePadding()) {
            // Swipes work as in a one to one chat: left starts a new chat, right goes back to the chat before.
            SwipeChats(canStartNew = true, hasPrevious = true, onNew = onNewChat, onBack = onPrevious,
                modifier = Modifier.weight(1f).fillMaxWidth()) {
            Box(Modifier.fillMaxSize()) {
                val loaded = chat
                when {
                    loaded == null -> CircularProgressIndicator(Modifier.align(Alignment.Center).size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                    messages.isEmpty() && outgoing.isEmpty() -> Text("No messages yet.\nSay hi. Beep joins in when it can help.",
                        Modifier.align(Alignment.Center).padding(32.dp), textAlign = TextAlign.Center, color = Beep.colors.muted, lineHeight = 22.sp)
                    else -> {
                        val days = remember(messages) { groupDays(messages) }
                        // Quoting a message or a selection makes the next message a reply to it.
                        fun quote(author: GroupAuthor): (String) -> Unit = { text ->
                            val you = author is GroupAuthor.Member && author.you
                            replyTo = ReplyTo(if (you) "yourself" else authorName(author), text)
                        }
                        val contents = remember(messages) { messages.map { it.content } }
                        LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(18.dp)) {
                            itemsIndexed(messages, key = { _, m -> m.seq }) { i, message ->
                                // Messages in a row from one person form a run: one name, tighter gaps and corners.
                                val day = days[message.seq]
                                val joinsAbove = i > 0 && day == null && sameAuthor(messages[i - 1].author, message.author)
                                val joinsBelow = i + 1 < messages.size && days[messages[i + 1].seq] == null &&
                                    sameAuthor(messages[i + 1].author, message.author)
                                Column(Modifier.padding(top = if (i == 0) 0.dp else if (joinsAbove) 3.dp else 16.dp)) {
                                    day?.let {
                                        Text(it, Modifier.fillMaxWidth().padding(bottom = 12.dp), fontSize = 11.sp,
                                            color = Beep.colors.dim, textAlign = TextAlign.Center)
                                    }
                                    val split = remember(message.content) { splitQuote(message.content) }
                                    val target = split.quote?.let { quotedIndex(contents, i, it) }
                                    GroupBubble(loaded.group, message.author, split, quote(message.author),
                                        first = !joinsAbove, last = !joinsBelow, flash = flashing == message.seq,
                                        pictures = pictures[message.seq].orEmpty(),
                                        quotedAuthor = target?.let { messages[it].author },
                                        onQuoteTap = target?.let { t -> {
                                            scope.launch { listState.animateScrollToItem(t) }
                                            flashing = messages[t].seq
                                        } })
                                }
                            }
                            items(outgoing, key = { it.requestId }) { out ->
                                Box(Modifier.padding(top = 16.dp).alpha(0.6f)) {
                                    GroupBubble(loaded.group, GroupAuthor.Member("", you = true), splitQuote(out.text), {}, first = true, last = true)
                                }
                            }
                            if (typing) item(key = "typing") {
                                Row(Modifier.padding(start = 4.dp, top = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                                    PulsingDots()
                                    Spacer(Modifier.width(10.dp))
                                    Text("Beep is typing", fontSize = 13.sp, color = Beep.colors.muted)
                                }
                            }
                        }
                    }
                }
            }
            }
            group?.let { MentionPicker(it, field) { field = it } }
            replyTo?.let { reply ->
                Row(Modifier.padding(start = 14.dp, end = 16.dp, top = 6.dp).fillMaxWidth()
                    .beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Beep.colors.surface)
                    .padding(start = 12.dp, top = 6.dp, bottom = 6.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.width(3.dp).height(34.dp).background(Beep.colors.brand, RoundedCornerShape(2.dp)))
                    Column(Modifier.weight(1f).padding(start = 10.dp)) {
                        Text("Replying to ${reply.name}", fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                            color = if (reply.name == "Beep") Beep.colors.brand else nameColors[(group?.let { memberIndex(it, reply.name) } ?: 0).mod(nameColors.size)])
                        Text(reply.text.lines().joinToString(" "), fontSize = 13.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    IconButton(onClick = { replyTo = null }, Modifier.size(32.dp)) {
                        Icon(Icons.Outlined.Close, "Cancel reply", Modifier.size(16.dp), tint = Beep.colors.muted)
                    }
                }
            }
            GroupComposer(field, onChange = { if (it.text.toByteArray().size <= 8192) field = it }, onSend = ::send, onExpand = onExpand)
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

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun GroupBubble(group: Group, author: GroupAuthor, message: Quoted, onQuote: (String) -> Unit,
                        first: Boolean, last: Boolean, flash: Boolean = false,
                        quotedAuthor: GroupAuthor? = null, onQuoteTap: (() -> Unit)? = null,
                        pictures: List<ImageRef> = emptyList()) {
    val radius = Beep.shapes.radius
    val you = author is GroupAuthor.Member && author.you
    val beep = author is GroupAuthor.Beep
    val name = if (author is GroupAuthor.Member) author.name else "Beep"
    val content = message.body
    var menuOpen by remember { mutableStateOf(false) }
    // The corners on the run's side stay tight between messages of one run.
    val shape = if (you) RoundedCornerShape(radius, if (first) radius else 6.dp, 6.dp, radius)
        else RoundedCornerShape(6.dp, radius, radius, if (last) radius else 6.dp)
    val glow by animateFloatAsState(if (flash) 1f else 0f, tween(if (flash) 200 else 1600), label = "quoted-flash")
    // The same bubbles as a one to one chat: yours on the right, Beep's reply as Beep writes it there,
    // and other members on the left with their name on top.
    val frame = when {
        you -> Modifier.padding(start = 48.dp).then(if (Beep.shapes.card == CardLook.Hairline)
            Modifier.clip(shape).background(Beep.colors.user) else Modifier.beepFrame(shape, Beep.colors.user))
        beep && !Beep.shapes.botBubble -> Modifier.fillMaxWidth()
        else -> Modifier.padding(end = if (beep) 24.dp else 48.dp).beepFrame(shape, Beep.colors.surface)
    }
    Column(Modifier.fillMaxWidth().background(Beep.colors.tints[2].copy(alpha = glow * 0.8f), RoundedCornerShape(radius)),
        horizontalAlignment = if (you) Alignment.End else Alignment.Start) {
        if (beep && first) Text("Beep", Modifier.padding(start = 4.dp, bottom = 4.dp), fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold, color = Beep.colors.brand)
        SelectableMessage(onQuote) { picker -> Box {
            // Long press and double tap select text, so a tap opens the menu.
            Column(frame.then(picker.tracking).combinedClickable(onClickLabel = "Message actions", indication = null,
                interactionSource = remember { MutableInteractionSource() }, onDoubleClick = picker::selectWord) {
                if (!picker.picking) menuOpen = true
            }) {
                if (!you && !beep && first) Text(name, Modifier.padding(start = 16.dp, end = 16.dp, top = 10.dp), fontSize = 12.sp,
                    fontWeight = FontWeight.SemiBold, color = nameColors[memberIndex(group, name).mod(nameColors.size)])
                SentPictureRow(pictures)
                message.quote?.let { QuoteChip(group, quotedAuthor, it, padded = !beep || Beep.shapes.botBubble, onClick = onQuoteTap) }
                MessageContent(withMentions(content), if (you) "user" else "assistant", compact = false,
                    padded = !beep || Beep.shapes.botBubble)
            }
            MessageMenu(menuOpen, onDismiss = { menuOpen = false }, text = content,
                onQuote = { onQuote(content.trim().lines().take(4).joinToString("\n")) })
        } }
    }
}

/** The quoted message at the top of a reply. A tap goes to it when it is still in the chat. */
@Composable
private fun QuoteChip(group: Group, author: GroupAuthor?, text: String, padded: Boolean, onClick: (() -> Unit)?) {
    val name = author?.let { authorName(it) }
    val color = when {
        author is GroupAuthor.Beep -> Beep.colors.brand
        author is GroupAuthor.Member && !author.you -> nameColors[memberIndex(group, author.name).mod(nameColors.size)]
        else -> Beep.colors.ink
    }
    Row(Modifier.padding(start = if (padded) 10.dp else 0.dp, end = if (padded) 10.dp else 0.dp, top = if (padded) 8.dp else 0.dp)
        .clip(RoundedCornerShape(10.dp)).background(Beep.colors.page.copy(alpha = 0.6f))
        .then(if (onClick != null) Modifier.clickable(onClickLabel = "Go to the quoted message", onClick = onClick) else Modifier)
        .height(IntrinsicSize.Min)) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(color))
        Column(Modifier.padding(horizontal = 8.dp, vertical = 5.dp)) {
            name?.let { Text(it, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = color) }
            Text(text.lines().joinToString(" "), fontSize = 13.sp, lineHeight = 17.sp, color = Beep.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
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
private fun GroupComposer(field: TextFieldValue, onChange: (TextFieldValue) -> Unit, onSend: () -> Unit, onExpand: (() -> Unit)?) {
    val size = 50.dp
    val shape = RoundedCornerShape(Beep.shapes.radius)
    // In the bubble, the same open-full-app button as a normal chat.
    Row(Modifier.padding(start = if (onExpand != null) 2.dp else 14.dp, end = 16.dp, top = 8.dp, bottom = 12.dp).fillMaxWidth(),
        verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(if (onExpand != null) 2.dp else 8.dp)) {
        if (onExpand != null) {
            IconButton(onClick = onExpand, modifier = Modifier.size(size)) {
                Icon(Icons.Outlined.OpenInFull, "Open full app", Modifier.size(18.dp), tint = Beep.colors.muted)
            }
        }
        Row(Modifier.weight(1f).heightIn(min = size).beepFrame(shape, Beep.colors.surface, pressableLook()),
            verticalAlignment = Alignment.CenterVertically) {
            TextField(field, onChange, Modifier.weight(1f),
                placeholder = { Text("Message", fontSize = 15.sp, color = Beep.colors.dim) },
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

/** Makes a group with the user in it, then hands it over so its invite code can be sent. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun NewGroupSheet(repository: ChatRepository, onDismiss: () -> Unit, onCreated: (Group) -> Unit, onJoinCode: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var memberName by remember { mutableStateOf(lastMemberName(context)) }
    var busy by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SheetHeader(Icons.Outlined.Groups, Beep.colors.tints[2], "New group", "Chat with people, and Beep helps when it can")
            NameField("Group name", name, { if (it.length <= 60) name = it })
            NameField("Your name in the group", memberName, { if (it.length <= 30) memberName = it })
            Text("You get a code to send to people. Anyone with Beep who enters it can join.",
                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onJoinCode, contentPadding = PaddingValues(horizontal = 0.dp)) { Text("Join with a code") }
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

/** The group's invite code, to copy, send or replace. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun GroupInviteSheet(repository: ChatRepository, groupId: String, groupName: String, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var link by remember(groupId) { mutableStateOf<GroupLink?>(null) }
    val code = link?.let { formatGroupCode(it.token) }
    var reset by remember(groupId) { mutableStateOf(false) }
    LaunchedEffect(groupId) {
        runCatching { withContext(Dispatchers.IO) { repository.groupInvite(groupId) } }
            .onSuccess { link = it }
            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show(); onDismiss() }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            SheetHeader(Icons.Outlined.Link, Beep.colors.tints[1], "Invite people", groupName)
            Box(Modifier.size(180.dp).align(Alignment.CenterHorizontally).background(QR_PAPER, RoundedCornerShape(12.dp)).padding(12.dp),
                contentAlignment = Alignment.Center) {
                link?.let { QrCode(it.url, Modifier.fillMaxSize()) }
            }
            Box(Modifier.fillMaxWidth().height(56.dp).background(Beep.colors.page, RoundedCornerShape(12.dp)),
                contentAlignment = Alignment.Center) {
                val shown = code
                if (shown == null) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                else Text(shown, fontFamily = FontFamily.Monospace, fontSize = 26.sp, fontWeight = FontWeight.SemiBold,
                    letterSpacing = 3.sp, color = Beep.colors.ink, maxLines = 1)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ChunkyButton(onClick = {
                    (context as? MainActivity)?.suppressNextClipboard = true
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Beep group link", link?.url))
                    Toast.makeText(context, "Link copied", Toast.LENGTH_SHORT).show()
                }, enabled = code != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(12.dp)) {
                    Icon(Icons.Outlined.ContentCopy, null, Modifier.size(18.dp)); Text("Copy link", fontWeight = FontWeight.SemiBold)
                }
                ChunkyButton(onClick = {
                    val text = "Join $groupName on Beep: ${link?.url}\n" +
                        "Or open Beep, start a new group, tap Join with a code and enter $code."
                    context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null))
                }, enabled = code != null, modifier = Modifier.weight(1f).height(48.dp), shape = RoundedCornerShape(12.dp),
                    color = Beep.colors.accent, contentColor = Beep.colors.onAccent) {
                    Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)); Text("Send link", fontWeight = FontWeight.SemiBold)
                }
            }
            Text((if (reset) "New code made. The old one no longer works. " else "") +
                "People open the link or scan the code, or enter the code under New group, Join with a code, and see the whole chat. Beep uses what it knows about each of you " +
                "to help, but never shows one person's memories to the others.",
                fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = {
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { repository.groupInvite(groupId, reset = true) } }
                            .onSuccess { link = it; reset = true }
                            .onFailure { Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
                    }
                }, enabled = code != null) { Text("Reset code", color = Beep.colors.error) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onDismiss) { Text("Done") }
            }
        }
    }
}

/** Entering an invite code someone sent. Case, spaces and dashes do not matter. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun JoinCodeSheet(repository: ChatRepository, onDismiss: () -> Unit, onFound: (String) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var typed by remember { mutableStateOf("") }
    var wrong by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val code = groupCode(typed)
    fun find(found: String) {
        busy = true
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.peekGroup(found) } }
                .onSuccess { onFound(found) }
                .onFailure {
                    busy = false
                    if (it is BeepException.NotFound) wrong = true
                    else Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show()
                }
        }
    }
    fun scan() {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { barcode ->
                // The QR holds an invite link to the code, https or an older beep://group/ one.
                val scanned = barcode.rawValue?.let(::groupToken)?.let(::groupCode)
                if (scanned == null) Toast.makeText(context, "That is not a Beep group code", Toast.LENGTH_SHORT).show()
                else { typed = formatGroupCode(scanned); wrong = false; find(scanned) }
            }
            .addOnFailureListener { Toast.makeText(context, "Could not open the scanner", Toast.LENGTH_SHORT).show() }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.surface) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SheetHeader(Icons.Outlined.Groups, Beep.colors.tints[1], "Join a group", "Enter or scan the code someone sent you")
            OutlinedTextField(typed, { if (it.length <= 24) { typed = it.uppercase(); wrong = false } }, Modifier.fillMaxWidth(),
                label = { Text("Invite code") }, singleLine = true, isError = wrong,
                textStyle = LocalTextStyle.current.copy(fontFamily = FontFamily.Monospace, fontSize = 18.sp, letterSpacing = 2.sp),
                supportingText = { Text(if (wrong) "No group has this code. Check it, or ask for a new one." else "Ten letters and digits, like K7QM4-X9TRP") },
                shape = RoundedCornerShape(12.dp),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, autoCorrectEnabled = false),
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Beep.colors.page, focusedContainerColor = Beep.colors.page,
                    unfocusedBorderColor = Beep.colors.softLine, focusedBorderColor = Beep.colors.brand))
            ChunkyButton(onClick = ::scan, enabled = !busy, modifier = Modifier.fillMaxWidth().height(48.dp), shape = RoundedCornerShape(12.dp)) {
                Icon(Icons.Outlined.QrCodeScanner, null, Modifier.size(18.dp)); Text("Scan a QR code", fontWeight = FontWeight.SemiBold)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onDismiss) { Text("Cancel") }
                Spacer(Modifier.width(8.dp))
                ChunkyButton(onClick = { code?.let(::find) }, enabled = !busy && code != null && !wrong, color = Beep.colors.accent,
                    contentColor = Beep.colors.onAccent, shape = RoundedCornerShape(12.dp)) {
                    Text("Find group", fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

/** The light card behind a QR code, so any camera reads it in dark mode too. */
private val QR_PAPER = Color(0xFFF4F8F5)

/** [text] as a QR code, drawn square by square. */
@Composable
private fun QrCode(text: String, modifier: Modifier = Modifier) {
    val matrix = remember(text) {
        QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.MARGIN to 0))
    }
    Canvas(modifier) {
        val cell = size.minDimension / matrix.width
        // A hair wider than a cell, so no seams show between squares.
        val square = Size(cell + 0.5f, cell + 0.5f)
        for (y in 0 until matrix.height) for (x in 0 until matrix.width) {
            if (matrix[x, y]) drawRect(Color(0xFF0A0E0C), Offset(x * cell, y * cell), square)
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

/** Where an invite code leads: the group's name and members, and a name to join as. */
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
                Text(displayText("This code no longer works"), style = displayStyle(30.sp), textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
                Text("Whoever sent it reset the group's code. Ask them for a new one.", color = Beep.colors.muted,
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
                    PrivacyLine(Icons.Outlined.Shield, "Beep reads the messages to see when it can help. When it answers you, it runs with your apps and your account, never someone else's.")
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
    // The member whose sheet is open, by index.
    var member by remember { mutableStateOf<Int?>(null) }
    var reminders by remember(groupId) { mutableStateOf<List<GroupReminder>?>(null) }
    LaunchedEffect(groupId) {
        runCatching { withContext(Dispatchers.IO) { repository.groupReminders(groupId) } }.onSuccess { reminders = it }
    }
    val joinedFormat = remember { DateTimeFormatter.ofPattern("d MMM") }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Group info"), style = displayStyle(22.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (group == null) {
                CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally).padding(top = 40.dp).size(22.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
                return@Column
            }
            Column(Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 6.dp), horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(6.dp)) {
                val initials = group.name.split(' ').filter { it.isNotBlank() }.take(2).joinToString("") { it.take(1).uppercase() }
                GroupAvatar(initials.ifEmpty { "?" }, Beep.colors.tints[group.id.hashCode().mod(Beep.colors.tints.size)], 84.dp)
                Text(displayText(group.name), style = displayStyle(28.sp), textAlign = TextAlign.Center)
                Text("Group · ${group.members.size} member${if (group.members.size == 1) "" else "s"}", fontSize = 13.sp, color = Beep.colors.muted)
            }
            Text("Members", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted, modifier = Modifier.padding(start = 4.dp, top = 4.dp))
            Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(horizontal = 14.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)) {
                group.members.forEachIndexed { i, m ->
                    val joined = Instant.ofEpochMilli(m.joinedAt).atZone(ZoneId.systemDefault()).format(joinedFormat)
                    MemberLine({ GroupAvatar(m.name.take(1).uppercase(), memberTint(i), 36.dp) },
                        if (m.you) "You" else m.name, if (m.you) "${m.name} · " + (if (m.creator) "made the group" else "joined $joined")
                            else if (m.creator) "Made the group" else "Joined $joined",
                        made = m.creator, onClick = if (m.you) null else { { member = i } })
                }
                MemberLine({ GroupAvatar("b", Beep.colors.brand, 36.dp, beep = true) }, "Beep", "Joins in when it can help, or when someone writes @beep")
                ChunkyButton(onClick = { onInvite(group) }, modifier = Modifier.fillMaxWidth()) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(18.dp)); Text("Invite with a link", fontWeight = FontWeight.SemiBold)
                }
            }
            Text("Reminders", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted, modifier = Modifier.padding(start = 4.dp, top = 8.dp))
            GroupReminders(reminders, you = group.members.firstOrNull { it.you }?.name) { reminder ->
                reminders = reminders?.filter { it.id != reminder.id }
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { repository.deleteGroupReminder(group.id, reminder.id) } }
                        .onSuccess { Toast.makeText(context, "Reminder deleted", Toast.LENGTH_SHORT).show() }
                        .onFailure {
                            Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show()
                            reminders = runCatching { withContext(Dispatchers.IO) { repository.groupReminders(group.id) } }.getOrNull() ?: reminders
                        }
                }
            }
            Text("Your apps", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted, modifier = Modifier.padding(start = 4.dp, top = 8.dp))
            Row(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Share my apps with Beep here", fontWeight = FontWeight.Medium)
                    Text(if (group.appsShared) "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first."
                        else "Beep uses your apps here only when you ask it yourself.", fontSize = 12.sp, lineHeight = 17.sp, color = Beep.colors.muted)
                }
                Spacer(Modifier.width(10.dp))
                Switch(checked = group.appsShared, onCheckedChange = { shared ->
                    fetched = fetched?.copy(appsShared = shared)
                    repository.shareGroupApps(group.id, shared)
                }, modifier = Modifier.semantics { contentDescription = "Share my apps with Beep here" },
                    colors = SwitchDefaults.colors(checkedTrackColor = Beep.colors.ok))
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
    val shown = member?.let { group?.members?.getOrNull(it) }
    if (group != null && shown != null) ModalBottomSheet(onDismissRequest = { member = null }, containerColor = Beep.colors.page) {
        val i = group.members.indexOf(shown)
        val day = Instant.ofEpochMilli(shown.joinedAt).atZone(ZoneId.systemDefault()).format(joinedFormat)
        Column(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                GroupAvatar(shown.name.take(1).uppercase(), memberTint(i), 56.dp)
                Column {
                    Text(shown.name, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                    Text(if (shown.creator) "Made the group on $day" else "Joined $day", fontSize = 13.sp, color = Beep.colors.muted)
                }
            }
            reminders?.let { all ->
                Row(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(14.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.Schedule, null, Modifier.size(18.dp), tint = Beep.colors.muted)
                    Text("Reminders set", Modifier.weight(1f).padding(start = 10.dp), fontSize = 13.sp)
                    Text("${all.count { it.setBy == shown.name }}", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            ChunkyButton(onClick = {
                val key = "group:${group.id}"
                val draft = repository.draft(key)
                repository.setDraft(key, draft.copy(text = "@${shown.name} " + draft.text.trimStart()))
                member = null
                onBack()
            }, modifier = Modifier.fillMaxWidth()) { Text("@ Mention ${shown.name}", fontWeight = FontWeight.SemiBold) }
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

/** The group's reminders, each with when it goes and who set it. Beep posts them into the chat. */
@Composable
private fun GroupReminders(reminders: List<GroupReminder>?, you: String?, onDelete: (GroupReminder) -> Unit) {
    val ask = "Ask Beep in the chat to add one, like \"remind us at 10 to check out\"."
    Column(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.surface).padding(horizontal = 14.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)) {
        when {
            reminders == null -> CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally).size(18.dp), strokeWidth = 2.dp, color = Beep.colors.muted)
            reminders.isEmpty() -> Text("No reminders yet. $ask", fontSize = 13.sp, lineHeight = 19.sp, color = Beep.colors.muted)
            else -> {
                reminders.forEachIndexed { i, reminder ->
                    val once = onceAt(reminder.schedule)
                    val time = once?.let { describeDay(it).replaceFirstChar(Char::uppercase) } ?: describeSchedule(reminder.schedule)
                    val by = if (reminder.setBy == you) "you" else reminder.setBy
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(36.dp).beepFrame(RoundedCornerShape(10.dp), memberTint(i)), contentAlignment = Alignment.Center) {
                            Icon(Icons.Outlined.Schedule, null, Modifier.size(18.dp), tint = Beep.colors.ink)
                        }
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(reminder.label, fontWeight = FontWeight.Medium)
                            Text(reminder.message, fontSize = 12.sp, lineHeight = 17.sp, color = Beep.colors.muted)
                            Text("$time · set by $by", fontSize = 12.sp, color = Beep.colors.muted)
                        }
                        IconButton(onClick = { onDelete(reminder) }, modifier = Modifier.size(36.dp)) {
                            Icon(Icons.Outlined.Close, "Delete reminder", Modifier.size(18.dp), tint = Beep.colors.muted)
                        }
                    }
                }
                Text(ask, fontSize = 12.sp, lineHeight = 17.sp, color = Beep.colors.muted)
            }
        }
    }
}

@Composable
private fun MemberLine(avatar: @Composable () -> Unit, name: String, note: String, made: Boolean = false, onClick: (() -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().then(if (onClick != null) Modifier.clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick) else Modifier),
        verticalAlignment = Alignment.CenterVertically) {
        avatar()
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(name, fontWeight = FontWeight.Medium)
                if (made) Text("Made it", Modifier.background(Beep.colors.tints[1], RoundedCornerShape(6.dp)).padding(horizontal = 6.dp, vertical = 1.dp),
                    fontSize = 10.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.ink)
            }
            Text(note, fontSize = 12.sp, color = Beep.colors.muted)
        }
        if (onClick != null) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = Beep.colors.dim)
    }
}
