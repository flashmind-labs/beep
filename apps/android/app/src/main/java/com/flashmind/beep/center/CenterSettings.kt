package com.flashmind.beep.center

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.Appearance
import com.flashmind.beep.Beep
import com.flashmind.beep.Skins

/** Settings from Home's gear: Appearance, Notifications and the linked devices. */
@Composable
internal fun CenterSettings(back: () -> Unit, openAppearance: () -> Unit, openNotifications: () -> Unit, openDevices: () -> Unit) {
    val choice = Appearance.choice.value
    Column(Modifier.fillMaxSize()) {
        CcTop("Settings", back)
        CcBody {
            CcPanel {
                CcRow("Appearance", chevron = true, onClick = openAppearance, right = {
                    Text(Skins.all.firstOrNull { it.id == choice }?.name ?: "Match system", fontSize = 11.5.sp, color = Beep.colors.muted)
                })
                CcRow("Notifications", chevron = true, onClick = openNotifications)
                CcRow("Devices", chevron = true, onClick = openDevices)
            }
        }
    }
}

@Composable
internal fun AppearanceSettings(back: () -> Unit) {
    val context = LocalContext.current
    val choice = Appearance.choice.value
    val swatches = listOf(Triple(Skins.SYSTEM, "Match system", listOf(Color(0xFF0F1011), Color(0xFF5E6AD2), Color(0xFFFAFAF9)))) +
        Skins.all.map { Triple(it.id, it.name, listOf(it.colors.page, it.colors.accent, it.colors.ok)) }
    Column(Modifier.fillMaxSize()) {
        CcTop("Appearance", back)
        CcBody {
            swatches.chunked(3).forEach { row ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEach { (id, name, colors) ->
                        val on = id == choice
                        val shape = RoundedCornerShape(Beep.shapes.radius)
                        Column(Modifier.weight(1f).clip(shape).border(if (on) 2.dp else 1.dp, if (on) Beep.colors.accent else Beep.colors.softLine, shape)
                            .clickable { Appearance.set(context, id) }.padding(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(Modifier.fillMaxWidth().height(34.dp).background(colors[0], RoundedCornerShape(Beep.shapes.smallRadius))
                                .border(1.dp, Color(0x40808080), RoundedCornerShape(Beep.shapes.smallRadius)).padding(5.dp),
                                horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.Bottom) {
                                colors.drop(1).forEach { Box(Modifier.size(10.dp).background(it, RoundedCornerShape(3.dp))) }
                            }
                            Text(name, fontSize = 12.sp, maxLines = 1)
                        }
                    }
                    repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
            CcNote("Match system uses Linear in dark mode and Light in light mode.")
        }
    }
}

@Composable
internal fun NotifySettings(back: () -> Unit, askPermission: () -> Unit) {
    val context = LocalContext.current
    var asks by remember { mutableStateOf(NotifyPrefs.on(context, NotifyPrefs.ASKS)) }
    var finished by remember { mutableStateOf(NotifyPrefs.on(context, NotifyPrefs.FINISHED)) }
    var ci by remember { mutableStateOf(NotifyPrefs.on(context, NotifyPrefs.CI)) }
    var stay by remember { mutableStateOf(NotifyPrefs.on(context, NotifyPrefs.STAY)) }
    fun set(key: String, on: Boolean) {
        NotifyPrefs.set(context, key, on)
        if (on && !NotifyPrefs.allowed(context)) askPermission()
    }
    Column(Modifier.fillMaxSize()) {
        CcTop("Notifications", back)
        CcBody {
            CcPanel {
                CcRow("Agent asks", "Allow and Deny on the notification", right = { CcToggle(asks, "Agent asks") { asks = it; set(NotifyPrefs.ASKS, it) } })
                CcRow("Agent finished", right = { CcToggle(finished, "Agent finished") { finished = it; set(NotifyPrefs.FINISHED, it) } })
                CcRow("Builds that ended", right = { CcToggle(ci, "Builds that ended") { ci = it; set(NotifyPrefs.CI, it) } })
            }
            CcPanel {
                CcRow("Stay connected", "Asks arrive while Beep is closed. Android shows a lasting notification.",
                    right = { CcToggle(stay, "Stay connected") {
                        stay = it
                        set(NotifyPrefs.STAY, it)
                        if (it) StayConnectedService.start(context) else StayConnectedService.stop(context)
                    } })
            }
        }
    }
}
