package com.flashmind.beep

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager

/** Restores the arrow for installations that selected a retired launcher icon. */
object LauncherIcon {
    private val retired = listOf(".LauncherClay", ".LauncherJelly", ".LauncherBeep", ".LauncherBubble", ".LauncherSpark", ".LauncherGhost")

    fun apply(context: Context) {
        val pm = context.packageManager
        // Enable the default first so the app always has a launcher entry.
        val states = listOf(".LauncherPager" to PackageManager.COMPONENT_ENABLED_STATE_ENABLED) +
            retired.map { it to PackageManager.COMPONENT_ENABLED_STATE_DISABLED }
        for ((alias, state) in states) {
            val component = ComponentName(context.packageName, context.packageName + alias)
            if (pm.getComponentEnabledSetting(component) != state) {
                pm.setComponentEnabledSetting(component, state, PackageManager.DONT_KILL_APP)
            }
        }
        context.getSharedPreferences("appearance", Context.MODE_PRIVATE).edit().remove("app_icon").apply()
    }
}
