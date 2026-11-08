package com.flashmind.beep

import android.annotation.SuppressLint
import android.content.ClipData
import android.content.ClipboardManager
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.json.JSONObject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID

/** Local Mermaid page, isolated from the mesh, credentials and network. */
@Composable
internal fun MermaidDiagram(source: String) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var expanded by remember(source) { mutableStateOf(false) }
    var pending by remember { mutableStateOf<ByteArray?>(null) }
    var exportError by remember { mutableStateOf("") }
    val saveSvg = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/svg+xml")) { uri ->
        if (uri != null) runCatching {
            context.contentResolver.openOutputStream(uri)?.use { it.write(pending ?: error("Missing diagram")) }
                ?: error("Could not open file")
        }.onFailure { exportError = "Could not save diagram." }
        pending = null
    }
    val savePng = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")) { uri ->
        if (uri != null) runCatching {
            context.contentResolver.openOutputStream(uri)?.use { it.write(pending ?: error("Missing diagram")) }
                ?: error("Could not open file")
        }.onFailure { exportError = "Could not save diagram." }
        pending = null
    }
    val action: (JSONObject) -> Unit = { message ->
        when (message.optString("action")) {
            "expand" -> expanded = true
            "close" -> expanded = false
            "copy" -> {
                (context as? MainActivity)?.suppressNextClipboard = true
                context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Diagram", source))
            }
            "copy-image" -> {
                if (message.optString("mime") == "image/png" && message.optString("data").length <= 24_000_000) scope.launch {
                    runCatching {
                        val file = withContext(Dispatchers.IO) {
                            val folder = File(context.cacheDir, "sent").apply { mkdirs() }
                            File(folder, "diagram-${UUID.randomUUID()}.png").apply {
                                writeBytes(android.util.Base64.decode(message.getString("data"), android.util.Base64.DEFAULT))
                            }
                        }
                        (context as? MainActivity)?.suppressNextClipboard = true
                        val clip = ClipData.newUri(context.contentResolver, "Diagram", SentFiles.uriOf(context, file))
                        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
                    }.onFailure { exportError = "Could not copy image." }
                }
            }
            "save" -> {
                val mime = message.optString("mime")
                if (mime == "image/png" || mime == "image/svg+xml") runCatching {
                    pending = android.util.Base64.decode(message.getString("data"), android.util.Base64.DEFAULT)
                    if (mime == "image/png") savePng.launch("diagram.png") else saveSvg.launch("diagram.svg")
                }.onFailure { exportError = "Could not export diagram." }
            }
        }
    }
    androidx.compose.foundation.layout.Column {
        MermaidPage(source, false, action)
        if (exportError.isNotEmpty()) androidx.compose.material3.Text(exportError, color = Beep.colors.muted)
    }
    if (expanded) Dialog(onDismissRequest = { expanded = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        MermaidPage(source, true, action)
    }
}

@androidx.annotation.Keep
private class DiagramBridge(private val receive: (String) -> Unit) {
    @JavascriptInterface
    fun postMessage(message: String) { if (message.length <= 24_000_000) receive(message) }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun MermaidPage(source: String, expanded: Boolean, action: (JSONObject) -> Unit) {
    var height by remember(source) { mutableIntStateOf(320) }
    val currentAction by rememberUpdatedState(action)
    val dark = androidx.compose.foundation.isSystemInDarkTheme()
    val context = LocalContext.current
    val web = remember(source, expanded, dark) {
        WebView(context).apply {
            setBackgroundColor(android.graphics.Color.TRANSPARENT)
            settings.javaScriptEnabled = true
            settings.blockNetworkLoads = true
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            settings.useWideViewPort = true
            settings.loadWithOverviewMode = true
            settings.setSupportZoom(true)
            settings.builtInZoomControls = true
            settings.displayZoomControls = false
            addJavascriptInterface(DiagramBridge { text ->
                post {
                    runCatching {
                        val message = JSONObject(text)
                        if (message.optString("kind") != "beep-diagram") return@runCatching
                        if (message.optString("action") == "resize" && !expanded) height = message.optInt("height", 320).coerceIn(120, 700)
                        else currentAction(message)
                    }
                }
            }, "beepDiagram")
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true
                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                    val url = request.url.toString()
                    return if (url in setOf("file:///android_asset/index.html", "file:///android_asset/renderer.js", "file:///android_asset/mermaid.min.js") || url.startsWith("blob:") || url.startsWith("data:")) null
                    else WebResourceResponse("text/plain", "UTF-8", java.io.ByteArrayInputStream(byteArrayOf()))
                }
                override fun onPageFinished(view: WebView, url: String) {
                    if (url != "file:///android_asset/index.html") return
                    val input = JSONObject().put("source", source).put("expanded", expanded).put("dark", dark)
                    view.evaluateJavascript("window.renderDiagram($input)", null)
                }
            }
            loadUrl("file:///android_asset/index.html")
        }
    }
    DisposableEffect(web) { onDispose { web.removeJavascriptInterface("beepDiagram"); web.stopLoading(); web.destroy() } }
    AndroidView(factory = { web }, modifier = if (expanded) Modifier.fillMaxSize() else Modifier.fillMaxWidth().height(height.dp))
}
