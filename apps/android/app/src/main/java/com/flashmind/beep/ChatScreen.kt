package com.flashmind.beep

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import com.flashmind.beep.proto.Models
import com.flashmind.beep.proto.ReasoningLevel
import androidx.compose.material.icons.outlined.ArrowDropDown
import androidx.compose.material.icons.outlined.Refresh
import kotlin.math.roundToInt
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.draw.alpha
import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.os.SystemClock
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.animation.*
import androidx.compose.animation.core.EaseInOutCubic
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.scrollBy
import kotlin.reflect.KProperty
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.Image
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalDate

// Short names for the current theme's colors.
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val Coral: Color @Composable @ReadOnlyComposable get() = Beep.colors.error
private val TextPrimary: Color @Composable @ReadOnlyComposable get() = Beep.colors.ink
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

/** Mirrors the server's MEMORY_THRESHOLD: above it the learner reads the turn. */
private const val MEMORY_THRESHOLD = 0.8
private val MEMORY_TOOLS = setOf("memory_store", "memory_update", "memory_forget")

/** How many tool chips a message shows before folding the earlier ones away. */
private const val SHOWN_TOOLS = 3

/** Reads and writes one part of a chat's draft, which the repository holds so every chat window sees the same one. */
private class DraftField<T>(private val read: () -> T, private val write: (T) -> Unit) {
    operator fun getValue(thisRef: Any?, property: KProperty<*>): T = read()
    operator fun setValue(thisRef: Any?, property: KProperty<*>, value: T) = write(value)
}

@Composable
internal fun ChatScreen(chat: Conversation, repository: ChatRepository, modifier: Modifier, compact: Boolean,
                       focusInput: MutableState<Boolean> = mutableStateOf(false),
                       clipboardContent: MutableState<String?> = mutableStateOf(null),
                       onExpand: (() -> Unit)? = null) {
    var draft by DraftField({ repository.draft(chat.id).text }) { repository.setDraft(chat.id, repository.draft(chat.id).copy(text = it)) }
    var attachedImages by DraftField({ repository.draft(chat.id).images }) { repository.setDraft(chat.id, repository.draft(chat.id).copy(images = it)) }
    // The composer's text with its cursor. When the draft changes from outside the field, such as a quote or
    // dictation, the cursor goes to `draftCursor` if one was set, or else to the end.
    var draftField by remember(chat.id) { mutableStateOf(TextFieldValue(draft, TextRange(draft.length))) }
    var draftCursor by remember(chat.id) { mutableStateOf<Int?>(null) }
    if (draftField.text != draft) {
        draftField = TextFieldValue(draft, TextRange((draftCursor ?: draft.length).coerceIn(0, draft.length)))
        draftCursor = null
    }
    // Each chat gets its own list that opens at the latest message.
    val openLink = LocalOpenLink.current ?: repository::openBrowser
    val list = remember(chat.id) { LazyListState(chat.messages.lastIndex.coerceAtLeast(0)) }
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    val photoPicker = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(MAX_IMAGES)) { uris ->
        if (uris.isNotEmpty()) {
            scope.launch(Dispatchers.IO) {
                val encoded = uris.mapNotNull { encodeImageUri(context, it) }
                attachedImages = (attachedImages + encoded).take(MAX_IMAGES)
            }
        }
    }
    var attaching by remember { mutableStateOf(false) }
    if (attaching) AttachSheet(room = MAX_IMAGES - attachedImages.size, onDismiss = { attaching = false },
        onAllPhotos = { photoPicker.launch(androidx.activity.result.PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
        onImages = { attachedImages = (attachedImages + it).take(MAX_IMAGES) })
    val inputFocusRequester = remember { FocusRequester() }
    LaunchedEffect(focusInput.value) {
        if (focusInput.value) {
            focusInput.value = false
            inputFocusRequester.requestFocus()
        }
    }
    // Follow the reply until the user drags away from the bottom; reaching the bottom again resumes it.
    // Only a drag counts: our own scrolling while a reply streams in must not stop following.
    var followLatest by remember(chat.id) { mutableStateOf(true) }
    // Admins get a rerun menu under each reply; the list is empty for everyone else.
    val modelChoices by repository.modelChoices.collectAsState()
    LaunchedEffect(Unit) {
        if (modelChoices == null && !compact) runCatching { withContext(Dispatchers.IO) { repository.models() } }
    }
    val dragged by list.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(list, chat.id) {
        snapshotFlow { dragged to list.canScrollForward }.collect { (dragging, canForward) ->
            if (!canForward) followLatest = true else if (dragging) followLatest = false
        }
    }
    // The first scroll of a chat jumps to the end, after the list has laid its messages out;
    // only what arrives later animates, so opening a chat does not scroll down from the top.
    var anchored by remember(chat.id) { mutableStateOf(false) }
    LaunchedEffect(chat.id, chat.messages.size, chat.messages.lastOrNull()?.text?.length, chat.status) {
        if (chat.messages.isEmpty() || !followLatest) return@LaunchedEffect
        list.awaitItems(chat.messages.size)
        list.scrollToEnd(animated = anchored)
        anchored = true
    }
    // The keyboard and a growing input resize the list every frame, so stay at the bottom without animating.
    val latestChat by rememberUpdatedState(chat)
    LaunchedEffect(list, chat.id) {
        snapshotFlow { list.layoutInfo.viewportSize }.drop(1).collect {
            if (followLatest && latestChat.messages.isNotEmpty()) list.scrollToEnd(animated = false)
        }
    }
    // Auto-dismiss clipboard banner after 10 seconds
    val clipText = clipboardContent.value
    LaunchedEffect(clipText) {
        if (clipText != null) { delay(10_000); clipboardContent.value = null }
    }
    // Dismiss clipboard banner when user starts typing
    LaunchedEffect(draft) { if (draft.isNotBlank()) clipboardContent.value = null }
    fun submit() {
        val hasContent = draft.isNotBlank() || attachedImages.isNotEmpty()
        if (hasContent && !chat.busy) {
            followLatest = true
            val text = draft.ifBlank { imagePrompt(attachedImages.size) }
            repository.send(text, attachedImages)
            draft = ""
            attachedImages = emptyList()
        }
    }
    // While Beep works, its progress sits in the conversation under the last message, even after some
    // reply text is in. Pinned above the composer it left a gap and took height from the messages.
    // The streaming text is its own progress, so "Replying" needs no line of its own.
    val workingInline = chat.busy && chat.status.isNotEmpty() && !chat.status.startsWith("Replying")
    val replyStarted = chat.messages.lastOrNull().let { it != null && it.role == "assistant" && it.text.isNotBlank() }
    Column(modifier.imePadding()) {
        // Clipboard suggestion banner
        ClipboardBanner(clipboardContent, onAction = { action, text ->
            clipboardContent.value = null
            val message = when (action) {
                "ask" -> text
                "translate" -> "Translate this:\n\n$text"
                "summarize" -> "Summarize this:\n\n$text"
                else -> text
            }
            followLatest = true
            repository.send(message)
        }, onDismiss = { clipboardContent.value = null })
        if (chat.messages.isEmpty()) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(if (compact) 20.dp else 24.dp), verticalArrangement = Arrangement.Center) {
                if (compact) {
                    Text(displayText("Hey. What can I help with?"), style = displayStyle(20.sp))
                    Spacer(Modifier.height(18.dp))
                } else {
                    Text(buildAnnotatedString {
                        append(displayText("A little help.\n"))
                        withStyle(SpanStyle(color = Beep.colors.ok)) { append(displayText("Right here.")) }
                    }, style = displayStyle(40.sp, lineHeight = 44.sp), modifier = Modifier.popIn("hello"))
                    Spacer(Modifier.height(12.dp))
                    Text("Find an answer. Untangle a thought.\nKeep something worth remembering.", color = TextMuted, lineHeight = 23.sp)
                    Spacer(Modifier.height(28.dp))
                }
                // Personal starters from the server once they arrive; until then, templates to finish typing.
                val starters by repository.starters.collectAsState()
                LaunchedEffect(chat.id) { repository.loadStarters() }
                val items = if (starters.isNotEmpty()) starters.map { Triple(it.title, it.prompt, true) }
                    else listOf(
                        Triple("Remember something", "Remember that I prefer short, practical answers.", true),
                        Triple("Search the web", "Search the web for ", false),
                        Triple("Set a reminder", "Remind me to ", false),
                    )
                items.forEachIndexed { index, (title, prompt, complete) ->
                    var shown by remember { mutableStateOf(false) }
                    LaunchedEffect(Unit) { delay(120L + index * 90L); shown = true }
                    Box(Modifier.popIn(title to shown, play = shown).alpha(if (shown) 1f else 0f)) {
                        Suggestion(title, prompt, starterIcon(title + " " + prompt), Beep.colors.tints[index % Beep.colors.tints.size]) {
                            if (complete) {
                                followLatest = true; repository.send(it)
                                // The server drops a used starter's set, so the next empty chat gets new ones.
                                repository.dropStarter(it)
                            } else draft = it
                        }
                    }
                    if (index < items.lastIndex) Spacer(Modifier.height(12.dp))
                }
                if (starters.isNotEmpty()) NewIdeas(repository)
            }
        } else {
            // Only messages that arrive while the chat is open pop in; history is drawn at rest.
            val settled = remember(chat.id) { chat.messages.map { it.id }.toHashSet() }
            val radius = Beep.shapes.radius
            val dayBreaks = remember(chat.messages) { dayBreaks(chat.messages) }
            // The "Latest reply" button floats over the list, so showing it does not resize the list.
            Box(Modifier.weight(1f).fillMaxWidth()) {
            LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(if (compact) 12.dp else 18.dp)) {
                itemsIndexed(chat.messages, key = { _, it -> it.id }) { index, message ->
                    val isUser = message.role == "user"
                    // A reply sits close under its own prompt; turns keep the full gap.
                    val gap = when {
                        index == 0 -> 0.dp
                        !isUser && chat.messages[index - 1].role == "user" -> if (compact) 4.dp else 6.dp
                        else -> if (compact) 12.dp else 18.dp
                    }
                    val fresh = message.id !in settled
                    SideEffect { settled.add(message.id) }
                    val newDay = dayBreaks[message.id]
                    if (message.role == "scheduled") {
                        Column(Modifier.padding(top = gap).animateItem(fadeOutSpec = null)) {
                            newDay?.let { DayMarker(it, Modifier.padding(bottom = 12.dp)) }
                            ScheduledMarker(message.text)
                        }
                        return@itemsIndexed
                    }
                    Column(Modifier.padding(top = gap).fillMaxWidth().animateItem(fadeOutSpec = null)
                        .popIn(message.id, play = fresh, origin = TransformOrigin(if (isUser) 1f else 0f, 1f)),
                        horizontalAlignment = if (isUser) Alignment.End else Alignment.Start) {
                        newDay?.let { DayMarker(it, Modifier.padding(bottom = 12.dp)) }
                        val isLastAssistant = !isUser && chat.messages.lastOrNull { it.role == "assistant" }?.id == message.id
                        val bubbleShape = if (isUser) RoundedCornerShape(radius, radius, 6.dp, radius)
                            else RoundedCornerShape(6.dp, radius, radius, radius)
                        val bubbleModifier = when {
                            isUser -> Modifier.padding(start = 48.dp)
                                .then(if (Beep.shapes.card == CardLook.Hairline) Modifier.clip(bubbleShape).background(Beep.colors.user)
                                    else Modifier.beepFrame(bubbleShape, Beep.colors.user))
                            Beep.shapes.botBubble -> Modifier.padding(end = 24.dp).beepFrame(bubbleShape, Beep.colors.surface)
                            else -> Modifier.fillMaxWidth()
                        }
                        // A finished tool that has something to show becomes a card; the rest stay chips.
                        val toolCards = remember(message.toolCalls) { message.toolCalls.map { it to cardFor(it) } }
                        val chips = toolCards.filter { (call, card) -> card == null && call.name !in QUIET_TOOLS }
                        if (!isUser && message.reasoning.isNotBlank()) ReasoningRow(message.reasoning, Modifier.padding(bottom = 4.dp))
                        if (chips.isNotEmpty()) {
                            // While the reply runs only the latest few chips show and the earlier ones fold into one
                            // line; once it is done a long run folds away entirely. The line opens the full list.
                            var allTools by remember(message.id) { mutableStateOf(false) }
                            val finished = !(isLastAssistant && chat.busy) && chips.all { it.first.isDone }
                            val folded = if (finished && chips.size > SHOWN_TOOLS) chips.size
                                else (chips.size - SHOWN_TOOLS).coerceAtLeast(0)
                            val hidden = if (allTools) 0 else folded
                            Column(Modifier.padding(bottom = 8.dp)) {
                                AnimatedVisibility(folded > 0, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
                                    FoldedTools(chips.take(folded).map { it.first }, finished = folded == chips.size,
                                        open = allTools, onToggle = { allTools = !allTools })
                                }
                                chips.forEachIndexed { i, (tc, _) ->
                                    key(tc.callId.ifEmpty { "$i" }) {
                                        AnimatedVisibility(i >= hidden, enter = expandVertically() + fadeIn(),
                                            exit = shrinkVertically() + fadeOut()) {
                                            ToolChip(tc, Beep.colors.tints[i % Beep.colors.tints.size], Modifier.padding(top = 6.dp), onOpen = openLink)
                                        }
                                    }
                                }
                            }
                        }
                        message.approvals.forEach { prompt ->
                            key(prompt.queryId) {
                                ApprovalCard(prompt, onAnswer = { repository.answerApproval(chat.id, prompt.queryId, it) },
                                    Modifier.padding(bottom = 8.dp))
                            }
                        }
                        val isStreaming = isLastAssistant && chat.busy
                        val textLen = message.text.length
                        val displayedLen by animateIntAsState(textLen, tween(if (isStreaming) 150 else 0), label = "stream-text")
                        val shownText = if (isStreaming && displayedLen < textLen) message.text.substring(0, displayedLen) else message.text
                        var menuOpen by remember(message.id) { mutableStateOf(false) }
                        // The quote goes above whatever is already typed.
                        val quoteText: (String) -> Unit = { text ->
                            val quoted = text.trim().lines().joinToString("\n") { "> $it" } + "\n\n"
                            draftCursor = quoted.length
                            draft = quoted + draft.trimStart()
                            inputFocusRequester.requestFocus()
                        }
                        val quote: () -> Unit = { quoteText(message.text.trim().lines().take(4).joinToString("\n")) }
                        if (shownText.isNotBlank() || message.images.isNotEmpty() || message.pictures.isNotEmpty()) SelectableMessage(quoteText) { picker -> Box {
                        // Long press and double tap belong to text selection, so a tap opens the menu (copy, quote, share).
                        Column(bubbleModifier.then(picker.tracking).combinedClickable(enabled = !isStreaming, onClickLabel = "Message actions",
                            indication = null, interactionSource = remember { MutableInteractionSource() },
                            onDoubleClick = picker::selectWord) { if (!picker.picking) menuOpen = true }) {
                            // One attached image fills the bubble; several sit in a row of thumbnails.
                            if (message.images.size == 1) {
                                val msgBitmap = remember(message.images[0]) { decodeImageBase64(message.images[0]) }
                                if (msgBitmap != null) {
                                    Image(msgBitmap.asImageBitmap(), "Attached image",
                                        contentScale = ContentScale.FillWidth,
                                        modifier = Modifier.fillMaxWidth().heightIn(max = 200.dp))
                                }
                            } else if (message.images.size > 1) {
                                Row(Modifier.horizontalScroll(rememberScrollState()).padding(6.dp),
                                    horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    message.images.forEachIndexed { index, img ->
                                        val msgBitmap = remember(img) { decodeImageBase64(img) }
                                        if (msgBitmap != null) {
                                            Image(msgBitmap.asImageBitmap(), "Attached image ${index + 1}",
                                                contentScale = ContentScale.Crop,
                                                modifier = Modifier.size(110.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius)))
                                        }
                                    }
                                }
                            }
                            // Pictures the server kept, for a chat reloaded or sent from another device.
                            if (message.images.isEmpty()) SentPictureRow(message.pictures)
                            if (shownText.isNotBlank()) MessageContent(shownText, message.role, compact,
                                padded = isUser || Beep.shapes.botBubble)
                        }
                        MessageMenu(menuOpen, onDismiss = { menuOpen = false }, text = message.text, onQuote = quote,
                            onDelete = if (message.role == "user" && !chat.busy) {
                                { scope.launch { runCatching { repository.deleteTurn(message, chat.id) }.onFailure { showError(context, it) } } }
                            } else null)
                        } }
                        if (isUser && message.model.isNotEmpty()) {
                            val name = modelChoices?.choices?.find { it.id == message.model }?.name ?: message.model
                            val level = message.modelThinking.takeIf { it.isNotEmpty() }?.let { " · $it" }.orEmpty()
                            Text("Rerun on $name$level", fontSize = 11.sp, color = TextMuted, modifier = Modifier.padding(top = 4.dp, end = 4.dp))
                        }
                        // The model and thinking Jev picked for this prompt under Auto. Everyone but
                        // an admin is on Auto for thinking only, so they see just the level.
                        if (isUser && message.autoModel.isNotEmpty()) {
                            val choices = modelChoices?.choices.orEmpty()
                            val line = if (choices.isEmpty()) message.autoThinking else {
                                val name = choices.find { it.id == message.autoModel }?.name
                                    ?: message.autoModel.substringAfterLast('/')
                                "$name · ${message.autoThinking}"
                            }
                            Text(line, fontSize = 11.sp, color = TextDim,
                                modifier = Modifier.padding(top = 4.dp, end = 4.dp))
                        }
                        if (!isStreaming) ToolCards(toolCards.mapNotNull { it.second }, compact, repository, openLink)
                        if (!isUser && !isStreaming && message.text.isNotBlank()) {
                            val shownLinks = toolCards.mapNotNull { (it.second as? ToolCard.Link)?.url }.toSet()
                            val links = remember(message.text) { linksIn(message.text) }.filter { it !in shownLinks }
                            if (links.isNotEmpty()) Column(Modifier.padding(top = 8.dp, end = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                links.forEach { LinkPreview(it, openLink) }
                            }
                        }
                        if (message.cards.isNotEmpty()) {
                            Spacer(Modifier.height(8.dp))
                            Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth().animateContentSize()) {
                                message.cards.forEach { card -> CardView(card, repository, compact) }
                            }
                        }
                        if (!message.taskSteps.isNullOrEmpty()) {
                            Spacer(Modifier.height(8.dp))
                            TaskProgressCard(message.taskSteps)
                        }
                        if (message.role == "assistant" && message.text.isNotBlank() && !chat.busy && !compact) Row(verticalAlignment = Alignment.CenterVertically) {
                            CopyReply(message.text, compact)
                            TextButton(onClick = quote, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
                                Icon(Icons.Outlined.FormatQuote, null, Modifier.size(14.dp), tint = TextMuted)
                                Spacer(Modifier.width(6.dp)); Text("Quote", fontSize = 11.sp, color = TextMuted)
                            }
                            modelChoices?.takeIf { it.choices.isNotEmpty() }?.let { models ->
                                // The server names the model for turns it recorded. A reply from this
                                // session ran on its prompt's model, or the user's own when it had none.
                                val used = message.model.ifEmpty {
                                    val prompt = chat.messages.take(index).lastOrNull { it.role == "user" }
                                    prompt?.model?.ifEmpty { models.selected.takeUnless { message.id.startsWith("history-") } }
                                }
                                RerunButton(models, used) { model, thinking ->
                                    followLatest = true
                                    scope.launch { runCatching { repository.rerun(message, model, thinking) }.onFailure { showError(context, it) } }
                                }
                            }
                            // The specialized agent that answered, for admins next to the memory score.
                            if (modelChoices?.choices?.isNotEmpty() == true && message.agent.isNotEmpty())
                                Text(message.agent, fontSize = 11.sp, color = TextDim, modifier = Modifier.padding(start = 8.dp))
                            // Jev's estimate that this turn held something worth remembering.
                            message.memoryScore?.let { score ->
                                val saved = message.toolCalls.any { it.name in MEMORY_TOOLS }
                                val note = if (score >= MEMORY_THRESHOLD && !saved) " · nothing new" else ""
                                Text("${(score * 100).roundToInt()}%$note", fontSize = 11.sp, color = TextDim,
                                    modifier = Modifier.padding(start = 8.dp))
                            }
                        }
                        if (isLastAssistant && !chat.busy && chat.messages.last().id == message.id) {
                            toolCards.firstNotNullOfOrNull { it.second as? ToolCard.Replies }?.let { replies ->
                                Spacer(Modifier.height(4.dp))
                                SuggestedReplies(replies) { followLatest = true; repository.send(it) }
                            }
                        }
                    }
                }
                if (workingInline) item(key = "working") {
                    Column(Modifier.padding(top = if (compact) 4.dp else 6.dp).fillMaxWidth().animateItem(fadeOutSpec = null).popIn("working-${chat.id}", origin = TransformOrigin(0f, 1f))) {
                        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 4.dp)
                            .animateContentSize(spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium))) {
                            PulsingDots()
                            Spacer(Modifier.width(10.dp))
                            StatusText(chat.status)
                        }
                        ThinkingPreview(chat.thinking, Modifier.padding(end = if (compact) 0.dp else 48.dp))
                        if (!compact && !replyStarted) ReplySkeleton(Modifier.padding(top = 12.dp, end = 48.dp))
                    }
                }
            }
            if (!compact) PromptMap(chat.messages, list, Modifier.matchParentSize()) { index ->
                followLatest = false
                scope.launch { list.animateScrollToItem(index) }
            }
            androidx.compose.animation.AnimatedVisibility(visible = !followLatest && list.canScrollForward,
                enter = fadeIn() + slideInVertically { it / 2 }, exit = fadeOut() + slideOutVertically { it / 2 },
                modifier = Modifier.align(Alignment.BottomCenter)) {
                ChunkyButton(onClick = { followLatest = true; scope.launch { list.scrollToEnd(animated = true) } },
                    modifier = Modifier.padding(vertical = 8.dp), contentPadding = PaddingValues(horizontal = 14.dp, vertical = 7.dp)) {
                    Icon(Icons.Outlined.ArrowDownward, null, Modifier.size(16.dp)); Text("Latest reply", fontSize = 12.sp, fontWeight = FontWeight.Medium)
                }
            }
            }
        }
        // A status left once the run is over, such as "Stopping…".
        AnimatedVisibility(visible = chat.status.isNotEmpty() && !chat.busy && !chat.status.startsWith("Replying"),
            enter = fadeIn(tween(200)) + expandVertically(tween(200)), exit = fadeOut(tween(150)) + shrinkVertically(tween(150))) {
            Column(Modifier.padding(horizontal = 20.dp, vertical = 6.dp)) {
                Row(Modifier.animateContentSize(spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium)),
                    verticalAlignment = Alignment.CenterVertically) {
                    PulsingDots()
                    Spacer(Modifier.width(10.dp))
                    StatusText(chat.status)
                }
                ThinkingPreview(chat.thinking)
            }
        }
        AnimatedVisibility(visible = chat.error.isNotEmpty(),
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 4.dp).fillMaxWidth()
                .clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(Beep.colors.error.copy(alpha = 0.10f))
                .padding(start = 14.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(chat.error, fontSize = 13.sp, color = Beep.colors.error, modifier = Modifier.weight(1f).padding(vertical = 8.dp), maxLines = 4)
                // A stopped turn may have no reply to carry the rerun menu, so it sits here too.
                val prompt = chat.messages.lastOrNull { it.role == "user" }
                val models = modelChoices?.takeIf { it.choices.isNotEmpty() }
                if (!chat.busy && !compact && prompt != null && models != null) {
                    RerunButton(models, prompt.model.ifEmpty { null }, Beep.colors.error, prominent = true) { model, thinking ->
                        followLatest = true
                        scope.launch { runCatching { repository.rerunPrompt(prompt, model, thinking) }.onFailure { showError(context, it) } }
                    }
                }
                if (chat.busy || prompt != null) TextButton(onClick = repository::retryLast) { Text("Retry", color = Beep.colors.error, fontWeight = FontWeight.SemiBold) }
            }
        }
        // Voice input state
        var listening by remember { mutableStateOf(false) }
        val speechAvailable = remember { SpeechRecognizer.isRecognitionAvailable(context) }
        val recognizer = remember { if (speechAvailable) SpeechRecognizer.createSpeechRecognizer(context) else null }
        fun listen(speech: SpeechRecognizer) {
            listening = true
            startListening(speech, onPartial = { partial -> draft = partial },
                onResult = { result -> draft = result; listening = false },
                onError = { listening = false })
        }
        val micPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted && recognizer != null) listen(recognizer)
        }
        DisposableEffect(recognizer) { onDispose { recognizer?.destroy() } }

        fun startVoice() {
            if (!speechAvailable) return
            if (listening) { recognizer?.stopListening(); listening = false; return }
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                listen(recognizer!!)
            } else {
                micPermission.launch(Manifest.permission.RECORD_AUDIO)
            }
        }

        // "Listening..." indicator
        AnimatedVisibility(visible = listening,
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = if (compact) 12.dp else 20.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                val pulseAlpha by rememberInfiniteTransition(label = "listen-pulse").animateFloat(
                    initialValue = 0.4f, targetValue = 1f,
                    animationSpec = infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "listen-alpha")
                Box(Modifier.size(7.dp).background(Beep.colors.accent.copy(alpha = pulseAlpha), CircleShape))
                Spacer(Modifier.width(8.dp))
                Text("Listening...", fontSize = 12.sp, color = TextMuted, fontWeight = FontWeight.Medium)
            }
        }

        // Image attachment previews
        AnimatedVisibility(visible = attachedImages.isNotEmpty(),
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.horizontalScroll(rememberScrollState())
                .padding(horizontal = if (compact) 12.dp else 18.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                attachedImages.forEachIndexed { index, img ->
                    val previewBitmap = remember(img) { decodeImageBase64(img) }
                    if (previewBitmap != null) {
                        Box {
                            Box(Modifier.size(72.dp).popIn(img).beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface, pressableLook())) {
                                Image(previewBitmap.asImageBitmap(), "Attached image ${index + 1}",
                                    contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                            }
                            ChunkyButton(onClick = { attachedImages = attachedImages - img }, color = Beep.colors.ink, contentColor = Beep.colors.page,
                                modifier = Modifier.size(24.dp).align(Alignment.TopEnd).offset(x = 6.dp, y = (-6).dp),
                                contentPadding = PaddingValues(0.dp)) {
                                Icon(Icons.Outlined.Close, "Remove image ${index + 1}", Modifier.size(14.dp))
                            }
                        }
                    }
                }
            }
        }

        // A message queued while the reply runs; it sends on its own when the reply finishes.
        AnimatedVisibility(visible = chat.hasQueued,
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = if (compact) 10.dp else 14.dp).padding(top = 6.dp).fillMaxWidth()
                .beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface, pressableLook())
                .padding(start = 12.dp, top = 4.dp, bottom = 4.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(Icons.Outlined.Schedule, null, Modifier.size(16.dp), tint = TextMuted)
                Column(Modifier.weight(1f)) {
                    Text(if (chat.busy) "Sends when Beep finishes" else "Not sent", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)
                    Text(chat.queuedText.ifBlank { "${chat.queuedImages.size} image" + if (chat.queuedImages.size > 1) "s" else "" },
                        fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                if (!chat.busy) IconButton(onClick = { followLatest = true; repository.sendQueued(chat.id) }, Modifier.size(36.dp)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send queued message", Modifier.size(18.dp), tint = Beep.colors.accent)
                }
                IconButton(onClick = {
                    val (text, images) = repository.unqueue(chat.id)
                    draft = listOf(text, draft).filter { it.isNotBlank() }.joinToString("\n\n")
                    attachedImages = (images + attachedImages).take(MAX_IMAGES)
                }, Modifier.size(36.dp)) {
                    Icon(Icons.Outlined.Close, "Edit queued message", Modifier.size(16.dp), tint = TextMuted)
                }
            }
        }

        val buttonSize = if (compact) 44.dp else 50.dp
        val buttonShape = RoundedCornerShape(Beep.shapes.radius)
        Row(Modifier.padding(start = if (onExpand != null) 2.dp else if (compact) 8.dp else 14.dp, end = if (compact) 10.dp else 16.dp,
            top = if (compact) 4.dp else 8.dp, bottom = if (compact) 8.dp else 12.dp).fillMaxWidth(),
            verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(if (onExpand != null) 2.dp else 8.dp)) {
            if (onExpand != null) {
                IconButton(onClick = onExpand, modifier = Modifier.size(buttonSize)) {
                    Icon(Icons.Outlined.OpenInFull, "Open full app", Modifier.size(18.dp), tint = TextMuted)
                }
            }
            Row(Modifier.weight(1f).heightIn(min = buttonSize).beepFrame(buttonShape, Surface, pressableLook()),
                verticalAlignment = Alignment.CenterVertically) {
                if (!chat.busy && draft.isBlank() && attachedImages.size < MAX_IMAGES) {
                    IconButton(onClick = { attaching = true }, modifier = Modifier.padding(start = 2.dp)) {
                        Icon(Icons.Outlined.Image, "Add a photo", Modifier.size(22.dp), tint = TextMuted)
                    }
                }
                TextField(value = draftField, onValueChange = { if (it.text.toByteArray().size <= 32768) { draftField = it; draft = it.text } },
                    modifier = Modifier.weight(1f).focusRequester(inputFocusRequester),
                    placeholder = { Text(if (listening) "Speak now..." else if (attachedImages.size > 1) "Ask about the images..." else if (attachedImages.isNotEmpty()) "Describe the image..." else "Ask anything...", fontSize = 15.sp, color = TextDim) },
                    textStyle = LocalTextStyle.current.copy(fontSize = 15.sp),
                    colors = TextFieldDefaults.colors(unfocusedContainerColor = Color.Transparent, focusedContainerColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent, focusedIndicatorColor = Color.Transparent, cursorColor = TextPrimary),
                    maxLines = if (compact) 3 else 6,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Default))
            }
            val buttonModifier = Modifier.size(buttonSize)
            when {
                chat.busy -> Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (draft.isNotBlank() || attachedImages.isNotEmpty()) {
                        ChunkyButton(onClick = {
                            repository.queue(draft, attachedImages, chat.id)
                            draft = ""; attachedImages = emptyList()
                        }, modifier = buttonModifier, shape = buttonShape,
                            color = Surface, contentColor = TextPrimary, contentPadding = PaddingValues(0.dp)) {
                            Icon(Icons.Outlined.Schedule, "Queue message", Modifier.size(22.dp))
                        }
                    }
                    ChunkyButton(onClick = repository::cancel, modifier = buttonModifier, shape = buttonShape,
                        color = Beep.colors.ink, contentColor = Beep.colors.page, contentPadding = PaddingValues(0.dp)) {
                        Icon(Icons.Outlined.Stop, "Stop reply", Modifier.size(22.dp))
                    }
                }
                draft.isNotBlank() || attachedImages.isNotEmpty() -> {
                    // A quick squish on tap so sending feels physical.
                    val squish = remember { Animatable(1f) }
                    val noMotion = reduceMotion()
                    ChunkyButton(onClick = {
                        submit()
                        if (!noMotion) scope.launch {
                            squish.animateTo(0.82f, tween(70)); squish.animateTo(1f, spring(dampingRatio = 0.4f, stiffness = 600f))
                        }
                    }, modifier = buttonModifier.graphicsLayer { scaleX = squish.value; scaleY = squish.value }, shape = buttonShape,
                        color = Beep.colors.accent, contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(0.dp)) {
                        Icon(Icons.AutoMirrored.Outlined.Send, "Send message", Modifier.size(20.dp))
                    }
                }
                speechAvailable -> {
                    val ring by rememberInfiniteTransition(label = "mic-ring").animateFloat(
                        initialValue = 0f, targetValue = 1f,
                        animationSpec = infiniteRepeatable(tween(1100, easing = LinearOutSlowInEasing)), label = "mic-ring-progress")
                    val ringColor = Beep.colors.accent
                    ChunkyButton(onClick = { startVoice() },
                        modifier = buttonModifier.drawBehind {
                            if (listening) drawCircle(ringColor.copy(alpha = 0.45f * (1 - ring)), radius = size.minDimension * (0.5f + 0.35f * ring))
                        }, shape = buttonShape,
                        color = if (listening) Beep.colors.accent else Surface,
                        contentColor = if (listening) Beep.colors.onAccent else TextPrimary, contentPadding = PaddingValues(0.dp)) {
                        Icon(Icons.Outlined.Mic, if (listening) "Stop listening" else "Voice input", Modifier.size(22.dp))
                    }
                }
                else -> ChunkyButton(onClick = {}, enabled = false, modifier = buttonModifier, shape = buttonShape, contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send message", Modifier.size(20.dp))
                }
            }
        }

    }
}

private fun startListening(
    recognizer: SpeechRecognizer,
    onPartial: (String) -> Unit,
    onResult: (String) -> Unit,
    onError: (Int) -> Unit
) {
    recognizer.setRecognitionListener(object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onRmsChanged(rmsdB: Float) {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEndOfSpeech() {}
        override fun onError(error: Int) { onError(error) }
        override fun onResults(results: Bundle?) {
            val text = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: ""
            if (text.isNotEmpty()) onResult(text)
            else onError(SpeechRecognizer.ERROR_NO_MATCH)
        }
        override fun onPartialResults(partial: Bundle?) {
            val text = partial?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: ""
            if (text.isNotEmpty()) onPartial(text)
        }
        override fun onEvent(eventType: Int, params: Bundle?) {}
    })
    val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
        putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
        putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
    }
    recognizer.startListening(intent)
}

@Composable
private fun ClipboardBanner(clipboardContent: MutableState<String?>, onAction: (String, String) -> Unit, onDismiss: () -> Unit) {
    val clip = clipboardContent.value
    AnimatedVisibility(
        visible = clip != null,
        enter = fadeIn(tween(200)) + slideInVertically(tween(250)) { -it },
        exit = fadeOut(tween(150)) + slideOutVertically(tween(200)) { -it }
    ) {
        val text = clip ?: return@AnimatedVisibility
        val preview = if (text.length > 40) text.take(40).trim() + "..." else text.trim()
        BeepCard(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp)) {
            Column(Modifier.padding(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.ContentPaste, null, Modifier.size(14.dp), tint = Moss)
                    Spacer(Modifier.width(6.dp))
                    Text(
                        preview.replace('\n', ' '),
                        fontSize = 12.sp, color = TextMuted, maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                    Spacer(Modifier.width(4.dp))
                    Icon(
                        Icons.Outlined.Close, "Dismiss",
                        modifier = Modifier.size(18.dp).clickable { onDismiss() },
                        tint = TextMuted
                    )
                }
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ClipboardChip("Ask about this") { onAction("ask", text) }
                    ClipboardChip("Translate") { onAction("translate", text) }
                    ClipboardChip("Summarize") { onAction("summarize", text) }
                }
            }
        }
    }
}

@Composable
private fun ClipboardChip(label: String, onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp)) {
        Text(label, fontSize = 12.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun CopyReply(text: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember(text) { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    TextButton(onClick = {
        (context as? MainActivity)?.suppressNextClipboard = true
        context.getSystemService(android.content.ClipboardManager::class.java)
            .setPrimaryClip(android.content.ClipData.newPlainText("Beep reply", text))
        copied = true
    }, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
        Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, if (compact) (if (copied) "Copied" else "Copy reply") else null,
            Modifier.size(14.dp), tint = if (copied) Beep.colors.ok else TextMuted)
        if (!compact) { Spacer(Modifier.width(6.dp)); Text(if (copied) "Copied" else "Copy reply", fontSize = 11.sp, color = TextMuted) }
    }
}

/** Grey lines where the reply will be written, with a soft sheen passing over them. */
@Composable
private fun ReplySkeleton(modifier: Modifier = Modifier) {
    val base = Beep.colors.raised
    val sheen = Beep.colors.surface
    val noMotion = reduceMotion()
    val shift by rememberInfiniteTransition(label = "skeleton").animateFloat(0f, 1f,
        infiniteRepeatable(tween(1300, easing = LinearEasing)), label = "skeleton-shift")
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(9.dp)) {
        listOf(1f, 0.86f, 0.55f).forEach { width ->
            Box(Modifier.fillMaxWidth(width).height(12.dp).clip(CircleShape).drawBehind {
                if (noMotion) { drawRect(base); return@drawBehind }
                val x = size.width * (shift * 2f - 0.5f)
                drawRect(Brush.linearGradient(listOf(base, sheen, base), start = Offset(x - size.width * 0.4f, 0f),
                    end = Offset(x + size.width * 0.4f, 0f)))
            })
        }
    }
}

/**
 * Admin only: runs the prompt behind a reply again on a model picked from the menu. Holding a
 * model offers a thinking level for that one rerun; [onPick] gets it lowercase, or "" for a tap.
 */
@Composable
private fun RerunButton(models: Models, used: String?, color: Color = TextMuted, prominent: Boolean = false, onPick: (String, String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    var held by remember { mutableStateOf<String?>(null) }
    val haptics = LocalHapticFeedback.current
    Box {
        TextButton(onClick = { held = null; open = true }, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
            if (prominent) {
                Text("Rerun", color = color, fontWeight = FontWeight.SemiBold)
            } else {
                Icon(Icons.Outlined.Refresh, null, Modifier.size(14.dp), tint = color)
                Spacer(Modifier.width(6.dp)); Text("Rerun", fontSize = 11.sp, color = color)
            }
            Icon(Icons.Outlined.ArrowDropDown, null, Modifier.size(if (prominent) 18.dp else 14.dp), tint = color)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = Surface, shape = RoundedCornerShape(Beep.shapes.smallRadius)) {
            models.choices.forEach { choice ->
                val isUsed = choice.id == used
                val note = when {
                    isUsed -> "Used for this reply"
                    choice.id == models.selected -> "Your model"
                    choice.default -> "Default"
                    else -> choice.id
                }
                // DropdownMenuItem has no long press, so the row is built by hand to the same size.
                Row(Modifier.widthIn(min = 210.dp)
                    .then(if (isUsed) Modifier.background(Moss.copy(alpha = 0.12f)) else Modifier)
                    .combinedClickable(onClickLabel = "Rerun on ${choice.name}", onLongClickLabel = "Pick thinking",
                        onLongClick = { haptics.performHapticFeedback(HapticFeedbackType.LongPress); held = choice.id.takeIf { it != held } },
                        onClick = { open = false; onPick(choice.id, "") })
                    .heightIn(min = 48.dp).padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(choice.name, fontSize = 14.sp, color = if (isUsed) Moss else TextPrimary,
                            fontWeight = if (isUsed) FontWeight.SemiBold else null)
                        Text(note, fontSize = 11.sp, color = if (isUsed) Moss else TextMuted)
                    }
                    if (isUsed) Icon(Icons.Outlined.Check, null, Modifier.padding(start = 8.dp).size(16.dp), tint = Moss)
                }
                if (held == choice.id) Column(Modifier.padding(start = 12.dp, end = 12.dp, bottom = 8.dp)) {
                    Text("Thinking for this rerun", fontSize = 11.sp, color = TextMuted)
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        ReasoningLevel.entries.forEach { level ->
                            val name = level.name.lowercase()
                            FilterChip(selected = false, onClick = { open = false; onPick(choice.id, name) },
                                label = { Text(name.replaceFirstChar { it.uppercase() }, fontSize = 12.sp) })
                        }
                    }
                }
            }
        }
    }
}

/** Tap actions on a message. */
@Composable
internal fun MessageMenu(open: Boolean, onDismiss: () -> Unit, text: String, onQuote: () -> Unit, onDelete: (() -> Unit)? = null) {
    val context = androidx.compose.ui.platform.LocalContext.current
    DropdownMenu(expanded = open, onDismissRequest = onDismiss, containerColor = Surface, shape = RoundedCornerShape(Beep.shapes.smallRadius)) {
        DropdownMenuItem(text = { Text("Copy") }, leadingIcon = { Icon(Icons.Outlined.ContentCopy, null, Modifier.size(18.dp)) }, onClick = {
            (context as? MainActivity)?.suppressNextClipboard = true
            context.getSystemService(android.content.ClipboardManager::class.java)
                .setPrimaryClip(android.content.ClipData.newPlainText("Message", text))
            onDismiss()
        })
        DropdownMenuItem(text = { Text("Quote in reply") }, leadingIcon = { Icon(Icons.Outlined.FormatQuote, null, Modifier.size(18.dp)) },
            onClick = { onQuote(); onDismiss() })
        DropdownMenuItem(text = { Text("Share") }, leadingIcon = { Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)) }, onClick = {
            context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null))
            onDismiss()
        })
        // Takes the reply and its tool calls with it.
        if (onDelete != null) DropdownMenuItem(text = { Text("Delete", color = Coral) },
            leadingIcon = { Icon(Icons.Outlined.Delete, null, Modifier.size(18.dp), tint = Coral) },
            onClick = { onDismiss(); onDelete() })
    }
}

private fun showError(context: android.content.Context, error: Throwable) =
    android.widget.Toast.makeText(context, error.message ?: "Something went wrong", android.widget.Toast.LENGTH_SHORT).show()

/** Where a new day starts in the chat. */
@Composable
private fun DayMarker(day: LocalDate, modifier: Modifier = Modifier) {
    Text(dayLabel(day), modifier.fillMaxWidth(), fontSize = 11.sp,
        color = TextDim, textAlign = TextAlign.Center)
}

/** Where a routine woke Beep: its prompt is the routine's, not something the user said. */
@Composable
private fun ScheduledMarker(label: String, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Outlined.Schedule, null, Modifier.size(13.dp), tint = TextDim)
        Spacer(Modifier.width(6.dp))
        Text(if (label.isBlank()) "Scheduled" else "Scheduled · $label", fontSize = 11.sp, color = TextDim,
            maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Wait until the list has laid out at least [count] items, so a scroll right after they arrive lands. */
internal suspend fun LazyListState.awaitItems(count: Int) {
    snapshotFlow { layoutInfo.totalItemsCount }.first { it >= count }
}

/**
 * Scroll to the very bottom, including the tail of a last item taller than the screen.
 * When the last item is already on screen it only scrolls by what is left of it, since
 * scrolling to the item first would line up its top and bounce a long reply up then down.
 */
internal suspend fun LazyListState.scrollToEnd(animated: Boolean) {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    if (layoutInfo.visibleItemsInfo.lastOrNull()?.index != last) scrollToItem(last)
    val item = layoutInfo.visibleItemsInfo.lastOrNull() ?: return
    val overflow = item.offset + item.size - (layoutInfo.viewportEndOffset - layoutInfo.afterContentPadding)
    if (overflow > 0) { if (animated) animateScrollBy(overflow.toFloat()) else scrollBy(overflow.toFloat()) }
}

@Composable
internal fun PulsingDots() {
    val noMotion = reduceMotion()
    val dot = Beep.colors.accent
    if (noMotion) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            repeat(3) { Box(Modifier.size(6.dp).background(dot, CircleShape)) }
        }
        return
    }
    val infiniteTransition = rememberInfiniteTransition(label = "dots")
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        repeat(3) { index ->
            val lift by infiniteTransition.animateFloat(
                initialValue = 0f, targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = tween(380, easing = EaseInOutCubic, delayMillis = index * 120),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-lift-$index"
            )
            Box(Modifier.size(6.dp).graphicsLayer { translationY = -4.dp.toPx() * lift }
                .background(dot.copy(alpha = 0.45f + 0.55f * lift), CircleShape))
        }
    }
}

/** An icon that hints at what a starter is about, picked from its words. */
private fun starterIcon(text: String): androidx.compose.ui.graphics.vector.ImageVector {
    val t = text.lowercase()
    fun has(vararg words: String) = words.any { it in t }
    return when {
        has("remind", "alarm", "timer") -> Icons.Outlined.Alarm
        has("remember", "note", "save") -> Icons.Outlined.BookmarkBorder
        has("mail", "inbox") -> Icons.Outlined.Email
        has("calendar", "meeting", "schedule", "week", "today", "tomorrow") -> Icons.Outlined.CalendarMonth
        has("map", "near", "place", "restaurant", "route", "where") -> Icons.Outlined.Place
        has("weather", "rain", "forecast") -> Icons.Outlined.WbSunny
        has("search", "news", "latest", "find", "look up") -> Icons.Outlined.Search
        has("plan", "idea", "brainstorm", "write", "draft") -> Icons.Outlined.Lightbulb
        else -> Icons.Outlined.AutoAwesome
    }
}

/** Tap sends the starter; press and hold opens it to show the full prompt first. */
@Composable
private fun Suggestion(title: String, prompt: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tint: Color, onClick: (String) -> Unit) {
    var open by remember(prompt) { mutableStateOf(false) }
    ChunkyButton(onClick = { onClick(prompt) }, onLongClick = { open = !open },
        modifier = Modifier.fillMaxWidth().padding(end = 3.dp, bottom = 3.dp),
        shape = RoundedCornerShape(Beep.shapes.radius), contentPadding = PaddingValues(12.dp)) {
        Box(Modifier.align(Alignment.Top).size(34.dp).background(tint, RoundedCornerShape(Beep.shapes.smallRadius)), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = TextPrimary, modifier = Modifier.size(18.dp))
        }
        Spacer(Modifier.width(12.dp))
        // Only the text grows, so the size animation's clip never cuts the card's border or ledge.
        Column(Modifier.weight(1f).heightIn(min = 34.dp)
            .animateContentSize(spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessMedium)),
            verticalArrangement = Arrangement.Center) {
            Text(title, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = TextPrimary)
            if (open) Text(prompt, fontSize = 13.sp, lineHeight = 19.sp, color = TextMuted, modifier = Modifier.padding(top = 4.dp))
        }
        Icon(Icons.Outlined.NorthEast, null, tint = TextMuted, modifier = Modifier.align(Alignment.Top).padding(top = 9.dp).size(16.dp))
    }
}

/** Asks the server for a new set of starters, at most 3 times in 10 minutes. */
@Composable
private fun NewIdeas(repository: ChatRepository) {
    val refreshes by repository.starterRefreshes.collectAsState()
    val loading by repository.startersRefreshing.collectAsState()
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    val wait = repository.starterRefreshWait(refreshes, now)
    LaunchedEffect(wait > 0) {
        while (repository.starterRefreshWait(refreshes, now) > 0) {
            delay(5_000)
            now = SystemClock.elapsedRealtime()
        }
    }
    val minutes = ((wait + 59_999) / 60_000).coerceAtLeast(1)
    val ready = wait == 0L && !loading
    val turn = if (loading) rememberInfiniteTransition(label = "new-ideas")
        .animateFloat(0f, 360f, infiniteRepeatable(tween(900, easing = LinearEasing)), label = "spin").value else 0f
    Box(Modifier.fillMaxWidth().padding(top = 4.dp), contentAlignment = Alignment.Center) {
        TextButton(
            onClick = { now = SystemClock.elapsedRealtime(); repository.newStarters() },
            enabled = ready,
            contentPadding = PaddingValues(horizontal = 8.dp),
            modifier = Modifier.heightIn(min = 48.dp).alpha(if (wait == 0L) 1f else 0.6f),
        ) {
            Icon(Icons.Outlined.Refresh, null, Modifier.size(16.dp).graphicsLayer { rotationZ = turn }, tint = TextMuted)
            Spacer(Modifier.width(6.dp))
            Text(
                when {
                    loading -> "Finding new ideas"
                    wait == 0L -> "New ideas"
                    else -> "New ideas in $minutes min"
                },
                color = TextMuted, fontSize = 13.sp,
            )
        }
    }
}
