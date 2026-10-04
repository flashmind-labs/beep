package com.flashmind.beep

import android.accessibilityservice.AccessibilityService
import android.app.AppOpsManager
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Process
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import com.flashmind.beep.proto.ForegroundApp
import org.json.JSONObject

/**
 * What the user is looking at when they open Beep's bubble: the app's name
 * from usage access, and its screen text from the accessibility service.
 */
object ScreenContext {
    /** The app behind the bubble, or null without usage access or the accessibility service. */
    fun foreground(context: Context): ForegroundApp? {
        val pkg = ScreenReaderService.lastApp ?: lastResumed(context) ?: return null
        return ForegroundApp(label(context, pkg), pkg)
    }

    fun usageAllowed(context: Context): Boolean {
        val ops = context.getSystemService(AppOpsManager::class.java)
        return ops.unsafeCheckOpNoThrow(AppOpsManager.OPSTR_GET_USAGE_STATS, Process.myUid(), context.packageName) ==
            AppOpsManager.MODE_ALLOWED
    }

    fun readerOn(context: Context): Boolean {
        val enabled = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
        val ours = ComponentName(context, ScreenReaderService::class.java).flattenToString()
        return enabled.split(':').any { it.equals(ours, ignoreCase = true) }
    }

    fun usageSettings(context: Context) = context.startActivity(
        Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))

    fun readerSettings(context: Context) = context.startActivity(
        Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))

    /** The text on screen behind the bubble, for the agent's read_screen tool. */
    fun read(): JSONObject {
        val service = ScreenReaderService.instance
            ?: return JSONObject().put("error", "reader_off").put("message",
                "Screen reading is off. Ask the user to turn it on in Beep's settings under Screen context.")
        return service.read()
    }

    /** The last app other than Beep, the launcher or the system UI to come to the front in the last hour. */
    private fun lastResumed(context: Context): String? {
        if (!usageAllowed(context)) return null
        val usage = context.getSystemService(UsageStatsManager::class.java)
        val now = System.currentTimeMillis()
        val events = usage.queryEvents(now - 60 * 60_000L, now)
        val skip = skipped(context)
        val event = UsageEvents.Event()
        var last: String? = null
        while (events.getNextEvent(event)) {
            if (event.eventType == UsageEvents.Event.ACTIVITY_RESUMED && event.packageName !in skip) last = event.packageName
        }
        return last
    }

    internal fun skipped(context: Context): Set<String> {
        val home = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        val launcher = context.packageManager.resolveActivity(home, 0)?.activityInfo?.packageName
        return setOfNotNull(context.packageName, launcher, "com.android.systemui")
    }

    private fun label(context: Context, pkg: String): String = runCatching {
        val pm = context.packageManager
        pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
    }.getOrDefault(pkg)
}

/** Reads the screen of the app behind Beep's bubble. The user turns it on in Android's accessibility settings. */
class ScreenReaderService : AccessibilityService() {
    companion object {
        @Volatile var instance: ScreenReaderService? = null
            private set
        /** The last app other than Beep to show a window, while the service runs. */
        @Volatile var lastApp: String? = null
            private set
        private const val TEXT_LIMIT = 8000
    }

    private var skip = emptySet<String>()

    override fun onServiceConnected() {
        skip = ScreenContext.skipped(this)
        instance = this
    }

    override fun onUnbind(intent: Intent?): Boolean {
        instance = null
        lastApp = null
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent) {
        if (event.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) return
        val pkg = event.packageName?.toString() ?: return
        if (pkg !in skip) lastApp = pkg
    }

    override fun onInterrupt() {}

    fun read(): JSONObject {
        // Windows come topmost first; Beep's own bubble or panel is usually on top.
        val apps = windows.filter { it.type == AccessibilityWindowInfo.TYPE_APPLICATION }
            .mapNotNull { window -> window.root?.takeIf { it.packageName?.toString() != packageName }?.let { window to it } }
        val (_, root) = apps.firstOrNull { it.first.isActive } ?: apps.firstOrNull()
            ?: return JSONObject().put("error", "no_app").put("message", "No other app is on screen.")
        val pkg = root.packageName?.toString() ?: ""
        val lines = mutableListOf<String>()
        collect(root, lines)
        val text = lines.joinToString("\n")
        return JSONObject()
            .put("package", pkg)
            .put("app", runCatching { packageManager.getApplicationLabel(packageManager.getApplicationInfo(pkg, 0)).toString() }.getOrDefault(pkg))
            .put("text", text.take(TEXT_LIMIT))
            .put("text_truncated", text.length > TEXT_LIMIT)
    }

    /** Visible text and descriptions in screen order, with fields and buttons marked. */
    private fun collect(node: AccessibilityNodeInfo, lines: MutableList<String>, depth: Int = 0) {
        if (depth > 60 || !node.isVisibleToUser) return
        val text = (node.text ?: node.contentDescription)?.toString()?.replace(Regex("\\s+"), " ")?.trim()
        if (!text.isNullOrEmpty()) {
            val line = when {
                node.isEditable -> "[field] $text"
                node.isClickable && node.childCount == 0 -> "[button] $text"
                else -> text
            }
            if (lines.lastOrNull() != line) lines += line
        }
        for (i in 0 until node.childCount) node.getChild(i)?.let { collect(it, lines, depth + 1) }
    }
}
