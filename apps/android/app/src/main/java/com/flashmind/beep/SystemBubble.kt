package com.flashmind.beep

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings

/** Floats the chat on screen as an Android bubble, through the chat's own conversation notification. */
object SystemBubble {
    fun show(context: Context): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (!manager.areNotificationsEnabled()) return false
        ChatNotifications.ensureChannel(context)
        if (!bubblesAllowed(manager)) return false
        val chat = (context.applicationContext as BeepApplication).repository.state.value.current
        ChatNotifications.show(context, chat, expand = true)
        return true
    }
    // Without the app-level bubble permission Android posts a plain notification and nothing floats.
    private fun bubblesAllowed(manager: NotificationManager): Boolean =
        if (Build.VERSION.SDK_INT >= 31) manager.bubblePreference != NotificationManager.BUBBLE_PREFERENCE_NONE
        else @Suppress("DEPRECATION") manager.areBubblesAllowed()
    private fun bubbles(context: Context) = context.getSystemService(NotificationManager::class.java).activeNotifications
        .filter { it.notification.flags and Notification.FLAG_BUBBLE != 0 }
    /** Take the bubbles down, for when the overlay bubble replaces them. */
    fun hide(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        bubbles(context).forEach { manager.cancel(it.tag, it.id) }
    }
    fun shown(context: Context) = bubbles(context).isNotEmpty()
    fun settings(context: Context) {
        context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_BUBBLE_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName))
    }
}
