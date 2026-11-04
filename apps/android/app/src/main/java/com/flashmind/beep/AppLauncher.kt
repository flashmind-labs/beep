package com.flashmind.beep

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.AlarmClock
import android.provider.CalendarContract
import androidx.compose.ui.platform.UriHandler

object AppLauncher {
    private val appPackages = mapOf(
        "spotify" to "com.spotify.music",
        "youtube" to "com.google.android.youtube",
        "maps" to "com.google.android.apps.maps",
        "whatsapp" to "com.whatsapp",
        "gmail" to "com.google.android.gm",
        "uber" to "com.ubercab"
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
        "clock" -> Intent(AlarmClock.ACTION_SHOW_ALARMS)
        "calendar" -> Intent(Intent.ACTION_INSERT).apply {
            data = CalendarContract.Events.CONTENT_URI
            if (uri.isNotEmpty()) putExtra(CalendarContract.Events.TITLE, uri)
        }
        "browser" -> Intent(Intent.ACTION_VIEW, Uri.parse(uri.ifEmpty { "https://google.com" }))
        else -> Intent(Intent.ACTION_VIEW).apply { if (uri.isNotEmpty()) data = Uri.parse(uri) }
    }

    /** Sets the alarm in the Clock app without showing it. Falls back to its set-alarm screen, filled in. */
    fun setAlarm(context: Context, alarm: AlarmRequest): Boolean {
        val intent = Intent(AlarmClock.ACTION_SET_ALARM)
            .putExtra(AlarmClock.EXTRA_HOUR, alarm.hour)
            .putExtra(AlarmClock.EXTRA_MINUTES, alarm.minute)
        if (alarm.label.isNotBlank()) intent.putExtra(AlarmClock.EXTRA_MESSAGE, alarm.label)
        if (alarm.days.isNotEmpty()) intent.putIntegerArrayListExtra(AlarmClock.EXTRA_DAYS, ArrayList(alarm.days))
        return tryStart(context, Intent(intent).putExtra(AlarmClock.EXTRA_SKIP_UI, true)) || tryStart(context, intent)
    }

    private fun tryStart(context: Context, intent: Intent): Boolean = try {
        start(context, intent); true
    } catch (_: Exception) { false }

    /**
     * Starts another app's screen in a task of its own. Started from the bubble
     * without this, the app opens inside the bubble's small window.
     */
    fun start(context: Context, intent: Intent) {
        context.applicationContext.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}

/** Opens links in replies with [AppLauncher.start], so the bubble never hosts the app a link opens. */
class OutsideUriHandler(private val context: Context) : UriHandler {
    override fun openUri(uri: String) {
        try {
            AppLauncher.start(context, Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
        } catch (e: ActivityNotFoundException) {
            throw IllegalArgumentException("Can't open $uri.", e)
        }
    }
}

/** The phone's default browser, outside the app. */
internal object ExternalBrowser {
    private fun view(url: String) = Intent(Intent.ACTION_VIEW, Uri.parse(url)).addCategory(Intent.CATEGORY_BROWSABLE)

    /** The default browser's package, or null when none is set and Android would ask. */
    private fun defaultPackage(context: Context): String? {
        val info = context.packageManager.resolveActivity(view("https://example.com"), PackageManager.MATCH_DEFAULT_ONLY)
            ?.activityInfo ?: return null
        // With no default, the resolver itself comes back.
        if (info.packageName == "android" || info.name.endsWith("ResolverActivity")) return null
        return info.packageName.takeIf { it != context.packageName }
    }

    /** Opens [url] in the default browser, or lets the user pick one when there is none. */
    fun open(context: Context, url: String) {
        if (url.isEmpty()) return
        val pkg = defaultPackage(context)
        val intent = if (pkg != null) view(url).setPackage(pkg) else Intent.createChooser(view(url), "Open in browser")
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { context.startActivity(intent) } catch (_: ActivityNotFoundException) {
            context.startActivity(Intent.createChooser(view(url), "Open in browser").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }
}
