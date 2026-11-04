package com.flashmind.beep

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.NotificationsNone
import androidx.compose.material.icons.outlined.Place
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.OffsetDateTime
import java.time.Instant
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import java.time.ZonedDateTime
import com.flashmind.beep.proto.Place as ProtoPlace

/** A spot the user saved by telling Beep where they are. */
data class Place(
    val id: String,
    val name: String,
    val address: String,
    val radiusMeters: Int,
    /** How many routines start when the user arrives at or leaves it. */
    val routines: Int,
    /** When the phone last saw the user there, in epoch milliseconds; null if never. */
    val lastHere: Long?,
    val here: Boolean,
)

/** Where the places screens read and change saved places. */
interface PlaceStore {
    suspend fun list(): List<Place>
    suspend fun delete(id: String)
    suspend fun rename(id: String, name: String)
    suspend fun setRadius(id: String, meters: Int)
}

/** The places on the server, through the repository. */
class ServerPlaces(private val repository: ChatRepository) : PlaceStore {
    override suspend fun list(): List<Place> = repository.listPlaces()
    override suspend fun delete(id: String) = repository.deletePlace(id)
    override suspend fun rename(id: String, name: String) = repository.renamePlace(id, name)
    override suspend fun setRadius(id: String, meters: Int) = repository.setPlaceRadius(id, meters)
}

internal fun appPlace(place: ProtoPlace) = Place(
    id = place.id,
    name = place.name,
    address = place.address.orEmpty(),
    radiusMeters = place.radius.toInt(),
    routines = place.routines.toInt(),
    lastHere = place.lastHereAt?.let { runCatching { OffsetDateTime.parse(it).toInstant().toEpochMilli() }.getOrNull() },
    here = place.here,
)

/** Arriving at a place or leaving it, as a place routine's schedule says. */
enum class PlaceEvent(val word: String) { Arrive("arrive"), Leave("leave") }

/** How often a place routine starts, as its schedule ends: nothing, `:daily` or `:once`. */
enum class PlaceRepeat(val suffix: String?, val title: String, val hint: String, val note: String?) {
    Always(null, "Every time", "Runs on every visit", null),
    Daily("daily", "Once a day", "Skips it if you come back the same day", "once a day at most"),
    Once("once", "Next time only", "Runs the next time, then deletes itself", "next time only"),
}

/** A place routine's schedule: `arrive:<place id>` or `leave:<place id>`, ending as [PlaceRepeat] says. */
data class PlaceSchedule(val event: PlaceEvent, val placeId: String, val repeat: PlaceRepeat) {
    override fun toString() = "${event.word}:$placeId" + (repeat.suffix?.let { ":$it" } ?: "")
}

/** Reads a place routine's schedule, or null for a time-based one. */
internal fun placeSchedule(schedule: String): PlaceSchedule? {
    val parts = schedule.trim().split(":")
    val event = PlaceEvent.entries.firstOrNull { it.word == parts.first() } ?: return null
    if (parts.size !in 2..3 || parts[1].isBlank()) return null
    val repeat = if (parts.size == 2) PlaceRepeat.Always
        else PlaceRepeat.entries.firstOrNull { it.suffix != null && it.suffix == parts[2] } ?: return null
    return PlaceSchedule(event, parts[1], repeat)
}

/** "When you arrive at Gym · once a day at most", naming the place from [names] by id. */
internal fun describePlaceSchedule(schedule: PlaceSchedule, names: Map<String, String>): String {
    val place = names[schedule.placeId] ?: "a saved place"
    val event = when (schedule.event) { PlaceEvent.Arrive -> "arrive at"; PlaceEvent.Leave -> "leave" }
    return "When you $event $place" + (schedule.repeat.note?.let { " · $it" } ?: "")
}

/** "Within 100 m · 1 routine · last here Thu", as a place's row says it. */
internal fun placeSummary(place: Place, now: ZonedDateTime = ZonedDateTime.now()): String {
    val routines = when (place.routines) { 0 -> "no routines"; 1 -> "1 routine"; else -> "${place.routines} routines" }
    val last = when {
        place.here -> "here now"
        place.lastHere != null -> "last here ${listTime(place.lastHere, now)}"
        else -> null
    }
    return listOfNotNull("Within ${place.radiusMeters} m", routines, last).joinToString(" · ")
}

/** True when the phone may read location with the app closed, which noticing arrivals needs. */
internal fun locationAlways(context: Context) =
    ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION) == PackageManager.PERMISSION_GRANTED

/** The places Beep knows, each with what uses it. Places are added from the chat, not here. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PlacesScreen(store: PlaceStore, onBack: () -> Unit, onLocationAlways: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var places by remember { mutableStateOf<List<Place>?>(null) }
    var deleting by remember { mutableStateOf<Place?>(null) }
    var error by remember { mutableStateOf("") }
    fun refresh() { scope.launch {
        runCatching { withContext(Dispatchers.IO) { store.list() } }.onSuccess { places = it; error = "" }.onFailure { error = userMessage(it); places = places ?: emptyList() }
    } }
    LaunchedEffect(Unit) { refresh() }
    deleting?.let { place ->
        AlertDialog(onDismissRequest = { deleting = null },
            confirmButton = { TextButton(onClick = {
                deleting = null
                scope.launch { runCatching { withContext(Dispatchers.IO) { store.delete(place.id) } }.onFailure { error = userMessage(it) }; refresh() }
            }) { Text("Delete", color = Beep.colors.error) } },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("Cancel") } },
            title = { Text("Delete ${place.name}?") },
            text = { Text(if (place.routines > 0) "Routines that start here stop. Beep forgets this place." else "Beep forgets this place.") },
            containerColor = Beep.colors.surface)
    }
    Scaffold(containerColor = Beep.colors.page, topBar = {
        TopAppBar(modifier = Modifier.beepBar(), expandedHeight = 54.dp, title = { Text(displayText("Places"), style = displayStyle(18.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Beep.colors.page))
    }) { padding ->
        val list = places
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (error.isNotEmpty()) item { Text(error, color = Beep.colors.error, fontSize = 12.sp) }
            if (list == null) item { Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) } }
            else if (list.isEmpty()) item {
                Text("No places yet.\nBeep can remind you when you arrive somewhere, or leave.", Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 40.dp, bottom = 8.dp),
                    textAlign = TextAlign.Center, color = Beep.colors.muted, lineHeight = 22.sp)
            } else {
                if (!locationAlways(context)) item {
                    Row(Modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.raised).clickable(onClick = onLocationAlways)
                        .padding(BeepSpace.row), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Icon(Icons.Outlined.Place, null, Modifier.size(20.dp))
                        Text("Beep can't notice arrivals yet. Allow location all the time.", Modifier.weight(1f), fontSize = 13.sp, lineHeight = 18.sp)
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = Beep.colors.muted)
                    }
                }
                items(list, key = { it.id }) { place -> PlaceRow(place) { deleting = place } }
            }
            if (list != null) item { PlacesHint() }
        }
    }
}

@Composable
private fun PlaceRow(place: Place, onDelete: () -> Unit) {
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(start = 14.dp, top = 12.dp, bottom = 12.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            PlaceLogo(Beep.colors.tints[place.name.hashCode().mod(Beep.colors.tints.size)])
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(place.name, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (place.address.isNotBlank()) Text(place.address, fontSize = 12.5.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(placeSummary(place), fontSize = 11.5.sp, color = Beep.colors.dim)
            }
            IconButton(onClick = onDelete) { Icon(Icons.Outlined.Close, "Delete ${place.name}", tint = Beep.colors.muted) }
        }
    }
}

@Composable
internal fun PlaceLogo(tint: Color, size: Int = 40) {
    Box(Modifier.size(size.dp).background(tint, RoundedCornerShape(Beep.shapes.smallRadius)), contentAlignment = Alignment.Center) {
        Icon(Icons.Outlined.Place, null, Modifier.size((size / 2).dp), tint = Beep.colors.ink)
    }
}

@Composable
private fun PlacesHint() {
    val text = buildAnnotatedString {
        append("Tell Beep where you are to add one, like ")
        withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append("\"I'm at the gym\"") }
        append(" or ")
        withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append("\"this is home\"") }
        append(".")
    }
    BeepCard(Modifier.fillMaxWidth(), color = Beep.colors.tints[1]) {
        Text(text, Modifier.padding(BeepSpace.row), fontSize = 13.sp, lineHeight = 19.sp, color = Beep.colors.ink)
    }
}

/**
 * Asks for location all the time, which place routines need to run with the app closed. Fine
 * location comes first, since Android only offers all the time on top of it. When Android won't
 * show its page any more, the button goes to the app's settings instead.
 */
@Composable
internal fun BackgroundLocationScreen(onDone: () -> Unit, onPlaces: () -> Unit) {
    val context = LocalContext.current
    var allowed by remember { mutableStateOf(locationAlways(context)) }
    var sentToSettings by remember { mutableStateOf(false) }
    // A new grant means the geofences can go in, or now fire with the app closed.
    fun registerAgain() {
        Geofences.forget(context)
        val repository = (context.applicationContext as BeepApplication).repository
        CoroutineScope(Dispatchers.IO).launch { repository.syncPlaces() }
    }
    fun openSettings() {
        sentToSettings = true
        context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
    val always = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        allowed = granted
        if (granted) registerAgain() else sentToSettings = true
    }
    val fine = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        if (result[Manifest.permission.ACCESS_FINE_LOCATION] == true) {
            registerAgain()
            always.launch(Manifest.permission.ACCESS_BACKGROUND_LOCATION)
        } else sentToSettings = true
    }
    // Coming back from settings, check again.
    LifecycleResumeEffect(Unit) {
        val now = locationAlways(context)
        if (now && !allowed) registerAgain()
        allowed = now
        onPauseOrDispose {}
    }
    Column(Modifier.fillMaxSize().background(Beep.colors.page).systemBarsPadding().padding(horizontal = 20.dp, vertical = 28.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if (allowed) {
            Spacer(Modifier.weight(1f))
            Box(Modifier.size(64.dp).background(Beep.colors.tints[1], RoundedCornerShape(Beep.shapes.radius)).align(Alignment.CenterHorizontally), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Check, null, tint = Beep.colors.ok)
            }
            Text(displayText("Beep will notice when you arrive."), style = displayStyle(24.sp), textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
            Text("Your places work with the app closed. Turn it off any time in Settings, Apps, Beep, Location.", color = Beep.colors.muted,
                textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp))
            ChunkyButton(onClick = onPlaces, modifier = Modifier.fillMaxWidth().padding(top = 12.dp), color = Beep.colors.accent) { Text("See your places", fontWeight = FontWeight.SemiBold) }
            Spacer(Modifier.weight(1f))
            return@Column
        }
        PlaceLogo(Beep.colors.tints[0], 56)
        Text(displayText("Notice when you arrive?"), style = displayStyle(24.sp))
        Text(buildAnnotatedString {
            append("For routines like \"when I get to the gym\", the phone watches your saved places and tells Beep when you arrive or leave. That needs location set to ")
            withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = Beep.colors.ink)) { append("Allow all the time") }
            append(".")
        }, color = Beep.colors.muted, lineHeight = 22.sp)
        BeepCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(BeepSpace.row), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                ExplainLine(Icons.Outlined.Place, "Works with the app closed, only for places you saved.")
                ExplainLine(Icons.Outlined.NotificationsNone, "You only get a notification when a routine has something to say.")
                ExplainLine(Icons.Outlined.Shield, "Uses the phone's geofencing, so the battery cost is small.")
            }
        }
        if (sentToSettings) BeepCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(BeepSpace.row), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Android asks for this in Settings", fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                Text("1. Tap Open settings", fontSize = 13.sp)
                Text("2. Permissions, then Location", fontSize = 13.sp)
                Text("3. Choose Allow all the time", fontSize = 13.sp)
            }
        }
        Spacer(Modifier.weight(1f))
        ChunkyButton(onClick = {
            val hasFine = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
            when {
                sentToSettings -> openSettings()
                hasFine -> always.launch(Manifest.permission.ACCESS_BACKGROUND_LOCATION)
                else -> fine.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
            }
        }, modifier = Modifier.fillMaxWidth(), color = Beep.colors.accent) {
            Text(if (sentToSettings) "Open settings" else "Allow all the time", fontWeight = FontWeight.SemiBold)
        }
        TextButton(onClick = onDone, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text("Not now", color = Beep.colors.muted) }
    }
}

@Composable
private fun ExplainLine(icon: ImageVector, text: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Icon(icon, null, Modifier.size(18.dp), tint = Beep.colors.muted)
        Text(text, fontSize = 13.sp, lineHeight = 19.sp)
    }
}

/** When something last changed: "09:41" today, then "Yesterday", "Thu" this week, "3 Oct". */
internal fun listTime(millis: Long, now: ZonedDateTime = ZonedDateTime.now()): String {
    val at = Instant.ofEpochMilli(millis).atZone(now.zone)
    val days = ChronoUnit.DAYS.between(at.toLocalDate(), now.toLocalDate())
    return when {
        days <= 0L -> at.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
        days == 1L -> "Yesterday"
        days < 7L -> at.format(DateTimeFormatter.ofPattern("EEE", Locale.getDefault()))
        at.year == now.year -> at.format(DateTimeFormatter.ofPattern("d MMM"))
        else -> at.format(DateTimeFormatter.ofPattern("d MMM yyyy"))
    }
}
