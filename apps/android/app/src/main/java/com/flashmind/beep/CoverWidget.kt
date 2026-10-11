package com.flashmind.beep

import android.app.ActivityOptions
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews

/**
 * The widget on the Galaxy Z Flip's Flex Window. A tap opens Beep on the cover screen with the
 * keyboard up, and the chat moves to the inside screen when the phone opens.
 */
class CoverWidgetReceiver : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        val intent = Intent(context, MainActivity::class.java).setAction("com.flashmind.beep.COVER")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            .putExtra("focus_input", true)
        // Without a display the activity can open on the inside screen, which is off while the phone is closed.
        val options = ActivityOptions.makeBasic().setLaunchDisplayId(COVER_DISPLAY).toBundle()
        val open = PendingIntent.getActivity(context, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT, options)
        val views = RemoteViews(context.packageName, R.layout.cover_widget)
        views.setOnClickPendingIntent(R.id.cover_widget, open)
        manager.updateAppWidget(ids, views)
    }

    private companion object {
        /** Samsung numbers the cover screen 1 and the inside screen 0. */
        const val COVER_DISPLAY = 1
    }
}
