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
 * even with the app closed. Each place has two geofences: arriving at its radius, and leaving at
 * a wider ring, as the server's own check does, so GPS drifting indoors does not count as leaving.
 * The phone allows 100 geofences per app.
 */
object Geofences {
    private const val MAX = 50
    /** Ends the request id of a place's leaving geofence. */
    private const val LEAVE = ":leave"
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

    /** How far from the middle of a place the phone must be to have left it, in metres. */
    internal fun leaveRadius(radius: Float) = radius + maxOf(radius / 2, 50f)

    /** The place a geofence's request id names. */
    internal fun placeOf(requestId: String) = requestId.removeSuffix(LEAVE)

    /** Registers the arriving and leaving geofences of each place, replacing what was there. */
    @SuppressLint("MissingPermission")
    suspend fun sync(context: Context, places: List<ProtoPlace>) {
        val watched = places.take(MAX)
        val key = "ring;" + watched.joinToString(";") { "${it.id},${it.latitude},${it.longitude},${it.radius}" }
        if (!allowed(context)) { forget(context); return }
        if (prefs(context).getString(KEY, null) == key) return
        val client = LocationServices.getGeofencingClient(context)
        try {
            client.removeGeofences(intent(context)).await()
            if (watched.isNotEmpty()) {
                fun fence(id: String, place: ProtoPlace, radius: Float, transition: Int) = Geofence.Builder()
                    .setRequestId(id)
                    .setCircularRegion(place.latitude, place.longitude, radius)
                    .setExpirationDuration(Geofence.NEVER_EXPIRE)
                    .setTransitionTypes(transition)
                    .build()
                val fences = watched.flatMap {
                    val radius = it.radius.toFloat()
                    listOf(fence(it.id, it, radius, Geofence.GEOFENCE_TRANSITION_ENTER),
                        fence(it.id + LEAVE, it, leaveRadius(radius), Geofence.GEOFENCE_TRANSITION_EXIT))
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
        val ids = event.triggeringGeofences.orEmpty().map { Geofences.placeOf(it.requestId) }.distinct()
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
