package com.flashmind.beep

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.flashmind.beep.proto.AppIntegration
import com.flashmind.beep.proto.AppKind
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
fun AppsScreen(repository: ChatRepository, onBack: () -> Unit, onCustom: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var apps by remember { mutableStateOf<List<AppIntegration>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf<String?>(null) }

    fun refresh() { scope.launch {
        try { apps = withContext(Dispatchers.IO) { repository.listApps() }; error = "" }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false; busy = null }
    } }
    fun connect(app: AppIntegration) { scope.launch {
        busy = app.slug
        if (app.kind == AppKind.FASTMAIL) {
            val appContext = context.applicationContext
            repository.authorizeApp(app.slug) { url ->
                appContext.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            }
            return@launch
        }
        try {
            val url = withContext(Dispatchers.IO) { repository.connectApp(app.slug) }
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (e: Exception) { error = e.message.orEmpty(); busy = null }
    } }
    fun disconnect(app: AppIntegration) { scope.launch {
        busy = app.slug
        try { withContext(Dispatchers.IO) { repository.disconnectApp(app.slug) }; refresh() }
        catch (e: Exception) { error = e.message.orEmpty(); busy = null }
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

    // Coming back from the browser after signing in is the cue to look again.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) refresh() }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    val t = Beep.colors
    Scaffold(containerColor = t.page, topBar = {
        TopAppBar(title = { Text(displayText("Apps"), style = displayStyle(24.sp)) },
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
                    AppRow(app, busy == app.slug, onConnect = { connect(app) }, onDisconnect = { disconnect(app) })
                }
            }
            if (available.isNotEmpty()) {
                item { SectionLabel(if (connected.isEmpty()) "Available" else "More apps") }
                items(available, key = { "a-" + it.slug }) { app ->
                    AppRow(app, busy == app.slug, onConnect = { connect(app) }, onDisconnect = { disconnect(app) })
                }
            }
            item {
                TextButton(onClick = onCustom, modifier = Modifier.padding(top = 4.dp)) {
                    Icon(Icons.Outlined.Tune, null, Modifier.size(16.dp), tint = t.muted)
                    Spacer(Modifier.width(8.dp))
                    Text("Custom connections", color = t.muted, fontSize = 13.sp)
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
private fun AppRow(app: AppIntegration, busy: Boolean, onConnect: () -> Unit, onDisconnect: () -> Unit) {
    val t = Beep.colors
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            AppLogo(app)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(app.name, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = t.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(2.dp))
                Text(app.description, fontSize = 12.sp, color = t.muted, lineHeight = 16.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(10.dp))
            when {
                busy -> CircularProgressIndicator(Modifier.padding(horizontal = 18.dp).size(20.dp), strokeWidth = 2.dp, color = t.ok)
                app.connected -> ConnectedChip(canDisconnect = true, onDisconnect = onDisconnect)
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

@Composable
private fun ConnectedChip(canDisconnect: Boolean, onDisconnect: () -> Unit) {
    val t = Beep.colors
    var open by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.clip(CircleShape).background(t.tints[1])
            .then(if (canDisconnect) Modifier.clickableNoRipple { open = true } else Modifier)
            .padding(start = 10.dp, end = if (canDisconnect) 6.dp else 12.dp, top = 6.dp, bottom = 6.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Icons.Outlined.Check, null, Modifier.size(14.dp), tint = t.ok)
            Text("Connected", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
            if (canDisconnect) Icon(Icons.Outlined.ExpandMore, "Options", Modifier.size(16.dp), tint = t.muted)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text("Disconnect", color = t.error) }, onClick = { open = false; onDisconnect() })
        }
    }
}

@Composable
private fun Modifier.clickableNoRipple(onClick: () -> Unit): Modifier =
    clickable(remember { MutableInteractionSource() }, indication = null, onClick = onClick)

/** Bundled brand icons for the curated apps. */
internal val bundledLogos = mapOf(
    "gmail" to R.drawable.logo_gmail, "googlecalendar" to R.drawable.logo_googlecalendar,
    "notion" to R.drawable.logo_notion, "googledrive" to R.drawable.logo_googledrive,
    "slack" to R.drawable.logo_slack, "github" to R.drawable.logo_github,
    "linear" to R.drawable.logo_linear, "todoist" to R.drawable.logo_todoist,
    "spotify" to R.drawable.logo_spotify, "fastmail" to R.drawable.logo_fastmail,
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

/** The bundled logo of the app a tool belongs to: Composio tools are `GMAIL_SEND_EMAIL`, MCP tools `fastmail_x`. */
fun appLogoForTool(name: String): Int? = bundledLogos[name.substringBefore('_').lowercase()]

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
