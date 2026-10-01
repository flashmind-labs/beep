package com.flashmind.neobeep

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.AlarmClock
import android.provider.CalendarContract

object AppLauncher {
    private val appPackages = mapOf(
        "spotify" to "com.spotify.music",
        "youtube" to "com.google.android.youtube",
        "maps" to "com.google.android.apps.maps",
        "whatsapp" to "com.whatsapp",
        "gmail" to "com.google.android.gm"
    )

    fun launch(context: Context, app: String, uri: String, fallbackUrl: String): Boolean {
        val intent = buildIntent(app, uri)
        // Try specific app package first
        val pkg = appPackages[app]
        if (pkg != null) intent.setPackage(pkg)
        if (tryStart(context, intent)) return true
        // Fall back to generic ACTION_VIEW without package restriction
        if (pkg != null && uri.isNotEmpty()) {
            val generic = Intent(Intent.ACTION_VIEW, Uri.parse(uri))
            if (tryStart(context, generic)) return true
        }
        // Fall back to fallback URL in browser
        if (fallbackUrl.isNotEmpty()) {
            val browser = Intent(Intent.ACTION_VIEW, Uri.parse(fallbackUrl))
            if (tryStart(context, browser)) return true
        }
        // Fall back to Play Store
        if (pkg != null) {
            val store = Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=$pkg"))
            if (tryStart(context, store)) return true
            val web = Intent(Intent.ACTION_VIEW, Uri.parse("https://play.google.com/store/apps/details?id=$pkg"))
            tryStart(context, web)
        }
        return false
    }

    private fun buildIntent(app: String, uri: String): Intent = when (app) {
        "phone" -> Intent(Intent.ACTION_DIAL).apply { if (uri.isNotEmpty()) data = Uri.parse(if (uri.startsWith("tel:")) uri else "tel:$uri") }
        "clock" -> Intent(AlarmClock.ACTION_SET_ALARM)
        "calendar" -> Intent(Intent.ACTION_INSERT).apply {
            data = CalendarContract.Events.CONTENT_URI
            if (uri.isNotEmpty()) putExtra(CalendarContract.Events.TITLE, uri)
        }
        "browser" -> Intent(Intent.ACTION_VIEW, Uri.parse(uri.ifEmpty { "https://google.com" }))
        else -> Intent(Intent.ACTION_VIEW).apply { if (uri.isNotEmpty()) data = Uri.parse(uri) }
    }

    private fun tryStart(context: Context, intent: Intent): Boolean = try {
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent); true
    } catch (_: Exception) { false }
}
