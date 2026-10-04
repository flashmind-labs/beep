package com.flashmind.beep

import android.app.AppOpsManager
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.Intent
import android.os.Process
import android.provider.Settings
import com.flashmind.beep.proto.ForegroundApp

/** The app the user was in when they opened Beep's bubble, from usage access. */
object ScreenContext {
    /** The app behind the bubble, or null without usage access. */
    fun foreground(context: Context): ForegroundApp? {
        val pkg = lastResumed(context) ?: return null
        return ForegroundApp(label(context, pkg), pkg)
    }

    fun usageAllowed(context: Context): Boolean {
        val ops = context.getSystemService(AppOpsManager::class.java)
        return ops.unsafeCheckOpNoThrow(AppOpsManager.OPSTR_GET_USAGE_STATS, Process.myUid(), context.packageName) ==
            AppOpsManager.MODE_ALLOWED
    }

    fun usageSettings(context: Context) = context.startActivity(
        Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))

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

    private fun skipped(context: Context): Set<String> {
        val home = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        val launcher = context.packageManager.resolveActivity(home, 0)?.activityInfo?.packageName
        return setOfNotNull(context.packageName, launcher, "com.android.systemui")
    }

    private fun label(context: Context, pkg: String): String = runCatching {
        val pm = context.packageManager
        pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
    }.getOrDefault(pkg)
}
