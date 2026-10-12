package com.flashmind.beep

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.BubbleChart
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Laptop
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import com.flashmind.beep.proto.Agents
import com.flashmind.beep.proto.ModelChoice
import com.flashmind.beep.proto.Models
import com.flashmind.beep.proto.ReasoningLevel
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Everything that is not a conversation or one of the drawer tiles. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    repository: ChatRepository,
    onBack: () -> Unit,
    onBubbleSettings: () -> Unit,
    onLinkedDevices: () -> Unit,
) {
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(title = { Text(displayText("Settings"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp)) {
            ThemePicker()
            AvatarPicker()
            BubblePicker(onBubbleSettings)
            // Loaded by AgentsSwitch; the model section words Auto by it.
            var agents by remember { mutableStateOf<Agents?>(null) }
            ModelPicker(repository, agentsOn = agents?.enabled == true)
            AgentsSwitch(repository, agents) { agents = it }
            Column {
                HorizontalDivider(color = Beep.colors.softLine)
                AccountRow(repository)
                HorizontalDivider(color = Beep.colors.softLine)
                SettingsRow(Icons.Outlined.Laptop, "Linked devices", "Use Beep from your computer", onLinkedDevices)
                HorizontalDivider(color = Beep.colors.softLine)
            }
        }
    }
}

/**
 * This phone's account name and whether its key is safe. Without the Google
 * backup the user can save the key as a profile file, and import that file
 * on the first screen of a new phone.
 */
@Composable
private fun AccountRow(repository: ChatRepository) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val name by repository.accountName.collectAsState()
    val backedUp by repository.backedUp.collectAsState()
    var saved by remember { mutableStateOf(false) }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) scope.launch {
            saved = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openOutputStream(uri)?.use { it.write(repository.exportProfile().toByteArray()) } != null
                }.getOrDefault(false)
            }
            Toast.makeText(context, if (saved) "Key saved. Keep the file private." else "Could not save the key", Toast.LENGTH_SHORT).show()
        }
    }
    val detail = when {
        backedUp == true -> "Key backed up to your Google account"
        saved -> "Key saved to a file"
        else -> "Key not backed up · tap to save it to a file"
    }
    SettingsRow(Icons.Outlined.Key, name.ifEmpty { "Account" }, detail) { export.launch("beep-$name.json") }
}

/** Which floating chat the float button opens, and whether the overlay may draw yet. */
@Composable
private fun BubblePicker(onBubbleSettings: () -> Unit) {
    val context = LocalContext.current
    val kind = Bubble.kind.value ?: Bubble.kind(context)
    var allowed by remember { mutableStateOf(Bubble.allowed(context)) }
    LifecycleResumeEffect(Unit) { allowed = Bubble.allowed(context); onPauseOrDispose {} }
    Column {
        Text("Floating chat", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            BubbleKind.entries.forEach { option ->
                val selected = option == kind
                val shape = RoundedCornerShape(14.dp)
                Row(Modifier.fillMaxWidth()
                    .border(if (selected) 2.dp else 1.dp, if (selected) Beep.colors.ink else Beep.colors.softLine, shape)
                    .clip(shape)
                    .clickable { Bubble.pick(context, option) }
                    .padding(horizontal = 12.dp, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(option.label, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                        Text(option.hint, fontSize = 12.sp, color = Beep.colors.muted)
                    }
                    if (selected) Icon(Icons.Outlined.Check, "Selected", Modifier.size(16.dp), tint = Beep.colors.ink)
                }
            }
        }
        if (kind == BubbleKind.Overlay && !allowed) {
            Spacer(Modifier.height(4.dp))
            SettingsRow(Icons.Outlined.BubbleChart, "Allow display over other apps",
                "Until then the float button uses the Android bubble", onBubbleSettings)
        }
    }
}

/**
 * The model and thinking level an admin's own turns run on. The server sends choices only to
 * admins, so everyone else never sees this section. With [agentsOn], the agent Jev picks brings
 * the model, and a failed pick falls back to General, so Auto on leaves nothing to pick here.
 */
@Composable
private fun ModelPicker(repository: ChatRepository, agentsOn: Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var models by remember { mutableStateOf(repository.modelChoices.value) }
    LaunchedEffect(Unit) {
        runCatching { withContext(Dispatchers.IO) { repository.models() } }.onSuccess { models = it }
    }
    val current = models ?: return
    if (current.choices.isEmpty()) return
    fun pick(choice: ModelChoice) {
        if (choice.id == current.selected) return
        models = current.copy(selected = choice.id)
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.setModel(choice.id) } }
                .onSuccess { models = it; Toast.makeText(context, "Model set for your turns", Toast.LENGTH_SHORT).show() }
                .onFailure { models = current; Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
        }
    }
    fun think(level: ReasoningLevel) {
        if (level == current.thinking) return
        models = current.copy(thinking = level)
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.setThinking(level) } }
                .onSuccess { models = it; Toast.makeText(context, "Thinking set for your turns", Toast.LENGTH_SHORT).show() }
                .onFailure { models = current; Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
        }
    }
    fun auto(enabled: Boolean) {
        models = current.copy(auto = enabled)
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.setAutoModel(enabled) } }
                .onSuccess { models = it }
                .onFailure { models = current; Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
        }
    }
    Column {
        Text("Model (admin)", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        if (current.autoAvailable) {
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Auto", fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                    Text(when {
                        current.auto && agentsOn -> "Jev picks an agent, which brings its model and thinking"
                        current.auto -> "Jev picks the model and thinking for each prompt"
                        agentsOn -> "Use the model below, with the agent's prompt"
                        else -> "Off: every prompt uses the model below"
                    }, fontSize = 12.sp, color = Beep.colors.muted)
                }
                Switch(checked = current.auto, onCheckedChange = ::auto,
                    colors = SwitchDefaults.colors(checkedTrackColor = Beep.colors.ok))
            }
            if (current.auto && !agentsOn) Text("Used when Jev cannot pick:", fontSize = 12.sp, color = Beep.colors.muted,
                modifier = Modifier.padding(bottom = 6.dp))
        }
        if (!(current.autoAvailable && current.auto && agentsOn)) Column {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                current.choices.forEach { choice ->
                    val selected = choice.id == current.selected
                    val shape = RoundedCornerShape(14.dp)
                    Row(Modifier.fillMaxWidth()
                        .border(if (selected) 2.dp else 1.dp, if (selected) Beep.colors.ink else Beep.colors.softLine, shape)
                        .clip(shape)
                        .clickable { pick(choice) }
                        .padding(horizontal = 12.dp, vertical = 9.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(if (choice.default) "${choice.name} (default)" else choice.name,
                                fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                            Text(modelDetail(choice), fontSize = 12.sp, color = Beep.colors.muted)
                        }
                        if (selected) Icon(Icons.Outlined.Check, "Selected", Modifier.size(16.dp), tint = Beep.colors.ink)
                    }
                }
            }
            Spacer(Modifier.height(12.dp))
            Text("Thinking", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
            Spacer(Modifier.height(4.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                ReasoningLevel.entries.forEach { level ->
                    FilterChip(selected = level == current.thinking, onClick = { think(level) },
                        label = { Text(level.name.lowercase().replaceFirstChar { it.uppercase() }) })
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        Text(if (agentsOn) "Applies to your own turns." else "Applies to your own turns. Everyone else stays on the default.",
            fontSize = 12.sp, color = Beep.colors.muted)
    }
}

/**
 * The server-wide agents switch, with the agents Jev picks from listed under it. Only admins
 * get an answer from the server, so everyone else never sees this section.
 */
@Composable
private fun AgentsSwitch(repository: ChatRepository, agents: Agents?, setAgents: (Agents) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val models by repository.modelChoices.collectAsState()
    LaunchedEffect(models?.choices?.isNotEmpty()) {
        if (models?.choices?.isNotEmpty() != true) return@LaunchedEffect
        runCatching { withContext(Dispatchers.IO) { repository.agents() } }.onSuccess(setAgents)
    }
    val current = agents ?: return
    if (current.agents.isEmpty()) return
    fun toggle(enabled: Boolean) {
        setAgents(current.copy(enabled = enabled))
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.setAgents(enabled) } }
                .onSuccess {
                    setAgents(it)
                    Toast.makeText(context, if (it.enabled) "Agents on for everyone" else "Agents off", Toast.LENGTH_SHORT).show()
                }
                .onFailure { setAgents(current); Toast.makeText(context, userMessage(it), Toast.LENGTH_SHORT).show() }
        }
    }
    val choices = models?.choices.orEmpty()
    Column {
        Text("Agents (admin)", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Agents", fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                Text(if (current.enabled) "On for everyone: Jev picks an agent for each prompt"
                    else "Off: everyone uses the general assistant", fontSize = 12.sp, color = Beep.colors.muted)
            }
            Switch(checked = current.enabled, onCheckedChange = ::toggle,
                colors = SwitchDefaults.colors(checkedTrackColor = Beep.colors.ok))
        }
        Column(Modifier.alpha(if (current.enabled) 1f else 0.5f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            current.agents.forEach { agent ->
                val model = choices.find { it.id == agent.model }?.name ?: agent.model.substringAfterLast('/')
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(agent.name, fontSize = 14.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
                        Text(agent.`when`, fontSize = 12.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    Spacer(Modifier.width(8.dp))
                    Text("$model, ${agent.thinking.name.lowercase()}", fontSize = 12.sp, color = Beep.colors.muted)
                }
            }
        }
    }
}

private fun modelDetail(choice: ModelChoice): String {
    val price = { usd: Double? -> usd?.let { "$" + "%.2f".format(it) } ?: "?" }
    val prices = "${price(choice.inputPrice)} in, ${price(choice.outputPrice)} out per M tokens"
    return if (choice.seesImages) prices else "$prices, text only"
}

@Composable
private fun SettingsRow(icon: ImageVector, label: String, detail: String?, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, Modifier.size(20.dp), tint = Beep.colors.muted)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(label, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink)
            if (detail != null) Text(detail, fontSize = 12.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = Beep.colors.muted)
    }
}
