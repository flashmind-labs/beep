package com.flashmind.neobeep

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
    private const val CHANNEL = "assistant-chat"
    private const val SHORTCUT = "beep-assistant"
    fun canBubble(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (!manager.areNotificationsEnabled()) return false
        return manager.areBubblesAllowed() || manager.areBubblesEnabled()
    }
    fun show(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (!manager.areNotificationsEnabled()) return false
        val channel = NotificationChannel(CHANNEL, "Assistant conversation", NotificationManager.IMPORTANCE_HIGH)
        channel.description = "Keep your assistant in a floating conversation bubble"
        channel.setAllowBubbles(true)
        manager.createNotificationChannel(channel)
        // Check if bubbles are actually allowed after channel creation
        val existing = manager.getNotificationChannel(CHANNEL)
        if (existing != null && !existing.canBubble()) {
            // Bubbles not enabled for this channel — open settings
            settings(context)
            return false
        }
        val person = Person.Builder().setName("Beep").setBot(true).setImportant(true)
            .setIcon(Icon.createWithResource(context, R.drawable.ic_beep)).build()
        val user = Person.Builder().setName("You").setKey("self").setBot(false).build()
        val main = Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW)
        context.getSystemService(ShortcutManager::class.java).addDynamicShortcuts(listOf(
            ShortcutInfo.Builder(context, SHORTCUT).setShortLabel("Beep")
                .setIcon(Icon.createWithResource(context, R.drawable.ic_beep))
                // Include both participants; this is a human-to-assistant conversation.
                // A shortcut listing only bot participants is excluded from conversations.
                .setPersons(arrayOf(user, person)).setLongLived(true).setIntent(main).build()
        ))
        val intent = Intent(context, BubbleActivity::class.java).setAction(Intent.ACTION_VIEW)
            .setData(Uri.parse("neobeep://assistant"))
        val bubble = Notification.BubbleMetadata.Builder(
            // Android embeds this activity by adding launch options to the intent.
            // Bubble PendingIntents must be mutable; the target is always explicit.
            PendingIntent.getActivity(context, 1, intent, PendingIntent.FLAG_UPDATE_CURRENT or
                if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0),
            Icon.createWithResource(context, R.drawable.ic_beep)
        ).setDesiredHeight(440).setAutoExpandBubble(true).setSuppressNotification(true).build()
        manager.notify(1, Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_beep).setContentTitle("Beep")
            .setContentIntent(PendingIntent.getActivity(context, 0, main, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .setCategory(Notification.CATEGORY_MESSAGE).setShortcutId(SHORTCUT)
            .setStyle(Notification.MessagingStyle(user)
                .addMessage("Your assistant, a tap away.", System.currentTimeMillis(), person))
            .setBubbleMetadata(bubble).setOnlyAlertOnce(true).build())
        return true
    }
    fun settings(context: Context) {
        context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_BUBBLE_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName))
    }
}
