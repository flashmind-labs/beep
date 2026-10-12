package com.flashmind.beep

import com.flashmind.beep.proto.MemoryRead
import com.flashmind.beep.proto.MemorySource
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val Coral: Color @Composable @ReadOnlyComposable get() = Beep.colors.error
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun MemoriesScreen(repository: ChatRepository, latestRead: Boolean = false, onBack: () -> Unit) {
    val cached = remember { repository.cachedMemories() }
    var memories by remember { mutableStateOf(cached.orEmpty()) }
    var query by rememberSaveable { mutableStateOf("") }
    var loading by remember { mutableStateOf(cached == null) }
    var error by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<ChatRepository.Memory?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    var reads by remember { mutableStateOf(emptyList<MemoryRead>()) }
    var readId by rememberSaveable { mutableStateOf<String?>(null) }
    // The latest read is picked once; choosing All afterwards sticks through edits.
    var defaulted by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        error = ""
        try {
            memories = withContext(Dispatchers.IO) { repository.listMemories() }
            reads = withContext(Dispatchers.IO) { repository.listMemoryReads() }
            if (latestRead && !defaulted) readId = reads.firstOrNull()?.id
            defaulted = true
        }
        catch (e: Exception) { error = userMessage(e) }
        finally { loading = false }
    } }
    fun act(action: suspend () -> Unit) { scope.launch {
        try { withContext(Dispatchers.IO) { action() }; refresh() }
        catch (e: Exception) { error = userMessage(e) }
    } }
    LaunchedEffect(Unit) { refresh() }
    val today = remember { LocalDate.now().toString() }
    val read = reads.firstOrNull { it.id == readId }
    val inRead = read?.memories?.toSet()
    val groups = memories
        .filter { inRead == null || it.id in inRead }
        .filter { query.isBlank() || it.content.contains(query, true) || it.category.contains(query, true) }
        .sortedByDescending { it.updated }
        .groupBy { it.category }
        .toSortedMap()
    val shown = groups.values.sumOf { it.size }
    editing?.let { memory ->
        var text by remember(memory.id) { mutableStateOf(memory.content) }
        AlertDialog(
            onDismissRequest = { editing = null },
            confirmButton = { TextButton(onClick = {
                val content = text.trim()
                if (content.isNotEmpty()) { act { repository.updateMemory(memory.id, content) }; editing = null }
            }) { Text("Save") } },
            dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
            title = { Text("Edit memory") },
            text = { OutlinedTextField(text, { text = it }, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) },
            containerColor = Surface)
    }
    if (confirmClear) AlertDialog(
        onDismissRequest = { confirmClear = false },
        confirmButton = { TextButton(onClick = { act { repository.clearMemories() }; confirmClear = false }) { Text("Forget all", color = Coral) } },
        dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("Cancel") } },
        title = { Text("Forget everything?") },
        text = { Text("Beep will forget all ${memories.size} memories. This cannot be undone.") },
        containerColor = Surface)
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText("Memories"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { if (memories.isNotEmpty()) TextButton(onClick = { confirmClear = true }) { Text("Clear all", color = Coral, fontSize = 13.sp) } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(query, { query = it }, placeholder = { Text("Search memories", fontSize = 13.sp) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.weight(1f),
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                Spacer(Modifier.width(12.dp))
                Text("$shown", fontSize = 12.sp, color = TextMuted)
            }
            if (reads.isNotEmpty()) Row(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 20.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ReadChip("All", null, read == null) { readId = null }
                reads.take(3).forEach { r -> ReadChip(readLabel(r), r.memories.size, r.id == readId) { readId = r.id } }
            }
            read?.let { Text("What Beep learned or confirmed reading ${it.source} ${readDate(it)}.", fontSize = 12.sp, color = TextMuted,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 6.dp)) }
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp))
            if (loading && memories.isEmpty()) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (shown == 0) Text(if (memories.isEmpty()) "Nothing remembered yet.\nBeep learns as you chat, or tell it \"remember that...\"" else "No matches.",
                fontSize = 13.sp, color = TextMuted, lineHeight = 20.sp, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                groups.forEach { (category, list) ->
                    item(key = "category-$category") {
                        Text(category.replaceFirstChar { it.uppercase() }, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextDim,
                            modifier = Modifier.padding(start = 4.dp, top = 10.dp, bottom = 2.dp))
                    }
                    items(list, key = { it.id }) { memory -> MemoryCard(memory, today, onEdit = { editing = memory }, onDelete = { act { repository.deleteMemory(memory.id) } }) }
                }
            }
        }
    }
}

/** A `YYYY-MM-DD` date as "9 Oct", with the year when it is not this year. */
private fun shortDate(date: String): String = try {
    val day = LocalDate.parse(date)
    val pattern = if (day.year == LocalDate.now().year) "d MMM" else "d MMM yyyy"
    day.format(DateTimeFormatter.ofPattern(pattern))
} catch (_: Exception) { date }

@Composable
private fun MemoryCard(memory: ChatRepository.Memory, today: String, onEdit: () -> Unit, onDelete: () -> Unit) {
    val ended = memory.validUntil?.let { it < today } == true
    val starts = memory.validFrom?.takeIf { it > today }?.let { "Starts ${shortDate(it)}" }
    val expires = memory.validUntil?.let { if (ended) "Expired ${shortDate(it)}" else "Expires ${shortDate(it)}" }
    val origin = when (memory.source) {
        MemorySource.USER -> "You told Beep"
        MemorySource.EMAIL -> "From your email"
        MemorySource.SLACK -> "From Slack"
        MemorySource.LEARNED -> "Learned"
    }
    val details = listOfNotNull(origin, shortDate(memory.updated), starts)
    BeepCard(Modifier.fillMaxWidth().clickable(onClick = onEdit)) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f)) {
                Text(memory.content, fontSize = 13.sp, lineHeight = 19.sp, color = if (ended) TextMuted else Color.Unspecified)
                Spacer(Modifier.height(4.dp))
                Text(details.joinToString(" \u00b7 "), fontSize = 11.sp, color = TextDim)
                memory.quote?.let { quote ->
                    Text("\u201c$quote\u201d", fontSize = 11.sp, lineHeight = 15.sp, color = TextDim, fontStyle = FontStyle.Italic,
                        maxLines = 3, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 2.dp))
                }
                if (expires != null) {
                    Row(Modifier.padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Outlined.Schedule, null, Modifier.size(12.dp), tint = if (ended) TextDim else Beep.colors.accent)
                        Spacer(Modifier.width(4.dp))
                        Text(expires, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = if (ended) TextDim else Beep.colors.accent)
                    }
                }
            }
            IconButton(onClick = onDelete, modifier = Modifier.size(36.dp)) {
                Icon(Icons.Outlined.Close, "Forget", Modifier.size(16.dp), tint = TextDim)
            }
        }
    }
}

@Composable
private fun ReadChip(label: String, count: Int?, selected: Boolean, onClick: () -> Unit) {
    FilterChip(selected, onClick, label = {
        Text(label, fontSize = 12.sp)
        count?.let { Text("  $it", fontSize = 12.sp, color = TextDim) }
    }, colors = FilterChipDefaults.filterChipColors(containerColor = Surface, selectedContainerColor = Moss.copy(alpha = 0.18f)),
        border = FilterChipDefaults.filterChipBorder(true, selected, borderColor = Border, selectedBorderColor = Moss))
}

private fun readFinished(read: MemoryRead) = 
    runCatching { OffsetDateTime.parse(read.finishedAt).atZoneSameInstant(ZoneId.systemDefault()).toLocalDate() }.getOrNull()
private fun readLabel(read: MemoryRead) =
    readFinished(read)?.let { "${read.source} · ${it.format(DateTimeFormatter.ofPattern("d MMM"))}" } ?: read.source
private fun readDate(read: MemoryRead): String {
    val day = readFinished(read) ?: return "earlier"
    return if (day == LocalDate.now()) "today" else "on ${day.format(DateTimeFormatter.ofPattern("d MMMM"))}"
}
