package com.flashmind.beep

import android.content.Context
import android.content.Intent
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.action.actionStartActivity
import androidx.glance.appwidget.action.actionStartActivity
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
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import androidx.glance.ImageProvider
import androidx.glance.Image
import androidx.glance.appwidget.cornerRadius
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class BeepWidget : GlanceAppWidget() {
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        provideContent {
            val c = BeepTokens.of(BeepStyle.MintQuiet,
                context.resources.configuration.uiMode and android.content.res.Configuration.UI_MODE_NIGHT_MASK == android.content.res.Configuration.UI_MODE_NIGHT_YES).colors
            val ink = c.ink
            val surface = c.surface
            val emerald = c.accent
            val muted = c.muted
            val border = c.softLine

            Box(
                modifier = GlanceModifier
                    .fillMaxSize()
                    .cornerRadius(8.dp)
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
                    // Talks live instead of typing.
                    Box(
                        modifier = GlanceModifier
                            .size(40.dp)
                            .cornerRadius(20.dp)
                            .background(emerald)
                            .clickable(actionStartActivity(Intent(context, MainActivity::class.java)
                                .setAction(LIVE_ACTION).putExtra("origin", LiveOrigin.Widget.name))),
                        contentAlignment = Alignment.Center
                    ) {
                        Image(
                            provider = ImageProvider(R.drawable.ic_wave),
                            contentDescription = "Talk live",
                            modifier = GlanceModifier.size(20.dp)
                        )
                    }
                }
            }
        }
    }
}

class BeepWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = BeepWidget()
}
