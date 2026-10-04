package com.flashmind.beep

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Paint
import android.location.Geocoder
import android.net.Uri
import android.util.LruCache
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Launch
import androidx.compose.material.icons.outlined.Alarm
import androidx.compose.material.icons.outlined.Call
import androidx.compose.material.icons.outlined.Directions
import androidx.compose.material.icons.outlined.Email
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.MyLocation
import androidx.compose.material.icons.outlined.Place
import androidx.compose.material.icons.outlined.Sms
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.tan

/** What a finished tool call shows in the chat instead of a bare chip. */
sealed class ToolCard {
    /** [near] centers a search, such as "italian restaurant" around the user. */
    data class Place(val name: String, val query: String, val directions: Boolean, val near: LatLon? = null) : ToolCard()
    data class Here(val latitude: Double, val longitude: Double) : ToolCard()
    data class Contacts(val people: List<Contact>) : ToolCard()
    data class OpenedApp(val app: String, val uri: String, val fallbackUrl: String) : ToolCard()
    data class Link(val url: String) : ToolCard()
    data class Alarm(val alarm: AlarmRequest) : ToolCard()
    data class Replies(val replies: List<String>, val choice: Boolean) : ToolCard()
}

data class Contact(val name: String, val phone: String, val email: String)

private fun JSONObject.text(key: String) = optString(key).takeIf { it.isNotBlank() && it != "null" }

/** Reads the card a tool's output describes, or null when it is just text for the model. */
fun cardFor(call: ToolCall): ToolCard? {
    if (!call.isDone || !call.success) return null
    val json = runCatching { JSONObject(call.result) }.getOrNull() ?: return null
    return when (call.name) {
        "suggest_replies" -> {
            val list = json.optJSONArray("replies") ?: return null
            val replies = (0 until list.length()).mapNotNull { list.optString(it).takeIf(String::isNotBlank) }
            if (replies.isEmpty()) null else ToolCard.Replies(replies, json.optString("kind") == "choice")
        }
        "open_maps" -> placeOf(json)
        "get_location" -> {
            val lat = json.optDouble("latitude", Double.NaN)
            val lon = json.optDouble("longitude", Double.NaN)
            if (lat.isNaN() || lon.isNaN()) null else ToolCard.Here(lat, lon)
        }
        "query_contacts" -> {
            val list = json.optJSONArray("contacts") ?: return null
            val people = (0 until list.length()).mapNotNull { i ->
                val c = list.optJSONObject(i) ?: return@mapNotNull null
                Contact(c.optString("name"), firstOf(c.optJSONArray("phones"), "number"), firstOf(c.optJSONArray("emails"), "address"))
            }
            if (people.isEmpty()) null else ToolCard.Contacts(people)
        }
        "open_app" -> json.text("app")?.let { ToolCard.OpenedApp(it, json.optString("uri"), json.optString("fallback_url")) }
        "open_browser" -> json.text("url")?.let { ToolCard.Link(it) }
        "set_alarm" -> AlarmRequest.from(json)?.let { ToolCard.Alarm(it) }
        else -> null
    }
}

private fun firstOf(list: JSONArray?, key: String): String {
    val first = list?.opt(0) ?: return ""
    return if (first is JSONObject) first.optString(key).ifBlank { first.optString("value") } else first.toString()
}

/** Handles both the `place` object and the older `uri` plus nested card shape. */
private fun placeOf(json: JSONObject): ToolCard.Place? {
    json.optJSONObject("place")?.let { p ->
        val directions = p.optString("mode") == "directions"
        val query = (if (directions) p.text("destination") else null) ?: p.text("query") ?: p.text("name") ?: return null
        val near = p.optJSONObject("near")?.let { n ->
            val lat = n.optDouble("latitude", Double.NaN)
            val lon = n.optDouble("longitude", Double.NaN)
            if (lat.isNaN() || lon.isNaN()) null else LatLon(lat, lon, "")
        }
        return ToolCard.Place(p.text("name") ?: query, query, directions, near)
    }
    val uri = json.optString("uri")
    val directions = uri.startsWith("google.navigation:")
    val query = Uri.decode(Regex("q=([^&]+)").find(uri)?.groupValues?.get(1)?.replace('+', ' ') ?: return null)
    val name = json.optJSONObject("card")?.optJSONObject("card_data")?.text("name") ?: query
    return ToolCard.Place(name, query, directions)
}

data class LatLon(val latitude: Double, val longitude: Double, val address: String)

/** Finds places with the phone's own geocoder, so no map key is needed. */
private object Places {
    private val found = LruCache<String, LatLon>(64)

    @Suppress("DEPRECATION")
    /** Within about 20 km of [near] when given, so a search finds places close by. */
    suspend fun lookup(context: Context, query: String, near: LatLon? = null): LatLon? {
        val key = near?.let { "$query@%.2f,%.2f".format(java.util.Locale.ROOT, it.latitude, it.longitude) } ?: query
        return found.get(key) ?: withContext(Dispatchers.IO) {
            val system = if (Geocoder.isPresent()) {
                runCatching {
                    val geocoder = Geocoder(context)
                    if (near == null) geocoder.getFromLocationName(query, 1)?.firstOrNull()
                    else geocoder.getFromLocationName(query, 1, near.latitude - SPAN, near.longitude - SPAN,
                        near.latitude + SPAN, near.longitude + SPAN)?.firstOrNull()
                }.getOrNull()?.let { LatLon(it.latitude, it.longitude, it.getAddressLine(0).orEmpty()) }
            } else null
            (system ?: nominatim(query, near))?.also { found.put(key, it) }
        }
    }

    private const val SPAN = 0.2

    /** OpenStreetMap search, for phones without a working system geocoder. */
    private fun nominatim(query: String, near: LatLon?): LatLon? = runCatching {
        val box = near?.let {
            "&bounded=1&viewbox=${it.longitude - SPAN},${it.latitude + SPAN},${it.longitude + SPAN},${it.latitude - SPAN}"
        }.orEmpty()
        val url = URL("https://nominatim.openstreetmap.org/search?format=json&limit=1$box&q=" + URLEncoder.encode(query, "UTF-8"))
        val connection = url.openConnection() as HttpURLConnection
        connection.connectTimeout = 8000
        connection.readTimeout = 8000
        connection.setRequestProperty("User-Agent", "Beep for Android")
        val body = try { connection.inputStream.bufferedReader().use { it.readText() } } finally { connection.disconnect() }
        JSONArray(body).optJSONObject(0)?.let {
            LatLon(it.getString("lat").toDouble(), it.getString("lon").toDouble(), it.optString("display_name"))
        }
    }.getOrNull()

    @Suppress("DEPRECATION")
    suspend fun addressAt(context: Context, latitude: Double, longitude: Double): String = withContext(Dispatchers.IO) {
        if (!Geocoder.isPresent()) return@withContext ""
        runCatching { Geocoder(context).getFromLocation(latitude, longitude, 1)?.firstOrNull()?.getAddressLine(0) }
            .getOrNull().orEmpty()
    }
}

/**
 * A still map around one point, stitched from OpenStreetMap raster tiles. Tiles
 * are 256px, fetched one zoom level deeper so they read sharp on dense screens.
 * Dark themes get the same tiles inverted and dimmed. Kept in memory for the session.
 */
private object MapTiles {
    private const val TILE = 256
    private val tiles = LruCache<String, Bitmap>(60)
    private val night = Paint().apply {
        colorFilter = ColorMatrixColorFilter(
            ColorMatrix(
                floatArrayOf(
                    -0.75f, 0f, 0f, 0f, 215f,
                    0f, -0.75f, 0f, 0f, 220f,
                    0f, 0f, -0.75f, 0f, 218f,
                    0f, 0f, 0f, 1f, 0f,
                ),
            ).apply { postConcat(ColorMatrix().apply { setSaturation(0.35f) }) },
        )
    }

    suspend fun snapshot(latitude: Double, longitude: Double, zoom: Int, width: Int, height: Int, dark: Boolean): Bitmap? =
        withContext(Dispatchers.IO) {
            val z = zoom + 1
            val world = TILE * 2.0.pow(z)
            val lat = Math.toRadians(latitude.coerceIn(-85.0, 85.0))
            val cx = (longitude + 180) / 360 * world
            val cy = (1 - ln(tan(lat) + 1 / cos(lat)) / PI) / 2 * world
            val left = cx - width / 2.0
            val top = cy - height / 2.0
            val count = 2.0.pow(z).toInt()
            val out = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(out)
            var drawn = 0
            for (tx in floor(left / TILE).toInt()..floor((left + width) / TILE).toInt()) {
                for (ty in floor(top / TILE).toInt()..floor((top + height) / TILE).toInt()) {
                    if (ty !in 0 until count) continue
                    val tile = tile(z, Math.floorMod(tx, count), ty) ?: continue
                    canvas.drawBitmap(tile, (tx * TILE - left).toFloat(), (ty * TILE - top).toFloat(), if (dark) night else null)
                    drawn++
                }
            }
            if (drawn > 0) out else null
        }

    private fun tile(z: Int, x: Int, y: Int): Bitmap? {
        val key = "$z/$x/$y"
        tiles.get(key)?.let { return it }
        return runCatching {
            val connection = URL("https://tile.openstreetmap.org/$z/$x/$y.png").openConnection() as HttpURLConnection
            connection.connectTimeout = 8000
            connection.readTimeout = 8000
            connection.setRequestProperty("User-Agent", "Beep for Android")
            try { connection.inputStream.use(BitmapFactory::decodeStream) } finally { connection.disconnect() }
        }.getOrNull()?.also { tiles.put(key, it) }
    }
}

/** The map part of a place or location card, with a pin on the spot. */
@Composable
private fun MapSnippet(point: LatLon?, height: Int, onOpen: () -> Unit) {
    val t = LocalBeep.current
    val dark = t.colors.page.luminance() < 0.4f
    BoxWithConstraints(Modifier.fillMaxWidth().height(height.dp).background(t.colors.raised).clickable(onClick = onOpen)) {
        val density = LocalDensity.current
        val widthPx = with(density) { maxWidth.roundToPx() }.coerceIn(1, 1400)
        val heightPx = with(density) { maxHeight.roundToPx() }.coerceIn(1, 900)
        val map by produceState<Bitmap?>(null, point, widthPx, dark) {
            value = point?.let { MapTiles.snapshot(it.latitude, it.longitude, 15, widthPx, heightPx, dark) }
        }
        map?.let { Image(it.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
            ?: Icon(Icons.Outlined.Map, null, Modifier.align(Alignment.Center).size(28.dp), tint = t.colors.dim)
        if (map != null) {
            Box(Modifier.align(Alignment.Center).offset(y = (-14).dp).size(30.dp)
                .border(2.5.dp, t.colors.page, CircleShape).background(t.colors.accent, CircleShape),
                contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Place, null, Modifier.size(17.dp), tint = t.colors.onAccent)
            }
            Text("© OpenStreetMap", fontSize = 8.sp, color = t.colors.muted,
                modifier = Modifier.align(Alignment.BottomEnd).background(t.colors.page.copy(alpha = 0.7f))
                    .padding(horizontal = 4.dp, vertical = 1.dp))
        }
    }
}

private fun openMaps(context: Context, query: String, point: LatLon?, directions: Boolean, near: LatLon? = null) {
    val uri = when {
        directions -> "google.navigation:q=${Uri.encode(query)}"
        // Maps runs the search itself around the user rather than jumping to one geocoded hit.
        near != null -> "geo:${near.latitude},${near.longitude}?q=${Uri.encode(query)}"
        point != null -> "geo:${point.latitude},${point.longitude}?q=${Uri.encode(query)}"
        else -> "geo:0,0?q=${Uri.encode(query)}"
    }
    AppLauncher.launch(context, "maps", uri, "https://www.google.com/maps/search/?api=1&query=${Uri.encode(query)}")
}

@Composable
private fun PlaceMapCard(card: ToolCard.Place, compact: Boolean) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val found by produceState<LatLon?>(null, card.query, card.near) { value = Places.lookup(context, card.query, card.near) }
    val point = found ?: card.near
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        MapSnippet(point, if (compact) 110 else 150) { openMaps(context, card.query, point, directions = false, card.near) }
        Column(Modifier.padding(14.dp)) {
            Text(card.name, style = displayStyle(if (compact) 15.sp else 17.sp), color = t.colors.ink,
                maxLines = 2, overflow = TextOverflow.Ellipsis)
            val address = found?.address.orEmpty()
            if (address.isNotBlank() && address != card.name) {
                Spacer(Modifier.height(3.dp))
                Text(address, fontSize = 13.sp, color = t.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(12.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                CardButton("Directions", Icons.Outlined.Directions, primary = true) { openMaps(context, card.query, point, directions = true) }
                CardButton("Open in Maps", Icons.Outlined.Map) { openMaps(context, card.query, point, directions = false, card.near) }
            }
        }
    }
}

@Composable
private fun HereCard(card: ToolCard.Here, compact: Boolean) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val point = remember(card) { LatLon(card.latitude, card.longitude, "") }
    val address by produceState("", card) { value = Places.addressAt(context, card.latitude, card.longitude) }
    val label = "%.5f, %.5f".format(card.latitude, card.longitude)
    BeepCard(Modifier.fillMaxWidth()) {
        MapSnippet(point, if (compact) 100 else 130) { openMaps(context, label, point, directions = false) }
        Row(Modifier.padding(horizontal = 14.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.MyLocation, null, Modifier.size(18.dp), tint = t.colors.brand)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text("You are here", fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink)
                Text(address.ifBlank { label }, fontSize = 12.sp, color = t.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun ContactsCard(card: ToolCard.Contacts) {
    val t = LocalBeep.current
    val context = LocalContext.current
    var all by remember(card) { mutableStateOf(false) }
    val shown = if (all) card.people else card.people.take(3)
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        shown.forEachIndexed { i, person ->
            if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(t.colors.softLine))
            Row(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(36.dp).background(t.colors.tints[i % t.colors.tints.size], CircleShape), contentAlignment = Alignment.Center) {
                    Text(person.name.take(1).uppercase(), fontWeight = FontWeight.Bold, color = t.colors.ink)
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(person.name, fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val detail = person.phone.ifBlank { person.email }
                    if (detail.isNotBlank()) Text(detail, fontSize = 12.sp, color = t.colors.muted, maxLines = 1)
                }
                if (person.phone.isNotBlank()) {
                    RoundAction(Icons.Outlined.Call, "Call ${person.name}") {
                        context.startActivity(Intent(Intent.ACTION_DIAL, Uri.parse("tel:${person.phone}")))
                    }
                    Spacer(Modifier.width(8.dp))
                    RoundAction(Icons.Outlined.Sms, "Message ${person.name}") {
                        context.startActivity(Intent(Intent.ACTION_SENDTO, Uri.parse("smsto:${person.phone}")))
                    }
                } else if (person.email.isNotBlank()) {
                    RoundAction(Icons.Outlined.Email, "Email ${person.name}") {
                        context.startActivity(Intent(Intent.ACTION_SENDTO, Uri.parse("mailto:${person.email}")))
                    }
                }
            }
        }
        if (card.people.size > 3 && !all) {
            Text("Show ${card.people.size - 3} more", fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = t.colors.brand,
                modifier = Modifier.fillMaxWidth().clickable { all = true }.padding(14.dp))
        }
    }
}

@Composable
private fun OpenedAppCard(card: ToolCard.OpenedApp) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val name = card.app.replaceFirstChar { it.uppercase() }
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(34.dp).background(t.colors.tints[1 % t.colors.tints.size], RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
                Icon(toolIconFor(card.app).takeUnless { it == toolIconFor("") } ?: Icons.AutoMirrored.Outlined.Launch, null,
                    Modifier.size(18.dp), tint = t.colors.ink)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Opened $name", fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink)
                if (card.uri.isNotBlank()) Text(card.uri, fontSize = 12.sp, color = t.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            CardButton("Open", Icons.AutoMirrored.Outlined.Launch) { AppLauncher.launch(context, card.app, card.uri, card.fallbackUrl) }
        }
    }
}

@Composable
private fun AlarmCard(alarm: AlarmRequest) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val days = alarm.days.joinToString(", ") { java.text.DateFormatSymbols().shortWeekdays[it] }
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(34.dp).background(t.colors.tints[1 % t.colors.tints.size], RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Alarm, null, Modifier.size(18.dp), tint = t.colors.ink)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Alarm ${alarm.time}", fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink)
                val detail = listOf(alarm.label, days).filter { it.isNotBlank() }.joinToString(" · ")
                if (detail.isNotBlank()) Text(detail, fontSize = 12.sp, color = t.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            CardButton("Alarms", Icons.AutoMirrored.Outlined.Launch) { AppLauncher.launch(context, "clock", "", "") }
        }
    }
}

data class LinkInfo(val title: String, val description: String)

/** Page titles for link previews, read from the first part of each page. */
private object LinkInfos {
    private val found = LruCache<String, LinkInfo>(64)
    private val meta = Regex("""<meta[^>]+(?:property|name)=["'](og:title|og:description|description)["'][^>]*content=["']([^"']*)["']""", RegexOption.IGNORE_CASE)
    private val titleTag = Regex("<title[^>]*>([^<]*)</title>", RegexOption.IGNORE_CASE)

    suspend fun of(url: String): LinkInfo? = found.get(url) ?: withContext(Dispatchers.IO) {
        runCatching {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 6000
            connection.readTimeout = 6000
            connection.instanceFollowRedirects = true
            connection.setRequestProperty("User-Agent", "Mozilla/5.0 (Linux; Android) Beep")
            val head = try {
                if (connection.contentType?.contains("html") != true) return@runCatching null
                connection.inputStream.use { stream -> String(stream.readNBytes(64 * 1024)) }
            } finally { connection.disconnect() }
            val tags = meta.findAll(head).associate { it.groupValues[1].lowercase() to unescape(it.groupValues[2]) }
            val title = tags["og:title"] ?: titleTag.find(head)?.groupValues?.get(1)?.let(::unescape)?.trim().orEmpty()
            LinkInfo(title, tags["og:description"] ?: tags["description"].orEmpty())
        }.getOrNull()?.also { found.put(url, it) }
    }

    private fun unescape(s: String) = s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'")
        .replace("&lt;", "<").replace("&gt;", ">").replace("&#x27;", "'")
}

private val urlPattern = Regex("""https?://[^\s<>()\[\]"'`]+[^\s<>()\[\]"'`.,;:!?]""")

/** Links in a reply worth previewing, at most two. */
fun linksIn(text: String): List<String> = urlPattern.findAll(text).map { it.value }.distinct().take(2).toList()

/** A small preview of a web page: title, a line of description and the site. Tapping opens it in the app. */
@Composable
fun LinkPreview(url: String, onOpen: (String) -> Unit) {
    val t = LocalBeep.current
    val info by produceState<LinkInfo?>(null, url) { value = LinkInfos.of(url) }
    val host = remember(url) { Uri.parse(url).host?.removePrefix("www.").orEmpty() }
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(t.shapes.smallRadius))
        .border(1.dp, t.colors.softLine, RoundedCornerShape(t.shapes.smallRadius))
        .background(t.colors.surface).clickable { onOpen(url) }.padding(10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(36.dp).background(t.colors.raised, RoundedCornerShape(9.dp)), contentAlignment = Alignment.Center) {
            Text(host.take(1).uppercase(), fontWeight = FontWeight.Bold, color = t.colors.ink)
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(info?.title?.ifBlank { null } ?: host, fontWeight = FontWeight.SemiBold, fontSize = 13.sp, color = t.colors.ink,
                maxLines = 1, overflow = TextOverflow.Ellipsis)
            val description = info?.description.orEmpty()
            if (description.isNotBlank()) Text(description, fontSize = 12.sp, color = t.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(host, fontSize = 11.sp, color = t.colors.brand, maxLines = 1)
        }
    }
}

/** Replies the model offered, as chips. A choice gets full-width buttons so it reads as a question to answer. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SuggestedReplies(card: ToolCard.Replies, onPick: (String) -> Unit) {
    val t = LocalBeep.current
    if (card.choice) {
        Column(Modifier.fillMaxWidth().padding(end = 6.dp, bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            card.replies.forEachIndexed { i, reply ->
                ChunkyButton(onClick = { onPick(reply) }, modifier = Modifier.fillMaxWidth().popIn("choice-$reply"),
                    color = t.colors.tints[i % t.colors.tints.size], shape = RoundedCornerShape(t.shapes.smallRadius),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp)) {
                    Text(reply, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                }
            }
        }
    } else {
        FlowRow(Modifier.fillMaxWidth().padding(end = 6.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            card.replies.forEach { reply ->
                ChunkyButton(onClick = { onPick(reply) }, modifier = Modifier.popIn("reply-$reply"),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
                    Text(reply, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

/** Cards for one message's tool calls. Suggested replies are drawn separately, under the newest reply only. */
@Composable
fun ToolCards(cards: List<ToolCard>, compact: Boolean, onBrowse: (String) -> Unit) {
    val shown = cards.filter { it !is ToolCard.Replies }
    if (shown.isEmpty()) return
    Column(Modifier.fillMaxWidth().padding(bottom = 8.dp, end = 4.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        shown.forEach { card ->
            when (card) {
                is ToolCard.Place -> PlaceMapCard(card, compact)
                is ToolCard.Here -> HereCard(card, compact)
                is ToolCard.Contacts -> ContactsCard(card)
                is ToolCard.OpenedApp -> OpenedAppCard(card)
                is ToolCard.Link -> LinkPreview(card.url, onBrowse)
                is ToolCard.Alarm -> AlarmCard(card.alarm)
                is ToolCard.Replies -> Unit
            }
        }
    }
}

@Composable
private fun CardButton(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, primary: Boolean = false, onClick: () -> Unit) {
    val t = LocalBeep.current
    ChunkyButton(onClick = onClick, color = if (primary) t.colors.accent else t.colors.surface,
        contentColor = if (primary) t.colors.onAccent else t.colors.ink, shape = RoundedCornerShape(t.shapes.smallRadius),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp)) {
        Icon(icon, null, Modifier.size(16.dp))
        Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun RoundAction(icon: androidx.compose.ui.graphics.vector.ImageVector, description: String, onClick: () -> Unit) {
    val t = LocalBeep.current
    ChunkyButton(onClick = onClick, modifier = Modifier.size(36.dp), color = t.colors.raised, contentPadding = PaddingValues(0.dp)) {
        Icon(icon, description, Modifier.size(17.dp))
    }
}
