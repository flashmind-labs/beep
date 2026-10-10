package com.flashmind.beep

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import androidx.annotation.DrawableRes
import androidx.annotation.RawRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val IconGround = Color(0xFF2F8A69)
private val InkGround = Color(0xFF0A0E0C)

/** A rendered avatar's bubble loops, one per mood. */
class Clips(@RawRes val idle: Int, @RawRes val thinking: Int, @RawRes val talking: Int)

/**
 * Beep's look, as the launcher icon and the bubble. Each one is an activity-alias in the manifest;
 * only one is enabled at a time. The bubble plays its [clips].
 */
enum class Avatar(
    val key: String, val label: String, val alias: String, @DrawableRes val foreground: Int, @DrawableRes val bubbleIcon: Int,
    val ground: Color, val clips: Clips,
) {
    Pager("pager", "Pager", ".LauncherPager", R.drawable.ic_launcher_pager_foreground, R.mipmap.ic_launcher_pager, IconGround,
        Clips(R.raw.avatar_pager_idle, R.raw.avatar_pager_thinking, R.raw.avatar_pager_talking)),
    Clay("clay", "Clay", ".LauncherClay", R.drawable.ic_launcher_clay_foreground, R.mipmap.ic_launcher_clay, InkGround,
        Clips(R.raw.avatar_clay_idle, R.raw.avatar_clay_thinking, R.raw.avatar_clay_talking)),
    Jelly("jelly", "Jelly", ".LauncherJelly", R.drawable.ic_launcher_jelly_foreground, R.mipmap.ic_launcher_jelly, InkGround,
        Clips(R.raw.avatar_jelly_idle, R.raw.avatar_jelly_thinking, R.raw.avatar_jelly_talking));

    companion object {
        fun fromKey(key: String?): Avatar? = entries.firstOrNull { it.key == key }
    }
}

object Avatars {
    private const val PREFS = "appearance"
    private const val KEY = "app_icon"
    private val ManifestDefault = Avatar.Pager
    /**
     * Aliases of the avatars that were dropped. They stay in the manifest, showing the pager, so a
     * phone that had one enabled keeps a launcher entry until [apply] turns it off.
     */
    private val Retired = listOf(".LauncherBeep", ".LauncherBubble", ".LauncherSpark", ".LauncherGhost")

    /** The user's pick, or null for the default. */
    val choice = mutableStateOf<Avatar?>(null)
    private var loaded = false

    /** The avatar in use: the user's pick, or the pager. */
    val current: Avatar get() = choice.value ?: ManifestDefault

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        choice.value = Avatar.fromKey(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null))
    }

    fun set(context: Context, icon: Avatar?) {
        choice.value = icon
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, icon?.key).apply()
        apply(context)
        // The Android bubble draws a fixed icon, so post it again with the new one.
        if (SystemBubble.shown(context)) SystemBubble.show(context)
    }

    /** Enables the alias for the current icon and disables the rest, touching only aliases whose state changes. */
    fun apply(context: Context) {
        load(context)
        val wanted = current
        val pm = context.packageManager
        // Enable first so the app is never left without a launcher entry.
        val ordered = Avatar.entries.sortedByDescending { it == wanted }
        for (icon in ordered) {
            val component = ComponentName(context.packageName, context.packageName + icon.alias)
            val enabled = when (pm.getComponentEnabledSetting(component)) {
                PackageManager.COMPONENT_ENABLED_STATE_ENABLED -> true
                PackageManager.COMPONENT_ENABLED_STATE_DEFAULT -> icon == ManifestDefault
                else -> false
            }
            if (enabled == (icon == wanted)) continue
            val state = if (icon == wanted) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
                else PackageManager.COMPONENT_ENABLED_STATE_DISABLED
            pm.setComponentEnabledSetting(component, state, PackageManager.DONT_KILL_APP)
        }
        for (alias in Retired) {
            val component = ComponentName(context.packageName, context.packageName + alias)
            if (pm.getComponentEnabledSetting(component) == PackageManager.COMPONENT_ENABLED_STATE_DISABLED) continue
            pm.setComponentEnabledSetting(component, PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP)
        }
    }
}

/** Row of avatars; the pager until the user picks another. */
@Composable
fun AvatarPicker(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    remember { Avatars.load(context.applicationContext) }
    val current = Avatars.current
    Column(modifier) {
        Text("Avatar", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Avatar.entries.forEach { icon ->
                IconOption(icon.label, icon, current == icon) { Avatars.set(context, icon) }
            }
        }
        Spacer(Modifier.height(6.dp))
        Text("How Beep looks on your home screen and in the bubble. The home screen may take a moment to update.",
            fontSize = 11.sp, color = Beep.colors.muted)
    }
}

@Composable
private fun IconOption(label: String, icon: Avatar, selected: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.clip(CircleShape).clickable(onClick = onClick)) {
        Box(Modifier.size(44.dp)
            .border(if (selected) 2.dp else 0.dp, if (selected) Beep.colors.ink else Color.Transparent, CircleShape)
            .clip(CircleShape)
            .background(icon.ground),
            contentAlignment = Alignment.Center) {
            // The foreground is a 108dp adaptive layer; launchers show the middle 72dp of it.
            Image(painterResource(icon.foreground), null, Modifier.requiredSize(66.dp))
        }
        Spacer(Modifier.height(4.dp))
        Text(label, fontSize = 11.sp, color = if (selected) Beep.colors.ink else Beep.colors.muted,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
    }
}
