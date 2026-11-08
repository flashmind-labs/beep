package com.flashmind.beep

import android.content.ComponentName
import android.content.pm.PackageManager
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Test

class LauncherIconTest {
    @Test
    fun retiredSelectionsReturnToTheArrow() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val pm = context.packageManager
        val primary = ComponentName(context.packageName, context.packageName + ".LauncherPager")
        val retired = listOf("Clay", "Jelly", "Beep", "Bubble", "Spark", "Ghost")
        for (name in retired) {
            val previous = ComponentName(context.packageName, context.packageName + ".Launcher$name")
            pm.setComponentEnabledSetting(previous, PackageManager.COMPONENT_ENABLED_STATE_ENABLED, PackageManager.DONT_KILL_APP)
            pm.setComponentEnabledSetting(primary, PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP)
            context.getSharedPreferences("appearance", 0).edit().putString("app_icon", name.lowercase()).commit()

            LauncherIcon.apply(context)

            assertEquals(PackageManager.COMPONENT_ENABLED_STATE_ENABLED, pm.getComponentEnabledSetting(primary))
            assertEquals(PackageManager.COMPONENT_ENABLED_STATE_DISABLED, pm.getComponentEnabledSetting(previous))
            assertEquals(false, context.getSharedPreferences("appearance", 0).contains("app_icon"))
            @Suppress("DEPRECATION")
            assertEquals(R.mipmap.ic_launcher, pm.getActivityInfo(previous, PackageManager.GET_DISABLED_COMPONENTS).icon)
        }
    }

    @Test
    fun conversationNotificationsUseTheArrow() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals(R.mipmap.ic_launcher, ChatNotifications.beepPerson(context).icon?.resId)
    }
}
