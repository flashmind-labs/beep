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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext

/**
 * The one in-app WebView for links the user opens. It outlives the screen,
 * so a page can start loading before the screen is composed and survives
 * closing and reopening it.
 */
object BrowserBridge {
    private var view: WebView? = null

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
                    startedUrl?.let { BrowserBridge.url.value = it }
                }

                override fun onPageFinished(view: WebView, finishedUrl: String?) {
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
        if (webView.url != target) webView.loadUrl(target)
    }
}
