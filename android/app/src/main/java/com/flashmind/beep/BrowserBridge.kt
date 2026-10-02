package com.flashmind.beep

import android.webkit.WebView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.json.JSONObject
import kotlin.coroutines.resume

object BrowserBridge {
    @Volatile
    var activeWebView: WebView? = null

    suspend fun executeCommand(browserAction: String, params: JSONObject): JSONObject {
        val webView = activeWebView
            ?: return JSONObject().put("error", "no_browser_open")
                .put("message", "No browser is open. Use the navigate action with a URL first.")

        return withContext(Dispatchers.Main) {
            when (browserAction) {
                "get_page_info" -> JSONObject()
                    .put("url", webView.url ?: "")
                    .put("title", webView.title ?: "")

                "get_text" -> {
                    val selector = sanitizeSelector(params.optString("selector", "body"))
                    val js = "(function(){ var el = document.querySelector('$selector'); return el ? el.textContent : null; })()"
                    val result = evaluateJs(webView, js)
                    if (result == "null" || result.isEmpty()) {
                        JSONObject().put("error", "element_not_found")
                            .put("message", "No element matches selector: ${params.optString("selector")}")
                    } else {
                        JSONObject().put("text", result)
                    }
                }

                "click" -> {
                    val selector = sanitizeSelector(params.optString("selector", ""))
                    val js = "(function(){ var el = document.querySelector('$selector'); if(el){ el.click(); return 'clicked'; } return null; })()"
                    val result = evaluateJs(webView, js)
                    if (result == "null" || result.isEmpty()) {
                        JSONObject().put("error", "element_not_found")
                            .put("message", "No element matches selector: ${params.optString("selector")}")
                    } else {
                        JSONObject().put("clicked", true)
                    }
                }

                "type" -> {
                    val selector = sanitizeSelector(params.optString("selector", ""))
                    val text = params.optString("text", "").replace("\\", "\\\\").replace("'", "\\'")
                    val js = "(function(){ var el = document.querySelector('$selector'); if(el){ el.focus(); el.value = '$text'; el.dispatchEvent(new Event('input', {bubbles:true})); el.dispatchEvent(new Event('change', {bubbles:true})); return 'typed'; } return null; })()"
                    val result = evaluateJs(webView, js)
                    if (result == "null" || result.isEmpty()) {
                        JSONObject().put("error", "element_not_found")
                            .put("message", "No element matches selector: ${params.optString("selector")}")
                    } else {
                        JSONObject().put("typed", true)
                    }
                }

                "scroll" -> {
                    val direction = params.optString("direction", "down")
                    val amount = if (direction == "up") -500 else 500
                    evaluateJs(webView, "window.scrollBy(0, $amount)")
                    JSONObject().put("scrolled", direction)
                }

                else -> JSONObject().put("error", "unknown_action")
                    .put("message", "Unknown browser action: $browserAction")
            }
        }
    }

    private fun sanitizeSelector(selector: String): String {
        // Escape single quotes to prevent JS injection through CSS selectors
        return selector.replace("\\", "\\\\").replace("'", "\\'")
    }

    private suspend fun evaluateJs(webView: WebView, js: String): String {
        return suspendCancellableCoroutine { cont ->
            webView.evaluateJavascript(js) { result ->
                // WebView returns the result wrapped in quotes for strings
                val cleaned = if (result != null && result.startsWith("\"") && result.endsWith("\"")) {
                    result.substring(1, result.length - 1)
                        .replace("\\n", "\n")
                        .replace("\\t", "\t")
                        .replace("\\\"", "\"")
                } else {
                    result ?: ""
                }
                cont.resume(cleaned)
            }
        }
    }
}
