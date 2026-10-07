package com.flashmind.beep

import android.webkit.WebView
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.ArrowForward
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle

private val BrowserBar: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val BrowserBorder: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine

/** The shared WebView with the chat in a panel below it that drags between short and tall. */
@Composable
fun BrowserScreen(onClose: () -> Unit, chat: @Composable (Modifier) -> Unit) {
    val url by BrowserBridge.url.collectAsStateWithLifecycle()
    val progress by BrowserBridge.progress.collectAsStateWithLifecycle()
    val canGoBack by BrowserBridge.canGoBack.collectAsStateWithLifecycle()
    val canGoForward by BrowserBridge.canGoForward.collectAsStateWithLifecycle()
    var webView by remember { mutableStateOf<WebView?>(null) }
    BackHandler { if (canGoBack) webView?.goBack() else onClose() }

    BoxWithConstraints(Modifier.fillMaxSize().background(Beep.colors.page).imePadding()) {
        val density = LocalDensity.current
        val shortPanel = 220.dp
        val tallPanel = maxHeight * 0.7f
        var panel by remember { mutableStateOf(shortPanel) }
        Column(Modifier.fillMaxSize()) {
            Surface(color = BrowserBar, shadowElevation = 4.dp) {
                Column {
                    Row(
                        Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 4.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        IconButton(onClick = { webView?.goBack() }, enabled = canGoBack) {
                            Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back", Modifier.size(20.dp))
                        }
                        IconButton(onClick = { webView?.goForward() }, enabled = canGoForward) {
                            Icon(Icons.AutoMirrored.Outlined.ArrowForward, "Forward", Modifier.size(20.dp))
                        }
                        Surface(
                            color = Beep.colors.raised,
                            shape = RoundedCornerShape(8.dp),
                            modifier = Modifier.weight(1f).padding(horizontal = 4.dp)
                        ) {
                            Text(
                                url, fontSize = 12.sp, maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                color = Beep.colors.muted,
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp)
                            )
                        }
                        IconButton(onClick = { webView?.reload() }) {
                            Icon(Icons.Outlined.Refresh, "Refresh", Modifier.size(20.dp))
                        }
                        IconButton(onClick = onClose) {
                            Icon(Icons.Outlined.Close, "Close", Modifier.size(20.dp))
                        }
                    }
                    if (progress < 100) {
                        LinearProgressIndicator(
                            progress = { progress / 100f },
                            modifier = Modifier.fillMaxWidth().height(2.dp),
                            color = Beep.colors.brand,
                            trackColor = BrowserBorder
                        )
                    }
                }
            }
            AndroidView(
                factory = { context -> BrowserBridge.webView(context).also { webView = it } },
                modifier = Modifier.weight(1f).fillMaxWidth()
            )
            Surface(color = BrowserBar, shadowElevation = 8.dp,
                shape = RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp)) {
                Column(Modifier.height(panel).navigationBarsPadding()) {
                    // Drag the handle to resize the chat; it snaps to short or tall on release.
                    Box(
                        Modifier.fillMaxWidth().height(24.dp).pointerInput(tallPanel) {
                            detectVerticalDragGestures(
                                onDragEnd = { panel = if (panel > (shortPanel + tallPanel) / 2) tallPanel else shortPanel },
                                onVerticalDrag = { _, dy ->
                                    panel = (panel - with(density) { dy.toDp() }).coerceIn(shortPanel, tallPanel)
                                }
                            )
                        },
                        contentAlignment = Alignment.Center
                    ) {
                        Box(Modifier.size(width = 36.dp, height = 4.dp)
                            .background(BrowserBorder, RoundedCornerShape(2.dp)))
                    }
                    chat(Modifier.weight(1f).fillMaxWidth())
                }
            }
        }
    }
    DisposableEffect(Unit) {
        onDispose { webView = null; BrowserBridge.pause() }
    }
}
