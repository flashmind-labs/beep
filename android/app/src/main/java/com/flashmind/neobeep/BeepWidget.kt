package com.flashmind.neobeep

import android.content.Context
import android.content.Intent
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.action.actionStartActivity
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Row
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import androidx.glance.ImageProvider
import androidx.glance.Image
import androidx.glance.layout.ContentScale
import androidx.glance.appwidget.cornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class BeepWidget : GlanceAppWidget() {
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        provideContent {
            val ink = Color(0xFF0A0E0C)
            val surface = Color(0xFF121916)
            val emerald = Color(0xFF3B9B7A)
            val muted = Color(0xFF8A9B93)
            val border = Color(0xFF243028)

            Box(
                modifier = GlanceModifier
                    .fillMaxSize()
                    .cornerRadius(24.dp)
                    .background(surface)
                    .clickable(actionStartActivity<MainActivity>()),
                contentAlignment = Alignment.Center
            ) {
                Row(
                    modifier = GlanceModifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Image(
                        provider = ImageProvider(R.drawable.ic_beep),
                        contentDescription = "Beep",
                        modifier = GlanceModifier.size(28.dp).padding(end = 12.dp)
                    )
                    Text(
                        text = "Ask Beep...",
                        style = TextStyle(
                            color = ColorProvider(muted),
                            fontSize = 16.sp,
                            fontWeight = FontWeight.Normal
                        ),
                        modifier = GlanceModifier.defaultWeight()
                    )
                }
            }
        }
    }
}

class BeepWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = BeepWidget()
}
