package com.flashmind.beep

import android.annotation.SuppressLint
import android.content.Context
import android.content.MutableContextWrapper
import android.graphics.Bitmap
import android.view.ViewGroup
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject
import org.json.JSONTokener
import kotlin.coroutines.resume

/**
 * The one in-app WebView, shared by the browser screen and the agent's
 * browser tool. It outlives the screen, so the agent can load pages before
 * the screen is composed and the page survives closing and reopening it.
 */
object BrowserBridge {
    private var view: WebView? = null
    private var pageLoad = CompletableDeferred<Unit>().apply { complete(Unit) }
    private var loadsStarted = 0

    val url = MutableStateFlow("")
    val progress = MutableStateFlow(100)
    val canGoBack = MutableStateFlow(false)
    val canGoForward = MutableStateFlow(false)

    /** The WebView for the browser screen to attach, moved out of any old parent. Call on the main thread. */
    fun webView(context: Context): WebView {
        view?.let { existing ->
            (existing.context as MutableContextWrapper).baseContext = context
            (existing.parent as? ViewGroup)?.removeView(existing)
            existing.onResume()
            return existing
        }
        return create(context)
    }

    /** The WebView to load pages into, left attached if the screen is showing it. */
    private fun loader(context: Context): WebView = view?.also { it.onResume() } ?: create(context)

    @SuppressLint("SetJavaScriptEnabled")
    private fun create(context: Context): WebView =
        WebView(MutableContextWrapper(context)).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.loadWithOverviewMode = true
            settings.useWideViewPort = true
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                    request.url.scheme !in setOf("http", "https")

                override fun onPageStarted(view: WebView, startedUrl: String?, favicon: Bitmap?) {
                    loadsStarted++
                    if (pageLoad.isCompleted) pageLoad = CompletableDeferred()
                    startedUrl?.let { BrowserBridge.url.value = it }
                }

                override fun onPageFinished(view: WebView, finishedUrl: String?) {
                    pageLoad.complete(Unit)
                    finishedUrl?.let { BrowserBridge.url.value = it }
                    history(view)
                }

                override fun doUpdateVisitedHistory(view: WebView, visitedUrl: String?, isReload: Boolean) {
                    visitedUrl?.let { BrowserBridge.url.value = it }
                    history(view)
                }
            }
            webChromeClient = object : WebChromeClient() {
                override fun onProgressChanged(view: WebView, newProgress: Int) {
                    BrowserBridge.progress.value = newProgress
                }
            }
            view = this
        }

    private fun history(view: WebView) {
        canGoBack.value = view.canGoBack()
        canGoForward.value = view.canGoForward()
    }

    /** Pause the page when the screen closes; the WebView itself stays, let go of the activity. */
    fun pause() {
        view?.let {
            (it.parent as? ViewGroup)?.removeView(it)
            it.onPause()
            (it.context as MutableContextWrapper).let { wrapper -> wrapper.baseContext = wrapper.applicationContext }
        }
    }

    /** Start loading [target] without waiting, for links the user taps. */
    suspend fun open(context: Context, target: String) = withContext(Dispatchers.Main) {
        val webView = loader(context)
        if (webView.url != target) startLoad(webView) { it.loadUrl(target) }
    }

    suspend fun navigate(context: Context, target: String): JSONObject = withContext(Dispatchers.Main) {
        val webView = loader(context)
        startLoad(webView) { it.loadUrl(target) }
        awaitLoad()
        pageInfo(webView)
    }

    suspend fun executeCommand(context: Context, action: String, params: JSONObject = JSONObject()): JSONObject =
        withContext(Dispatchers.Main) {
            val webView = view?.takeIf { it.url != null }
                ?: return@withContext error("no_browser_open", "No page is open. Use the navigate action with a URL first.")
            when (action) {
                "back" -> if (webView.canGoBack()) {
                    val before = loadsStarted
                    webView.goBack(); afterAction(before); pageInfo(webView)
                } else error("no_history", "There is no earlier page to go back to.")
                "forward" -> if (webView.canGoForward()) {
                    val before = loadsStarted
                    webView.goForward(); afterAction(before); pageInfo(webView)
                } else error("no_history", "There is no later page to go forward to.")
                "reload" -> { startLoad(webView) { it.reload() }; awaitLoad(); pageInfo(webView) }
                "get_page_info" -> pageInfo(webView)
                "read_page" -> {
                    val result = evaluate(webView, READ_PAGE)
                    runCatching { JSONObject(result) }.getOrElse { error("read_failed", "Could not read the page.") }
                }
                "get_text" -> {
                    val text = evaluate(webView, "(function(){ var el = document.querySelector(${q(params.optString("selector"))}); " +
                        "return el ? el.innerText.slice(0, $TEXT_LIMIT) : null; })()")
                    if (text.isEmpty()) notFound(params) else JSONObject().put("text", text)
                }
                "click" -> act(webView, params, "click", JSONObject().put("clicked", true)) { find ->
                    "(function(){ var el = $find; if (!el) return ''; el.scrollIntoView({block: 'center'}); " +
                        "if (el.focus) el.focus(); el.click(); return 'ok'; })()"
                }
                "type" -> act(webView, params, "type", JSONObject().put("typed", true)) { find ->
                    "(${TYPE_JS})($find, ${q(params.optString("text"))}, ${params.optBoolean("submit")})"
                }
                "scroll" -> {
                    val amount = if (params.optString("direction") == "up") -0.8 else 0.8
                    evaluate(webView, "window.scrollBy(0, window.innerHeight * $amount)")
                    JSONObject().put("scrolled", params.optString("direction", "down"))
                }
                "wait_for" -> {
                    val check = "!!document.querySelector(${q(params.optString("selector"))})"
                    val found = withTimeoutOrNull(10_000) {
                        while (evaluate(webView, check) != "true") delay(250)
                    } != null
                    if (found) JSONObject().put("found", true) else notFound(params)
                }
                else -> error("unknown_action", "Unknown browser action: $action")
            }
        }

    /** Run [script] on the element [params] names, then wait for any page load it starts. */
    private suspend fun act(webView: WebView, params: JSONObject, verb: String, done: JSONObject, script: (String) -> String): JSONObject {
        val find = if (params.has("index")) "document.querySelector('[data-beep-idx=\"${params.getInt("index")}\"]')"
            else "document.querySelector(${q(params.optString("selector"))})"
        val before = loadsStarted
        if (evaluate(webView, script(find)) != "ok") return notFound(params)
        afterAction(before)
        return done.put("url", webView.url ?: "").put("title", webView.title ?: "")
            .put("note", "The page may have changed; read_page before the next $verb.")
    }

    /** Wait for the load an action started, if it started one within a moment. */
    private suspend fun afterAction(loadsBefore: Int) {
        delay(400)
        if (loadsStarted != loadsBefore) awaitLoad() else settle()
    }

    private inline fun startLoad(webView: WebView, load: (WebView) -> Unit) {
        pageLoad = CompletableDeferred()
        load(webView)
    }

    /** Wait until the page has finished loading and stayed loaded for a moment. */
    private suspend fun awaitLoad() {
        withTimeoutOrNull(15_000) {
            while (true) {
                pageLoad.await()
                settle()
                if (pageLoad.isCompleted) break
            }
        }
    }

    /** Give scripts on the page a moment to render after a load or a click. */
    private suspend fun settle() = delay(500)

    private fun pageInfo(webView: WebView) = JSONObject()
        .put("url", webView.url ?: "")
        .put("title", webView.title ?: "")
        .put("loading", !pageLoad.isCompleted)

    private fun error(code: String, message: String) = JSONObject().put("error", code).put("message", message)

    private fun notFound(params: JSONObject) = if (params.has("index"))
        error("element_not_found", "No element numbered ${params.optInt("index")}. Use read_page to get fresh numbers.")
    else error("element_not_found", "No element matches selector: ${params.optString("selector")}")

    private fun q(value: String) = JSONObject.quote(value)

    /** Evaluate [js] and decode its result; strings come back unquoted, null as "". */
    private suspend fun evaluate(webView: WebView, js: String): String = suspendCancellableCoroutine { cont ->
        webView.evaluateJavascript(js) { result ->
            val value = runCatching { JSONTokener(result ?: "null").nextValue() }.getOrNull()
            cont.resume(if (value == null || value == JSONObject.NULL) "" else value.toString())
        }
    }

    private const val TEXT_LIMIT = 8000

    /** Number the visible things the agent can act on and return them with the page text. */
    private val READ_PAGE = """
        (function() {
          var sel = 'a[href],button,input:not([type=hidden]),textarea,select,summary,[role=button],[role=link],' +
            '[role=tab],[role=menuitem],[role=checkbox],[role=option],[onclick],[contenteditable=true]';
          document.querySelectorAll('[data-beep-idx]').forEach(function(e) { e.removeAttribute('data-beep-idx'); });
          var lines = [], n = 0;
          document.querySelectorAll(sel).forEach(function(el) {
            if (n >= 200) return;
            var r = el.getBoundingClientRect(), s = getComputedStyle(el);
            if (r.width < 1 || r.height < 1 || s.visibility === 'hidden' || s.display === 'none') return;
            el.setAttribute('data-beep-idx', n);
            var tag = el.tagName.toLowerCase();
            var label = (el.innerText || el.value || el.getAttribute('aria-label') || el.placeholder ||
              el.title || el.name || '').replace(/\s+/g, ' ').trim().slice(0, 80);
            var extra = tag === 'a' ? el.getAttribute('href') : (tag === 'input' ? el.type : '');
            var line = '[' + n + '] ' + tag + (extra ? ' (' + String(extra).slice(0, 80) + ')' : '') + ' "' + label + '"';
            if (r.bottom < 0 || r.top > innerHeight) line += ' off screen';
            lines.push(line);
            n++;
          });
          var text = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n');
          return JSON.stringify({
            url: location.href, title: document.title,
            text: text.slice(0, $TEXT_LIMIT), text_truncated: text.length > $TEXT_LIMIT,
            elements: lines.join('\n')
          });
        })()
    """.trimIndent()

    /** Set a field's value the way frameworks like React notice, then submit if asked. */
    private val TYPE_JS = """
        function(el, text, submit) {
          if (!el) return '';
          el.scrollIntoView({block: 'center'});
          el.focus();
          if (el.isContentEditable) {
            el.textContent = text;
          } else {
            var proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype
              : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
            Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, text);
          }
          el.dispatchEvent(new Event('input', {bubbles: true}));
          el.dispatchEvent(new Event('change', {bubbles: true}));
          if (submit) {
            var key = {key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true};
            var go = el.dispatchEvent(new KeyboardEvent('keydown', key));
            el.dispatchEvent(new KeyboardEvent('keypress', key));
            el.dispatchEvent(new KeyboardEvent('keyup', key));
            if (go && el.form) { if (el.form.requestSubmit) el.form.requestSubmit(); else el.form.submit(); }
          }
          return 'ok';
        }
    """.trimIndent()
}
