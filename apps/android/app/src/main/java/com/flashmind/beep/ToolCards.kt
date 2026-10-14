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
import androidx.compose.material.icons.outlined.ArrowDropDown
import androidx.compose.material.icons.outlined.Call
import androidx.compose.material.icons.outlined.Directions
import androidx.compose.material.icons.outlined.Email
import androidx.compose.material.icons.outlined.LocalTaxi
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.MyLocation
import androidx.compose.material.icons.outlined.Place
import androidx.compose.material.icons.outlined.Sms
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.NotificationsNone
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.drawscope.Stroke
import kotlinx.coroutines.launch
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import java.nio.charset.Charset
import java.net.URLEncoder
import java.io.InputStream
import java.util.Locale
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.tan

/** What a finished tool call shows in the chat instead of a bare chip. */
sealed class ToolCard {
    /**
     * [near] centers a search, such as "italian restaurant" around the user. Directions start
     * at [origin] when given, else at the phone.
     */
    data class Place(
        val name: String,
        val query: String,
        val directions: Boolean,
        val near: LatLon? = null,
        val origin: String? = null,
        val travelMode: String? = null,
    ) : ToolCard()
    data class Here(val latitude: Double, val longitude: Double) : ToolCard()
    data class Contacts(val people: List<Contact>) : ToolCard()
    data class OpenedApp(val app: String, val uri: String, val fallbackUrl: String) : ToolCard()
    data class Link(val url: String) : ToolCard()
    data class Alarm(val alarm: AlarmRequest) : ToolCard()
    data class Replies(val replies: List<String>, val choice: Boolean) : ToolCard()
    data class Images(val images: List<SentImage>) : ToolCard()
    data class Products(val products: List<Product>) : ToolCard()
    /** Recommendations from show_cards: places, films and shows, events or any other kind. */
    data class Recs(val cards: List<RecCard>) : ToolCard()
    /** A file Beep sent, fetched from the server by [id] when first opened. */
    data class File(val id: String, val name: String, val mime: String, val size: Long) : ToolCard()
    /** A place Beep saved by name, which routines can start on arriving at or leaving. */
    data class SavedPlace(val id: String, val name: String, val address: String, val radiusMeters: Int,
        val latitude: Double, val longitude: Double) : ToolCard()
    /** Transit routes a get_route call planned, fetched from the server by the call's id. */
    data class Route(val callId: String) : ToolCard()
}

data class SentImage(val url: String, val caption: String)

data class Contact(val name: String, val phone: String, val email: String)

private fun JSONObject.text(key: String) = optString(key).takeIf { it.isNotBlank() && it != "null" }

/** Tools that get no chip: what they do is already on screen, or is bookkeeping. */
val QUIET_TOOLS = setOf("suggest_replies", "show_products", "show_cards", "notify_user", "task_progress")

/** Reads the card a tool's output describes, or null when it is just text for the model. */
fun cardFor(call: ToolCall): ToolCard? {
    if (!call.isDone || !call.success) return null
    // Its output is text for the model; the card itself waits on the server.
    if (call.name == "get_route") return call.callId.takeIf(String::isNotEmpty)?.let(ToolCard::Route)
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
        "send_image" -> {
            val list = json.optJSONArray("images") ?: return null
            val images = (0 until list.length()).mapNotNull { i ->
                val image = list.optJSONObject(i) ?: return@mapNotNull null
                val url = image.text("url") ?: image.optJSONObject("file")?.let { f ->
                    f.text("id")?.let { SentFiles.imageUrl(it, f.text("name") ?: "image") }
                }
                url?.let { SentImage(it, image.text("caption").orEmpty()) }
            }
            if (images.isEmpty()) null else ToolCard.Images(images)
        }
        "show_products" -> shownProducts(json).takeIf { it.isNotEmpty() }?.let { ToolCard.Products(it) }
        "show_cards" -> shownRecCards(json).takeIf { it.isNotEmpty() }?.let { ToolCard.Recs(it) }
        "place_save" -> savedPlaceOf(json)
        "send_file" -> json.optJSONObject("file")?.let { f ->
            val id = f.text("id") ?: return null
            val name = f.text("name") ?: "file"
            val mime = f.text("mime") ?: "application/octet-stream"
            // An image shows as a picture, as one from send_image does.
            if (mime.startsWith("image/")) ToolCard.Images(listOf(SentImage(SentFiles.imageUrl(id, name), "")))
            else ToolCard.File(id, name, mime, f.optLong("size"))
        }
        else -> null
    }
}

/** Only a place that was saved; one held back for being near another shows nothing. */
private fun savedPlaceOf(json: JSONObject): ToolCard.SavedPlace? {
    if (!json.optBoolean("saved")) return null
    val p = json.optJSONObject("place") ?: return null
    val lat = p.optDouble("latitude", Double.NaN)
    val lon = p.optDouble("longitude", Double.NaN)
    if (lat.isNaN() || lon.isNaN()) return null
    return ToolCard.SavedPlace(p.text("id") ?: return null, p.text("name") ?: return null, p.text("address").orEmpty(),
        p.optInt("radius_m", 100), lat, lon)
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
        return ToolCard.Place(p.text("name") ?: query, query, directions, near, p.text("origin"), p.text("travel_mode"))
    }
    val uri = json.optString("uri")
    val directions = uri.startsWith("google.navigation:")
    val query = Uri.decode(Regex("q=([^&]+)").find(uri)?.groupValues?.get(1)?.replace('+', ' ') ?: return null)
    val name = json.optJSONObject("card")?.optJSONObject("card_data")?.text("name") ?: query
    return ToolCard.Place(name, query, directions)
}

/** [country] is the ISO 3166 code, upper case, or empty when the geocoder gave none. */
data class LatLon(val latitude: Double, val longitude: Double, val address: String, val country: String = "")

/** Finds places with the phone's own geocoder, so no map key is needed. */
private object Places {
    private val found = LruCache<String, LatLon>(64)

    @Suppress("DEPRECATION")
    /** Within about 20 km of [near] when given, so a search finds places close by. */
    suspend fun lookup(context: Context, query: String, near: LatLon? = null): LatLon? {
        val key = near?.let { "$query@%.2f,%.2f".format(Locale.ROOT, it.latitude, it.longitude) } ?: query
        return found.get(key) ?: withContext(Dispatchers.IO) {
            val system = if (Geocoder.isPresent()) {
                runCatching {
                    val geocoder = Geocoder(context)
                    if (near == null) geocoder.getFromLocationName(query, 1)?.firstOrNull()
                    else geocoder.getFromLocationName(query, 1, near.latitude - SPAN, near.longitude - SPAN,
                        near.latitude + SPAN, near.longitude + SPAN)?.firstOrNull()
                }.getOrNull()?.let { LatLon(it.latitude, it.longitude, it.getAddressLine(0).orEmpty(), it.countryCode.orEmpty().uppercase()) }
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
        val url = URL("https://nominatim.openstreetmap.org/search?format=json&addressdetails=1&limit=1$box&q=" + URLEncoder.encode(query, "UTF-8"))
        val connection = url.openConnection() as HttpURLConnection
        connection.connectTimeout = 8000
        connection.readTimeout = 8000
        connection.setRequestProperty("User-Agent", "Beep for Android")
        val body = try { connection.inputStream.bufferedReader().use { it.readText() } } finally { connection.disconnect() }
        JSONArray(body).optJSONObject(0)?.let {
            LatLon(it.getString("lat").toDouble(), it.getString("lon").toDouble(), it.optString("display_name"),
                it.optJSONObject("address")?.optString("country_code").orEmpty().uppercase())
        }
    }.getOrNull()

    @Suppress("DEPRECATION")
    suspend fun addressAt(context: Context, latitude: Double, longitude: Double): String = withContext(Dispatchers.IO) {
        if (!Geocoder.isPresent()) return@withContext ""
        runCatching { Geocoder(context).getFromLocation(latitude, longitude, 1)?.firstOrNull()?.getAddressLine(0) }
            .getOrNull().orEmpty()
    }
}

/** Longitude as a Web Mercator x, 0 to 1 from the antimeridian eastwards, as map tiles are laid out. */
internal fun mercatorX(longitude: Double) = (longitude + 180) / 360

/** Latitude as a Web Mercator y, 0 at the top of the map to 1 at the bottom. */
internal fun mercatorY(latitude: Double): Double {
    val lat = Math.toRadians(latitude.coerceIn(-85.0, 85.0))
    return (1 - ln(tan(lat) + 1 / cos(lat)) / PI) / 2
}

/** Which tiles a map is drawn from: OpenStreetMap's own, or Esri's quiet gray canvas to draw a route over. */
internal enum class TileStyle { Streets, Quiet }

/**
 * A still map stitched from 256px raster tiles, kept in memory for the session. OpenStreetMap
 * tiles are inverted and dimmed for dark themes; Esri's canvas has a dark version of its own.
 */
internal object MapTiles {
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

    /** Around one point, with tiles one zoom level deeper than [zoom] so they read sharp on dense screens. */
    suspend fun snapshot(latitude: Double, longitude: Double, zoom: Int, width: Int, height: Int, dark: Boolean): Bitmap? =
        area(mercatorX(longitude), mercatorY(latitude), zoom + 1, width, height, dark, TileStyle.Streets)

    /** [width] x [height] tile pixels at zoom [z], centred on Web Mercator ([x], [y]). */
    suspend fun area(x: Double, y: Double, z: Int, width: Int, height: Int, dark: Boolean, style: TileStyle): Bitmap? =
        withContext(Dispatchers.IO) {
            val world = TILE * 2.0.pow(z)
            val left = x * world - width / 2.0
            val top = y * world - height / 2.0
            val count = 2.0.pow(z).toInt()
            val out = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(out)
            val paint = if (dark && style == TileStyle.Streets) night else null
            var drawn = 0
            for (tx in floor(left / TILE).toInt()..floor((left + width) / TILE).toInt()) {
                for (ty in floor(top / TILE).toInt()..floor((top + height) / TILE).toInt()) {
                    if (ty !in 0 until count) continue
                    val tile = tile(url(style, dark, z, Math.floorMod(tx, count), ty)) ?: continue
                    canvas.drawBitmap(tile, (tx * TILE - left).toFloat(), (ty * TILE - top).toFloat(), paint)
                    drawn++
                }
            }
            if (drawn > 0) out else null
        }

    private fun url(style: TileStyle, dark: Boolean, z: Int, x: Int, y: Int) = when (style) {
        TileStyle.Streets -> "https://tile.openstreetmap.org/$z/$x/$y.png"
        TileStyle.Quiet -> "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/" +
            "World_${if (dark) "Dark" else "Light"}_Gray_Base/MapServer/tile/$z/$y/$x"
    }

    private fun tile(url: String): Bitmap? {
        tiles.get(url)?.let { return it }
        return runCatching {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 8000
            connection.readTimeout = 8000
            connection.setRequestProperty("User-Agent", "Beep for Android")
            try { connection.inputStream.use(BitmapFactory::decodeStream) } finally { connection.disconnect() }
        }.getOrNull()?.also { tiles.put(url, it) }
    }
}

/** The map part of a place or location card, with a pin on the spot and, given [ringMeters], a ring that far around it. */
@Composable
private fun MapSnippet(point: LatLon?, height: Int, ringMeters: Int? = null, onOpen: () -> Unit) {
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
        if (map != null && point != null && ringMeters != null) {
            val ring = t.colors.accent
            Box(Modifier.fillMaxSize().drawBehind {
                // The snapshot is drawn one zoom level past 15, a pixel per bitmap pixel.
                val metersPerPixel = 156_543.034 * cos(Math.toRadians(point.latitude)) / 2.0.pow(16)
                val radius = (ringMeters / metersPerPixel).toFloat()
                drawCircle(ring.copy(alpha = 0.16f), radius)
                drawCircle(ring, radius, style = Stroke(2.dp.toPx()))
            })
        }
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

/** google.navigation always starts at the phone, so a route from elsewhere uses the Maps URL. */
private fun openDirections(context: Context, card: ToolCard.Place) {
    val mode = card.travelMode ?: "driving"
    val url = "https://www.google.com/maps/dir/?api=1&destination=${Uri.encode(card.query)}&travelmode=$mode" +
        card.origin?.let { "&origin=${Uri.encode(it)}" }.orEmpty()
    val flag = when (mode) { "walking" -> "w"; "transit" -> "r"; "bicycling" -> "b"; else -> "d" }
    val uri = if (card.origin != null) url else "google.navigation:q=${Uri.encode(card.query)}&mode=$flag"
    AppLauncher.launch(context, "maps", uri, url)
}

private fun openMaps(context: Context, query: String, point: LatLon?, near: LatLon? = null) {
    val uri = when {
        // Maps runs the search itself around the user rather than jumping to one geocoded hit.
        near != null -> "geo:${near.latitude},${near.longitude}?q=${Uri.encode(query)}"
        point != null -> "geo:${point.latitude},${point.longitude}?q=${Uri.encode(query)}"
        else -> "geo:0,0?q=${Uri.encode(query)}"
    }
    AppLauncher.launch(context, "maps", uri, "https://www.google.com/maps/search/?api=1&query=${Uri.encode(query)}")
}

/** Opens Uber with pickup at the phone and the drop-off at [point]. */
private fun openUber(context: Context, name: String, point: LatLon) {
    val link = Uri.parse("https://m.uber.com/ul/").buildUpon()
        .appendQueryParameter("action", "setPickup")
        .appendQueryParameter("pickup", "my_location")
        .appendQueryParameter("dropoff[nickname]", name)
        .appendQueryParameter("dropoff[formatted_address]", point.address.ifBlank { name })
        .appendQueryParameter("dropoff[latitude]", point.latitude.toString())
        .appendQueryParameter("dropoff[longitude]", point.longitude.toString())
        .build().toString()
    AppLauncher.launch(context, "uber", link, link)
}

/** A local ride app and the ISO country codes it runs in. The list order is the menu order. */
private class RideApp(val name: String, val pkg: String, val countries: Set<String>)

private val EUROPE_BOLT = setOf("EE", "LV", "LT", "FI", "SE", "NO", "PL", "CZ", "SK", "RO", "BG", "HR", "PT", "ES", "FR",
    "DE", "AT", "NL", "BE", "IE", "GB", "MT", "CY", "GR", "UA", "GE")

private const val GRAB_PACKAGE = "com.grabtaxi.passenger"

private val RIDE_APPS = listOf(
    RideApp("Grab", GRAB_PACKAGE, setOf("SG", "MY", "ID", "TH", "VN", "PH", "KH", "MM")),
    RideApp("Gojek", "com.gojek.app", setOf("ID", "SG")),
    RideApp("Cabify", "com.cabify.rider", setOf("ES", "AR", "CL", "CO", "PE", "UY")),
    RideApp("Lyft", "me.lyft.android", setOf("US", "CA")),
    RideApp("FreeNow", "taxi.android.client", setOf("DE", "AT", "IE", "GB", "ES", "FR", "IT", "PL", "PT", "GR")),
    RideApp("Bolt", "ee.mtakso.client", EUROPE_BOLT + setOf("NG", "KE", "ZA", "GH")),
    RideApp("DiDi", "com.didiglobal.passenger", setOf("MX", "CL", "CO", "PE", "AU", "NZ")),
    RideApp("Careem", "com.careem.acma", setOf("AE", "SA", "JO", "PK")),
    RideApp("Ola", "com.olacabs.customer", setOf("IN")),
    RideApp("Yandex Go", "ru.yandex.taxi", setOf("RU")),
)

private const val LOCAL_RIDES_SHOWN = 3

private fun launchIntent(context: Context, pkg: String) = context.packageManager.getLaunchIntentForPackage(pkg)

/** Lyft's documented deep link, which sets the drop-off. */
private fun lyftLink(point: LatLon) =
    "lyft://ridetype?id=lyft&destination[latitude]=${point.latitude}&destination[longitude]=${point.longitude}"

/** Opens the installed [app] on its home, or on the drop-off for Lyft. */
private fun openRideApp(context: Context, app: RideApp, launch: Intent, point: LatLon) {
    val dropoff = if (app.name == "Lyft") Intent(Intent.ACTION_VIEW, Uri.parse(lyftLink(point))).setPackage(app.pkg) else null
    if (dropoff != null && runCatching { context.startActivity(dropoff.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }.isSuccess) return
    context.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

/** Grab's booking link, which sets the drop-off. Not publicly documented, so [openGrab] falls back to the home. */
private fun grabLink(name: String, point: LatLon) = Uri.parse("grab://open").buildUpon()
    .appendQueryParameter("screenType", "BOOKING")
    .appendQueryParameter("dropOffLatitude", point.latitude.toString())
    .appendQueryParameter("dropOffLongitude", point.longitude.toString())
    .appendQueryParameter("dropOffAddress", point.address.ifBlank { name })
    .appendQueryParameter("dropOffKeywords", name)
    .build()

/**
 * Opens Grab on a booking to [point], or its home if the link fails, or Uber's home without the
 * Grab app, since Uber books Grab rides there.
 */
private fun openGrab(context: Context, launch: Intent, name: String, point: LatLon) {
    val booking = Intent(Intent.ACTION_VIEW, grabLink(name, point)).setPackage(GRAB_PACKAGE)
    if (launch.`package` == GRAB_PACKAGE &&
        runCatching { context.startActivity(booking.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }.isSuccess) return
    context.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

private class RideChoice(val name: String, val note: String, val open: () -> Unit)

/**
 * The ride options for [point]'s country, or empty when no local app there is installed. Apps
 * that are not installed are left out, since there is nothing to open. Where Grab runs,
 * Uber's own rides are gone, so Grab comes first; elsewhere Uber stays first.
 */
private fun rideChoices(context: Context, name: String, point: LatLon): List<RideChoice> {
    val local = RIDE_APPS.filter { point.country in it.countries }
    if (local.isEmpty()) return emptyList()
    val uber = RideChoice("Uber", "Drop-off set to $name") { openUber(context, name, point) }
    val grab = local.firstOrNull { it.name == "Grab" }
    if (grab != null) {
        val grabApp = launchIntent(context, grab.pkg)
        val launch = grabApp ?: launchIntent(context, "com.ubercab") ?: return emptyList()
        return listOf(
            RideChoice("Grab", if (grabApp != null) "In the Grab app" else "Through Uber: tap Grab there") { openGrab(context, launch, name, point) },
            RideChoice("Uber", "Uber's own rides") { openUber(context, name, point) },
        )
    }
    val others = local.mapNotNull { app -> launchIntent(context, app.pkg)?.let { app to it } }
        .take(LOCAL_RIDES_SHOWN)
        .map { (app, launch) ->
            val note = if (app.name == "Lyft") "Drop-off set to $name" else "Set the drop-off there"
            RideChoice(app.name, note) { openRideApp(context, app, launch, point) }
        }
    if (others.isEmpty()) return emptyList()
    return listOf(uber) + others
}

@Composable
private fun RideMenu(choices: List<RideChoice>) {
    val t = LocalBeep.current
    var open by remember { mutableStateOf(false) }
    Box {
        CardButton("Ride", Icons.Outlined.LocalTaxi, trailing = Icons.Outlined.ArrowDropDown) { open = true }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }, containerColor = t.colors.surface,
            shape = RoundedCornerShape(t.shapes.smallRadius)) {
            choices.forEach { choice ->
                DropdownMenuItem(text = {
                    Column {
                        Text(choice.name, fontSize = 14.sp)
                        Text(choice.note, fontSize = 11.sp, color = t.colors.muted)
                    }
                }, onClick = { open = false; choice.open() })
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PlaceMapCard(card: ToolCard.Place, compact: Boolean) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val found by produceState<LatLon?>(null, card.query, card.near) { value = Places.lookup(context, card.query, card.near) }
    val point = found ?: card.near
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        MapSnippet(point, if (compact) 110 else 150) { openMaps(context, card.query, point, card.near) }
        Column(Modifier.padding(14.dp)) {
            Text(card.name, style = displayStyle(if (compact) 15.sp else 17.sp), color = t.colors.ink,
                maxLines = 2, overflow = TextOverflow.Ellipsis)
            val address = found?.address.orEmpty()
            if (address.isNotBlank() && address != card.name) {
                Spacer(Modifier.height(3.dp))
                Text(address, fontSize = 13.sp, color = t.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            card.origin?.let { origin ->
                Spacer(Modifier.height(3.dp))
                Text("From $origin", fontSize = 13.sp, color = t.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(12.dp))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                CardButton("Directions", Icons.Outlined.Directions, primary = true) { openDirections(context, card) }
                // A ride goes to one looked-up place from the phone, so not for searches or routes from elsewhere.
                val ride = found?.takeIf { card.near == null && card.origin == null }
                if (ride != null) {
                    val choices = remember(ride) { rideChoices(context, card.name, ride) }
                    if (choices.isEmpty()) CardButton("Uber", Icons.Outlined.LocalTaxi) { openUber(context, card.name, ride) }
                    else RideMenu(choices)
                }
                CardButton("Open in Maps", Icons.Outlined.Map) { openMaps(context, card.query, point, card.near) }
            }
        }
    }
}

/** How near counts as being at a saved place, as the card offers it. */
private val PLACE_RADII = listOf(50, 100, 250)

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SavedPlaceCard(card: ToolCard.SavedPlace, compact: Boolean, repository: ChatRepository) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var name by rememberSaveable(card.id) { mutableStateOf(card.name) }
    var radius by rememberSaveable(card.id) { mutableIntStateOf(card.radiusMeters) }
    var renaming by remember { mutableStateOf(false) }
    var addingRoutine by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf("") }
    val point = remember(card) { LatLon(card.latitude, card.longitude, card.address) }
    fun setRadius(meters: Int) {
        val before = radius
        radius = meters
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { repository.setPlaceRadius(card.id, meters) } }
                .onSuccess { note = "" }.onFailure { radius = before; note = userMessage(it) }
        }
    }
    if (renaming) {
        var draft by remember { mutableStateOf(name) }
        AlertDialog(onDismissRequest = { renaming = false }, containerColor = t.colors.surface,
            title = { Text("Rename $name") },
            text = { OutlinedTextField(draft, { draft = it }, singleLine = true, label = { Text("Name") }) },
            confirmButton = { TextButton(enabled = draft.isNotBlank(), onClick = {
                val wanted = draft.trim()
                renaming = false
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { repository.renamePlace(card.id, wanted) } }
                        .onSuccess { name = wanted; note = "" }.onFailure { note = userMessage(it) }
                }
            }) { Text("Rename") } },
            dismissButton = { TextButton(onClick = { renaming = false }) { Text("Cancel") } })
    }
    if (addingRoutine) {
        val here = Place(card.id, name, card.address, radius, 0, null, false)
        val places by produceState(listOf(here)) {
            value = runCatching { withContext(Dispatchers.IO) { repository.listPlaces() } }.getOrNull()
                ?.takeIf { list -> list.any { it.id == card.id } } ?: value
        }
        CreateCronDialog(places, onDismiss = { addingRoutine = false }, onAskBeep = null,
            preset = PlaceSchedule(PlaceEvent.Arrive, card.id, PlaceRepeat.Daily)) { label, prompt, schedule ->
            scope.launch {
                runCatching { withContext(Dispatchers.IO) { repository.createCron(label, prompt, schedule) } }
                    .onSuccess { addingRoutine = false; note = "Routine saved" }.onFailure { note = userMessage(it) }
            }
        }
    }
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        MapSnippet(point, if (compact) 110 else 140, ringMeters = radius) { openMaps(context, name, point) }
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(name, Modifier.weight(1f, fill = false), style = displayStyle(if (compact) 15.sp else 17.sp), color = t.colors.ink,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text("Saved", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = t.colors.ink,
                    modifier = Modifier.background(t.colors.tints[1], RoundedCornerShape(50)).padding(horizontal = 8.dp, vertical = 2.dp))
            }
            if (card.address.isNotBlank()) {
                Spacer(Modifier.height(3.dp))
                Text(card.address, fontSize = 13.sp, color = t.colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(10.dp))
            Text("Counts as here within", fontSize = 12.sp, color = t.colors.muted)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PLACE_RADII.forEach { meters ->
                    FilterChip(selected = radius == meters, onClick = { if (radius != meters) setRadius(meters) }, label = { Text("$meters m") })
                }
            }
            if (note.isNotEmpty()) Text(note, fontSize = 12.sp, color = t.colors.muted, modifier = Modifier.padding(top = 4.dp))
            Spacer(Modifier.height(8.dp))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                CardButton("When I get here", Icons.Outlined.NotificationsNone, primary = true) { addingRoutine = true }
                CardButton("Rename", Icons.Outlined.Edit) { renaming = true }
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
        MapSnippet(point, if (compact) 100 else 130) {
            openMaps(context, "%.5f,%.5f".format(Locale.ROOT, card.latitude, card.longitude), point)
        }
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
                    Text(initial(person.name), fontWeight = FontWeight.Bold, color = t.colors.ink)
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(person.name, fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = t.colors.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val detail = person.phone.ifBlank { person.email }
                    if (detail.isNotBlank()) Text(detail, fontSize = 12.sp, color = t.colors.muted, maxLines = 1)
                }
                if (person.phone.isNotBlank()) {
                    RoundAction(Icons.Outlined.Call, "Call ${person.name}") {
                        AppLauncher.start(context, Intent(Intent.ACTION_DIAL, Uri.parse("tel:${person.phone}")))
                    }
                    Spacer(Modifier.width(8.dp))
                    RoundAction(Icons.Outlined.Sms, "Message ${person.name}") {
                        AppLauncher.start(context, Intent(Intent.ACTION_SENDTO, Uri.parse("smsto:${person.phone}")))
                    }
                } else if (person.email.isNotBlank()) {
                    RoundAction(Icons.Outlined.Email, "Email ${person.name}") {
                        AppLauncher.start(context, Intent(Intent.ACTION_SENDTO, Uri.parse("mailto:${person.email}")))
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
            Box(Modifier.size(34.dp).background(t.colors.tints[1], RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
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
            Box(Modifier.size(34.dp).background(t.colors.tints[1], RoundedCornerShape(10.dp)), contentAlignment = Alignment.Center) {
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
    private val charsetParam = Regex("""charset=["']?([\w.:-]+)""", RegexOption.IGNORE_CASE)
    private val numericEntity = Regex("&#(x?)([0-9a-f]+);", RegexOption.IGNORE_CASE)

    suspend fun of(url: String): LinkInfo? = found.get(url) ?: withContext(Dispatchers.IO) {
        runCatching {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 6000
            connection.readTimeout = 6000
            connection.instanceFollowRedirects = true
            connection.setRequestProperty("User-Agent", "Mozilla/5.0 (Linux; Android) Beep")
            val head = try {
                val type = connection.contentType
                if (type?.contains("html") != true) return@runCatching null
                decode(connection.inputStream.use { readUpTo(it, 64 * 1024) }, type)
            } finally { connection.disconnect() }
            val tags = meta.findAll(head).associate { it.groupValues[1].lowercase() to unescape(it.groupValues[2]) }
            val title = tags["og:title"] ?: titleTag.find(head)?.groupValues?.get(1)?.let(::unescape)?.trim().orEmpty()
            LinkInfo(title, tags["og:description"] ?: tags["description"].orEmpty())
        }.getOrNull()?.also { found.put(url, it) }
    }

    /** At most [limit] bytes; readNBytes needs API 33. */
    private fun readUpTo(input: InputStream, limit: Int): ByteArray {
        val buf = ByteArray(limit)
        var size = 0
        while (size < limit) {
            val n = input.read(buf, size, limit - size)
            if (n < 0) break
            size += n
        }
        return buf.copyOf(size)
    }

    /** In the charset the header names, else the one the page declares, else UTF-8. Plenty of news sites still send Latin-1. */
    private fun decode(bytes: ByteArray, contentType: String): String {
        val declared = charsetParam.find(contentType)?.groupValues?.get(1)
            ?: charsetParam.find(String(bytes, 0, minOf(bytes.size, 2048), Charsets.ISO_8859_1))?.groupValues?.get(1)
        val charset = declared?.let { runCatching { Charset.forName(it) }.getOrNull() } ?: Charsets.UTF_8
        return String(bytes, charset)
    }

    private fun unescape(s: String) = numericEntity.replace(s) { m ->
        m.groupValues[2].toIntOrNull(if (m.groupValues[1].isEmpty()) 10 else 16)
            ?.takeIf { Character.isValidCodePoint(it) }?.let { String(Character.toChars(it)) } ?: m.value
    }.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'")
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
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            card.replies.forEach { reply ->
                ChunkyButton(onClick = { onPick(reply) }, modifier = Modifier.popIn("reply-$reply"),
                    contentPadding = PaddingValues(horizontal = 11.dp, vertical = 5.dp)) {
                    Text(reply, fontSize = 12.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

/** Cards for one message's tool calls. Suggested replies are drawn separately, under the newest reply only. */
@Composable
fun ToolCards(cards: List<ToolCard>, compact: Boolean, repository: ChatRepository, onBrowse: (String) -> Unit) {
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
                is ToolCard.Images -> card.images.forEach { image ->
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        ReplyImage(image.url, image.caption, compact)
                        if (image.caption.isNotBlank()) Text(image.caption, fontSize = 13.sp, color = LocalBeep.current.colors.muted)
                    }
                }
                is ToolCard.Products -> ProductRow(card.products, loading = false)
                is ToolCard.Recs -> RecRow(card.cards)
                is ToolCard.File -> FileCard(card)
                is ToolCard.SavedPlace -> SavedPlaceCard(card, compact, repository)
                is ToolCard.Route -> RouteCardView(card, compact, repository)
                is ToolCard.Replies -> Unit
            }
        }
    }
}

@Composable
internal fun CardButton(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, primary: Boolean = false,
    trailing: androidx.compose.ui.graphics.vector.ImageVector? = null, onClick: () -> Unit) {
    val t = LocalBeep.current
    ChunkyButton(onClick = onClick, color = if (primary) t.colors.accent else t.colors.surface,
        contentColor = if (primary) t.colors.onAccent else t.colors.ink, shape = RoundedCornerShape(t.shapes.smallRadius),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp)) {
        Icon(icon, null, Modifier.size(16.dp))
        Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        trailing?.let { Icon(it, null, Modifier.size(16.dp)) }
    }
}

@Composable
private fun RoundAction(icon: androidx.compose.ui.graphics.vector.ImageVector, description: String, onClick: () -> Unit) {
    val t = LocalBeep.current
    ChunkyButton(onClick = onClick, modifier = Modifier.size(36.dp), color = t.colors.raised, contentPadding = PaddingValues(0.dp)) {
        Icon(icon, description, Modifier.size(17.dp))
    }
}
