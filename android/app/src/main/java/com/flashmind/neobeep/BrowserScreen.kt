package com.flashmind.neobeep

import android.annotation.SuppressLint
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView

private val BrowserBar = Color(0xFF151D18)
private val BrowserBorder = Color(0xFF2A3830)

@SuppressLint("SetJavaScriptEnabled")
@Composable
fun BrowserScreen(url: String, onClose: () -> Unit, onContentExtracted: (String) -> Unit) {
    var webView by remember { mutableStateOf<WebView?>(null) }
    var currentUrl by remember { mutableStateOf(url) }
    var loading by remember { mutableStateOf(true) }
    var progress by remember { mutableIntStateOf(0) }
    var canGoBack by remember { mutableStateOf(false) }
    var canGoForward by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().background(Color(0xFF0D1410))) {
        // Top bar
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
                        color = Color(0xFF1C2620),
                        shape = RoundedCornerShape(8.dp),
                        modifier = Modifier.weight(1f).padding(horizontal = 4.dp)
                    ) {
                        Text(
                            currentUrl, fontSize = 12.sp, maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            color = Color(0xFF7A8A80),
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp)
                        )
                    }
                    IconButton(onClick = { webView?.reload() }) {
                        Icon(Icons.Outlined.Refresh, "Refresh", Modifier.size(20.dp))
                    }
                    IconButton(onClick = {
                        webView?.evaluateJavascript("document.body.innerText") { content ->
                            if (content != null && content != "null") {
                                val text = content
                                    .removeSurrounding("\"")
                                    .replace("\\n", "\n")
                                    .replace("\\t", "\t")
                                    .replace("\\\"", "\"")
                                onContentExtracted(text)
                            }
                        }
                        onClose()
                    }) {
                        Icon(Icons.Outlined.Close, "Close", Modifier.size(20.dp))
                    }
                }
                if (loading) {
                    LinearProgressIndicator(
                        progress = { progress / 100f },
                        modifier = Modifier.fillMaxWidth().height(2.dp),
                        color = Color(0xFF3B9B7A),
                        trackColor = BrowserBorder
                    )
                }
            }
        }
        // WebView
        AndroidView(
            factory = { context ->
                WebView(context).apply {
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    settings.loadWithOverviewMode = true
                    settings.useWideViewPort = true
                    webViewClient = object : WebViewClient() {
                        override fun onPageFinished(view: WebView?, finishedUrl: String?) {
                            currentUrl = finishedUrl ?: currentUrl
                            canGoBack = view?.canGoBack() ?: false
                            canGoForward = view?.canGoForward() ?: false
                            loading = false
                        }
                        override fun doUpdateVisitedHistory(view: WebView?, visitedUrl: String?, isReload: Boolean) {
                            currentUrl = visitedUrl ?: currentUrl
                            canGoBack = view?.canGoBack() ?: false
                            canGoForward = view?.canGoForward() ?: false
                        }
                    }
                    webChromeClient = object : WebChromeClient() {
                        override fun onProgressChanged(view: WebView?, newProgress: Int) {
                            progress = newProgress
                            loading = newProgress < 100
                        }
                    }
                    loadUrl(url)
                    webView = this
                }
            },
            modifier = Modifier.fillMaxSize()
        )
        DisposableEffect(Unit) {
            onDispose {
                webView?.stopLoading()
                webView?.destroy()
                webView = null
            }
        }
    }
}
