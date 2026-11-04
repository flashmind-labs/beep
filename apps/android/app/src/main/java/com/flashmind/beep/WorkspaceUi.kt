package com.flashmind.beep

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay

internal class WorkspaceLoad<T> {
    var value by mutableStateOf<T?>(null)
    var error by mutableStateOf<String?>(null)
    var loading by mutableStateOf(true)
    var revision by mutableIntStateOf(0)
    fun refresh() { revision++ }
}

@Composable
internal fun <T> workspaceLoad(key: Any?, poll: Boolean = false, load: suspend () -> T): WorkspaceLoad<T> {
    val state = remember(key) { WorkspaceLoad<T>() }
    val loader by rememberUpdatedState(load)
    val owner = LocalLifecycleOwner.current
    LaunchedEffect(state, state.revision, owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            do {
                state.loading = true
                try { state.value = loader(); state.error = null }
                catch (e: CancellationException) { throw e }
                catch (e: Exception) { state.error = userMessage(e) }
                finally { state.loading = false }
                if (poll) delay(5_000)
            } while (poll)
        }
    }
    return state
}

internal suspend fun workspaceAction(onError: (String?) -> Unit, block: suspend () -> Unit) {
    onError(null)
    try { block() }
    catch (e: CancellationException) { throw e }
    catch (e: Exception) { onError(userMessage(e)) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun WorkspacePage(title: String, onBack: () -> Unit, refresh: (() -> Unit)? = null,
    tabs: @Composable () -> Unit = {}, actions: @Composable RowScope.() -> Unit = {},
    content: @Composable ColumnScope.() -> Unit) {
    BackHandler(onBack = onBack)
    Scaffold(containerColor = Beep.colors.page, topBar = {
        Column {
            TopAppBar(modifier = Modifier.beepBar(), expandedHeight = 54.dp, title = { Text(title, style = displayStyle(18.sp), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
                actions = {
                    actions()
                    if (refresh != null) IconButton(onClick = refresh) { Icon(Icons.Outlined.Refresh, "Refresh", Modifier.size(20.dp)) }
                }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
            tabs()
        }
    }) { padding -> Column(Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize().imePadding(), content = content) }
}

@Composable
internal fun WorkspaceBody(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    LazyColumn(modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item { Column(verticalArrangement = Arrangement.spacedBy(14.dp), content = content) }
    }
}

@Composable
internal fun WorkspaceHeading(text: String, action: String? = null, onAction: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(text, Modifier.weight(1f), fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
        if (action != null) TextButton(onClick = onAction) { Text(action, fontSize = 12.sp) }
    }
}

@Composable
internal fun WorkspaceRow(icon: ImageVector, title: String, detail: String = "", tag: String? = null,
    onClick: (() -> Unit)? = null, tagColor: Color = Beep.colors.muted) {
    val divider = Beep.colors.softLine
    Row(Modifier.fillMaxWidth().drawWithContent {
        drawContent()
        drawLine(divider, Offset(0f, size.height), Offset(size.width, size.height), strokeWidth = 1.dp.toPx())
    }.then(if (onClick == null) Modifier else Modifier.clickable(onClick = onClick))
        .heightIn(min = 48.dp).padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(28.dp).background(Beep.colors.raised, RoundedCornerShape(6.dp))
            .border(1.dp, Beep.colors.softLine, RoundedCornerShape(6.dp)), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(16.dp), tint = Beep.colors.brand)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (detail.isNotEmpty()) Text(detail, fontSize = 11.sp, color = Beep.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (!tag.isNullOrEmpty()) WorkspaceTag(tag, tagColor)
        if (onClick != null) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(14.dp), tint = Beep.colors.dim)
    }
}

@Composable
internal fun WorkspaceTag(text: String, color: Color = Beep.colors.muted) {
    Text(text, fontSize = 10.sp, color = color, maxLines = 1,
        modifier = Modifier.border(1.dp, Beep.colors.softLine, RoundedCornerShape(6.dp)).padding(horizontal = 6.dp, vertical = 2.dp))
}

@Composable
internal fun WorkspaceFacts(facts: List<Pair<String, String>>) {
    BeepCard(Modifier.fillMaxWidth()) {
        facts.chunked(2).forEachIndexed { index, row ->
            if (index > 0) HorizontalDivider(color = Beep.colors.softLine)
            Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                row.forEach { (label, value) -> Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(label, fontSize = 11.sp, color = Beep.colors.muted)
                    Text(value, fontSize = 13.sp)
                } }
            }
        }
    }
}

@Composable
internal fun WorkspaceSearch(query: String, placeholder: String, onChange: (String) -> Unit) {
    OutlinedTextField(query, onChange, Modifier.fillMaxWidth(), singleLine = true,
        placeholder = { Text(placeholder, fontSize = 13.sp) }, shape = RoundedCornerShape(6.dp),
        textStyle = LocalTextStyle.current.copy(fontSize = 13.sp))
}

@Composable
internal fun WorkspaceCode(text: String) {
    SelectionContainer { Text(text, Modifier.fillMaxWidth().padding(12.dp),
        fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 20.sp) }
}

@Composable
internal fun WorkspaceProse(text: String) {
    SelectionContainer { Text(text, Modifier.fillMaxWidth().padding(12.dp), fontSize = 13.sp, lineHeight = 20.sp) }
}

@Composable
internal fun WorkspaceError(error: String?, retry: (() -> Unit)? = null) {
    if (error == null) return
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(error, Modifier.weight(1f), color = Beep.colors.error, fontSize = 12.sp)
        if (retry != null) TextButton(onClick = retry) { Text("Retry") }
    }
}

@Composable
internal fun WorkspaceEmpty(text: String) {
    Text(text, Modifier.fillMaxWidth().padding(vertical = 24.dp), fontSize = 13.sp, color = Beep.colors.muted)
}

@Composable
internal fun WorkspaceLoading() {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        repeat(3) { Box(Modifier.fillMaxWidth().height(48.dp).background(Beep.colors.raised, RoundedCornerShape(8.dp))) }
    }
}

@Composable
internal fun WorkspaceTabs(selected: String, tabs: List<String>, onSelect: (String) -> Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
        tabs.forEach { tab ->
            val active = tab == selected
            val line = Beep.colors.accent
            TextButton(onClick = { onSelect(tab) }, modifier = Modifier.height(40.dp).semantics { this.selected = active }.drawWithContent {
                drawContent()
                if (active) drawLine(line, Offset(0f, size.height), Offset(size.width, size.height), strokeWidth = 2.dp.toPx())
            }) {
                Text(tab, fontSize = 12.sp, color = if (active) Beep.colors.ink else Beep.colors.muted,
                    fontWeight = if (active) FontWeight.SemiBold else FontWeight.Normal)
            }
        }
    }
    HorizontalDivider(color = Beep.colors.softLine)
}

@Composable
internal fun WorkspaceChoice(label: String, value: String, options: List<String>, onChoose: (Int) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Column {
        Text(label, fontSize = 12.sp, color = Beep.colors.muted)
        Box {
            OutlinedButton(onClick = { expanded = true }, enabled = options.isNotEmpty(),
                modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(6.dp)) {
                Text(value.ifEmpty { "Choose" }, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                options.forEachIndexed { index, name -> DropdownMenuItem(text = { Text(name) }, onClick = { expanded = false; onChoose(index) }) }
            }
        }
    }
}

internal fun Enum<*>.workspaceLabel(): String = name.lowercase().replace('_', ' ').replaceFirstChar { it.uppercase() }
