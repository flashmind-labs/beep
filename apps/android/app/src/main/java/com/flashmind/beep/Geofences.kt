package com.flashmind.beep

import android.Manifest
import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log
import androidx.core.content.ContextCompat
import com.flashmind.beep.proto.PlaceCrossing
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingEvent
import com.google.android.gms.location.GeofencingRequest
import com.google.android.gms.location.LocationServices
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withTimeoutOrNull
import com.flashmind.beep.proto.Place as ProtoPlace

/**
 * Watches the saved places with the phone's geofencing, so arriving or leaving reaches the server
 * even with the app closed. The phone allows 100 geofences per app.
 */
object Geofences {
    private const val MAX = 100
    private const val TAG = "Geofences"
    private const val PREFS = "geofences"
    /** What was last registered, so an unchanged list is not registered again. */
    private const val KEY = "registered"

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun intent(context: Context): PendingIntent =
        // Mutable, since the location service fills in which geofences fired.
        PendingIntent.getBroadcast(context, 0, Intent(context, GeofenceReceiver::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE)

    private fun allowed(context: Context) =
        ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

    /** Makes the next [sync] register again, as after a reboot or a new permission. */
    fun forget(context: Context) = prefs(context).edit().remove(KEY).apply()

    /** Registers one geofence per place, entering and leaving, replacing what was there. */
    @SuppressLint("MissingPermission")
    suspend fun sync(context: Context, places: List<ProtoPlace>) {
        val watched = places.take(MAX)
        val key = watched.joinToString(";") { "${it.id},${it.latitude},${it.longitude},${it.radius}" }
        if (!allowed(context)) { forget(context); return }
        if (prefs(context).getString(KEY, null) == key) return
        val client = LocationServices.getGeofencingClient(context)
        try {
            client.removeGeofences(intent(context)).await()
            if (watched.isNotEmpty()) {
                val fences = watched.map {
                    Geofence.Builder()
                        .setRequestId(it.id)
                        .setCircularRegion(it.latitude, it.longitude, it.radius.toFloat())
                        .setExpirationDuration(Geofence.NEVER_EXPIRE)
                        .setTransitionTypes(Geofence.GEOFENCE_TRANSITION_ENTER or Geofence.GEOFENCE_TRANSITION_EXIT)
                        .build()
                }
                // No initial trigger: the server already knows whether the phone is there.
                val request = GeofencingRequest.Builder().setInitialTrigger(0).addGeofences(fences).build()
                client.addGeofences(request, intent(context)).await()
            }
            prefs(context).edit().putString(KEY, key).apply()
        } catch (e: Exception) {
            // Location off or the permission taken away; the next sync tries again.
            Log.w(TAG, "Could not register geofences", e)
        }
    }
}

/** Tells the server about each place the phone arrived at or left. */
class GeofenceReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val event = GeofencingEvent.fromIntent(intent) ?: return
        if (event.hasError()) return
        val crossing = when (event.geofenceTransition) {
            Geofence.GEOFENCE_TRANSITION_ENTER -> PlaceCrossing.ARRIVE
            Geofence.GEOFENCE_TRANSITION_EXIT -> PlaceCrossing.LEAVE
            else -> return
        }
        val ids = event.triggeringGeofences.orEmpty().map { it.requestId }
        if (ids.isEmpty()) return
        val repository = (context.applicationContext as BeepApplication).repository
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                // A background broadcast has about a minute before Android stops it.
                withTimeoutOrNull(25_000L) {
                    ids.forEach { id -> runCatching { repository.crossPlace(id, crossing) } }
                }
            } finally {
                pending.finish()
            }
        }
    }
}

/** Geofences are gone after a reboot or an update, so this registers them again. */
class GeofenceRestoreReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val repository = (context.applicationContext as BeepApplication).repository
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                Geofences.forget(context)
                withTimeoutOrNull(25_000L) { repository.syncPlaces() }
            } finally {
                pending.finish()
            }
        }
    }
}
