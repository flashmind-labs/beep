package com.flashmind.beep

import android.app.*
import android.content.Context
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.provider.Settings

object Bubbles {
    private const val CHANNEL = "beep-bubble"
    private const val SHORTCUT = "beep-assistant"
    fun show(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (!manager.areNotificationsEnabled()) return false
        if (manager.getNotificationChannel(CHANNEL) == null) {
            val channel = NotificationChannel(CHANNEL, "Assistant conversation", NotificationManager.IMPORTANCE_DEFAULT)
            channel.description = "Keep your assistant in a floating conversation bubble"
            channel.setAllowBubbles(true)
            manager.createNotificationChannel(channel)
        }
        if (!bubblesAllowed(manager)) return false
        val person = Person.Builder().setName("Beep").setBot(true).setImportant(true)
            .setIcon(Icon.createWithResource(context, R.drawable.ic_beep)).build()
        val user = Person.Builder().setName("You").setKey("self").setBot(false).build()
        val main = Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW)
        context.getSystemService(ShortcutManager::class.java).addDynamicShortcuts(listOf(
            ShortcutInfo.Builder(context, SHORTCUT).setShortLabel("Beep")
                .setIcon(Icon.createWithResource(context, R.drawable.ic_beep))
                .setPersons(arrayOf(user, person)).setLongLived(true).setIntent(main).build()
        ))
        val intent = Intent(context, BubbleActivity::class.java).setAction(Intent.ACTION_VIEW)
            .setData(Uri.parse("beep://assistant"))
        val bubble = Notification.BubbleMetadata.Builder(
            PendingIntent.getActivity(context, 1, intent, PendingIntent.FLAG_UPDATE_CURRENT or
                if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0),
            Icon.createWithResource(context, R.drawable.ic_beep)
        ).setDesiredHeight(440).setAutoExpandBubble(true).setSuppressNotification(true).build()
        manager.notify(1, Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome).setContentTitle("Beep")
            .setContentIntent(PendingIntent.getActivity(context, 0, main, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .setCategory(Notification.CATEGORY_MESSAGE).setShortcutId(SHORTCUT)
            .setStyle(Notification.MessagingStyle(user)
                .addMessage("Your assistant, a tap away.", System.currentTimeMillis(), person))
            .setBubbleMetadata(bubble).setOnlyAlertOnce(true).build())
        return true
    }
    // Without the app-level bubble permission Android posts a plain notification and nothing floats.
    private fun bubblesAllowed(manager: NotificationManager): Boolean =
        if (Build.VERSION.SDK_INT >= 31) manager.bubblePreference != NotificationManager.BUBBLE_PREFERENCE_NONE
        else @Suppress("DEPRECATION") manager.areBubblesAllowed()
    fun settings(context: Context) {
        context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_BUBBLE_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName))
    }
}
