package com.flashmind.beep

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import androidx.annotation.DrawableRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
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
import java.time.LocalDate
import java.time.Month

/** Launcher icons. Each one is an activity-alias in the manifest; only one is enabled at a time. */
enum class AppIcon(val key: String, val label: String, val alias: String, @DrawableRes val foreground: Int) {
    Beep("beep", "Beep", ".LauncherBeep", R.drawable.ic_launcher_foreground),
    Bubble("bubble", "Bubble", ".LauncherBubble", R.drawable.ic_launcher_bubble_foreground),
    Spark("spark", "Spark", ".LauncherSpark", R.drawable.ic_launcher_spark_foreground),
    Ghost("ghost", "Ghost", ".LauncherGhost", R.drawable.ic_launcher_ghost_foreground);

    companion object {
        fun fromKey(key: String?): AppIcon? = entries.firstOrNull { it.key == key }
    }
}

object AppIcons {
    private const val PREFS = "appearance"
    private const val KEY = "app_icon"
    private val ManifestDefault = AppIcon.Beep

    /** The user's pick, or null for automatic (the ghost in October, Beep otherwise). */
    val choice = mutableStateOf<AppIcon?>(null)
    private var loaded = false

    fun seasonal(today: LocalDate = LocalDate.now()): AppIcon =
        if (today.month == Month.OCTOBER) AppIcon.Ghost else AppIcon.Beep

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        choice.value = AppIcon.fromKey(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null))
    }

    fun set(context: Context, icon: AppIcon?) {
        choice.value = icon
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, icon?.key).apply()
        apply(context)
    }

    /** Enables the alias for the current icon and disables the rest, touching only aliases whose state changes. */
    fun apply(context: Context) {
        load(context)
        val wanted = choice.value ?: seasonal()
        val pm = context.packageManager
        // Enable first so the app is never left without a launcher entry.
        val ordered = AppIcon.entries.sortedByDescending { it == wanted }
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
    }
}

private val IconGround = Color(0xFF2F8A69)

/** Row of launcher icons. Auto follows the season; the others pin one icon. */
@Composable
fun AppIconPicker(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    remember { AppIcons.load(context.applicationContext) }
    val choice = AppIcons.choice.value
    Column(modifier) {
        Text("App icon", fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            IconOption("Auto", AppIcons.seasonal(), choice == null) { AppIcons.set(context, null) }
            AppIcon.entries.forEach { icon ->
                IconOption(icon.label, icon, choice == icon) { AppIcons.set(context, icon) }
            }
        }
        Spacer(Modifier.height(6.dp))
        Text("Auto uses the ghost in October. Your home screen may take a moment to update.",
            fontSize = 11.sp, color = Beep.colors.muted)
    }
}

@Composable
private fun IconOption(label: String, icon: AppIcon, selected: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.clip(CircleShape).clickable(onClick = onClick)) {
        Box(Modifier.size(44.dp)
            .border(if (selected) 2.dp else 0.dp, if (selected) Beep.colors.ink else Color.Transparent, CircleShape)
            .clip(CircleShape)
            .background(IconGround),
            contentAlignment = Alignment.Center) {
            // The foreground is a 108dp adaptive layer; launchers show the middle 72dp of it.
            Image(painterResource(icon.foreground), null, Modifier.requiredSize(66.dp))
        }
        Spacer(Modifier.height(4.dp))
        Text(label, fontSize = 11.sp, color = if (selected) Beep.colors.ink else Beep.colors.muted,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
    }
}
