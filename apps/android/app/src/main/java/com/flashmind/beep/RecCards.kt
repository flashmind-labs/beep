package com.flashmind.beep

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.CalendarContract
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.snapping.rememberSnapFlingBehavior
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject
import java.text.NumberFormat
import java.time.OffsetDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/**
 * A recommendation from show_cards: a place, a film or show, an event, or any other kind.
 * Every card has the same base; [place], [media] and [event] add the part for their kind.
 */
data class RecCard(
    val kind: String,
    val title: String,
    val url: String,
    val subtitle: String?,
    val image: String?,
    val facts: List<String>,
    val rating: Double?,
    val reviews: Long?,
    val note: String?,
    val place: RecPlace?,
    val media: RecMedia?,
    val event: RecEvent?,
) {
    /** "4.6 ★ (2,310) · Closest, takes bookings", or whichever parts there are. */
    val details: String? get() {
        val stars = rating?.let { r ->
            val count = reviews?.let { " (${NumberFormat.getIntegerInstance().format(it)})" } ?: ""
            "${"%.1f".format(Locale.ROOT, r)} ★$count"
        }
        return listOfNotNull(stars, note).joinToString(" · ").ifEmpty { null }
    }
}

data class RecPlace(
    val address: String?,
    val latitude: Double?,
    val longitude: Double?,
    val distanceMeters: Int?,
    val hours: String?,
    val openNow: Boolean?,
    val bookingUrl: String?,
)

data class RecMedia(val year: Int?, val creator: String?, val watch: String?, val watchUrl: String?)

data class RecEvent(val start: OffsetDateTime, val end: OffsetDateTime?, val venue: String?, val ticketsUrl: String?)

private fun JSONObject.text(key: String) = optString(key).trim().takeIf { has(key) && !isNull(key) && it.isNotEmpty() }
private fun JSONObject.link(key: String) = text(key)?.takeIf { it.startsWith("https://") || it.startsWith("http://") }
private fun JSONObject.number(key: String) = optDouble(key).takeIf { has(key) && !it.isNaN() }

/** The cards a show_cards result lists. The server has checked them; one that still does not read is skipped. */
internal fun shownRecCards(json: JSONObject): List<RecCard> {
    val list = json.optJSONArray("cards") ?: return emptyList()
    return (0 until list.length()).mapNotNull { i ->
        val o = list.optJSONObject(i) ?: return@mapNotNull null
        val title = o.text("title") ?: return@mapNotNull null
        val url = o.link("url") ?: return@mapNotNull null
        val facts = o.optJSONArray("facts")?.let { a -> (0 until a.length()).mapNotNull { a.optString(it).trim().takeIf(String::isNotEmpty) } }.orEmpty()
        RecCard(
            kind = o.text("kind") ?: "other",
            title = title,
            url = url,
            subtitle = o.text("subtitle"),
            image = o.text("image")?.takeIf { it.startsWith("https://") },
            facts = facts,
            rating = o.number("rating"),
            reviews = o.optLong("reviews", -1).takeIf { it >= 0 },
            note = o.text("note"),
            place = o.optJSONObject("place")?.let { p ->
                RecPlace(p.text("address"), p.number("latitude"), p.number("longitude"),
                    p.optInt("distance_m", -1).takeIf { it >= 0 }, p.text("hours"),
                    if (p.has("open_now") && !p.isNull("open_now")) p.optBoolean("open_now") else null, p.link("booking_url"))
            },
            media = o.optJSONObject("media")?.let { m ->
                RecMedia(m.optInt("year", 0).takeIf { it > 0 }, m.text("creator"), m.text("watch"), m.link("watch_url"))
            },
            event = o.optJSONObject("event")?.let { e ->
                val start = e.text("start")?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() } ?: return@let null
                RecEvent(start, e.text("end")?.let { runCatching { OffsetDateTime.parse(it) }.getOrNull() }, e.text("venue"), e.link("tickets_url"))
            },
        )
    }
}

/** "650 m", "1.2 km" or "13 km". */
internal fun distanceText(meters: Int): String = when {
    meters < 1000 -> "${(meters + 5) / 10 * 10} m"
    meters < 10_000 -> "%.1f km".format(Locale.getDefault(), meters / 1000.0)
    else -> "${(meters + 500) / 1000} km"
}

/** Cards side by side in a row that scrolls and snaps; a lone card takes the full width. */
@Composable
internal fun RecRow(cards: List<RecCard>) {
    if (cards.size == 1) { RecCardView(cards[0], Modifier.fillMaxWidth()); return }
    val state = rememberLazyListState()
    LazyRow(state = state, flingBehavior = rememberSnapFlingBehavior(state), verticalAlignment = Alignment.Top,
        horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
        itemsIndexed(cards, key = { i, c -> "$i ${c.url}" }) { _, card -> RecCardView(card, Modifier.width(216.dp).padding(bottom = 4.dp)) }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun RecCardView(card: RecCard, modifier: Modifier) {
    val context = LocalContext.current
    val open = LocalOpenLink.current
    val openPage = { url: String -> open?.invoke(url) ?: ExternalBrowser.open(context, url) }
    val place = card.place.takeIf { card.kind == "place" }
    val media = card.media.takeIf { card.kind == "media" }
    val event = card.event.takeIf { card.kind == "event" }
    Column(modifier.beepFrame(RoundedCornerShape(16.dp), Beep.colors.surface)) {
        Column(Modifier.fillMaxWidth().clickable(onClickLabel = if (place != null) "Open in Maps" else "Open the page") {
            if (place != null) openPlace(context, card, place) else openPage(card.url)
        }) {
            when {
                media != null -> Poster(card.image, media)
                else -> RecPicture(card.image, event)
            }
            Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 10.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                Text(card.title, color = Beep.colors.ink, fontSize = 14.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                val subtitle = card.subtitle ?: media?.let { listOfNotNull(it.year?.toString(), it.creator).joinToString(" · ").ifEmpty { null } }
                    ?: event?.venue ?: place?.address
                subtitle?.let { Text(it, color = Beep.colors.muted, fontSize = 12.sp, lineHeight = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                val facts = card.facts + listOfNotNull(place?.distanceMeters?.let(::distanceText))
                if (facts.isNotEmpty()) FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    facts.forEach { fact ->
                        Text(fact, fontSize = 11.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink,
                            modifier = Modifier.clip(RoundedCornerShape(50)).background(Beep.colors.tints[1]).padding(horizontal = 7.dp, vertical = 1.dp))
                    }
                }
                place?.let { p ->
                    val status = p.hours ?: when (p.openNow) { true -> "Open now"; false -> "Closed now"; null -> null }
                    status?.let { Text(it, fontSize = 12.sp, fontWeight = FontWeight.Medium, color = if (p.openNow == true) Beep.colors.ok else Beep.colors.muted) }
                }
                event?.let { Text(eventTime(it.start), fontSize = 12.sp, fontWeight = FontWeight.Medium, color = Beep.colors.ink) }
                card.details?.let { Text(it, color = Beep.colors.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
        }
        val actions = buildList {
            place?.let { p ->
                add("Directions" to { openDirections(context, card, p) })
                p.bookingUrl?.let { add("Book" to { openPage(it) }) }
            }
            event?.let { e ->
                add("Calendar" to { addToCalendar(context, card, e) })
                e.ticketsUrl?.let { add("Tickets" to { openPage(it) }) }
            }
            media?.let { m -> m.watchUrl?.let { url -> add("Open ${m.watch?.removePrefix("Rent on ") ?: "page"}" to { ExternalBrowser.open(context, url) }) } }
        }
        if (actions.isNotEmpty()) Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            actions.forEach { (label, action) ->
                Text(label, textAlign = TextAlign.Center, fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.ink, maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).clip(RoundedCornerShape(9.dp)).border(1.5.dp, Beep.colors.ledge, RoundedCornerShape(9.dp))
                        .background(Beep.colors.surface).clickable(onClick = action).padding(vertical = 7.dp))
            }
        } else Box(Modifier.height(4.dp))
    }
}

@Composable
private fun rememberBitmap(url: String?): ImageBitmap? {
    val bitmap by produceState<ImageBitmap?>(null, url) { value = url?.let { ReplyImages.load(it) }?.asImageBitmap() }
    return bitmap
}

@Composable
private fun RecPicture(url: String?, event: RecEvent?) {
    val bitmap = rememberBitmap(url)
    Box(Modifier.fillMaxWidth().height(108.dp).background(Beep.colors.code)) {
        bitmap?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
        event?.let { e ->
            Column(Modifier.padding(10.dp).width(42.dp).beepFrame(RoundedCornerShape(10.dp), Beep.colors.surface).padding(vertical = 4.dp),
                horizontalAlignment = Alignment.CenterHorizontally) {
                Text(e.start.format(DateTimeFormatter.ofPattern("EEE", Locale.getDefault())).uppercase(Locale.getDefault()),
                    fontSize = 9.5.sp, lineHeight = 11.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.accent)
                Text(e.start.dayOfMonth.toString(), fontSize = 17.sp, lineHeight = 19.sp, fontWeight = FontWeight.Bold, color = Beep.colors.ink)
            }
        }
    }
}

@Composable
private fun Poster(url: String?, media: RecMedia) {
    val bitmap = rememberBitmap(url)
    Row(Modifier.fillMaxWidth().height(150.dp).background(Beep.colors.tints[1])) {
        Box(Modifier.width(100.dp).fillMaxSize().background(Beep.colors.code)) {
            bitmap?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
        }
        media.watch?.let { watch ->
            Column(Modifier.weight(1f).fillMaxSize().padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.Bottom)) {
                Text("Watch on", fontSize = 10.5.sp, color = Beep.colors.muted)
                Text(watch, fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.ink)
            }
        }
    }
}

/** "Sat 18 Oct, 21:00" in the event's own time, written the phone's way. */
private fun eventTime(start: OffsetDateTime): String {
    val day = start.format(DateTimeFormatter.ofPattern("EEE d MMM", Locale.getDefault()))
    val time = start.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(Locale.getDefault()))
    return "$day, $time"
}

private fun placeQuery(card: RecCard, place: RecPlace) =
    place.address?.let { "${card.title}, $it" } ?: card.title

private fun openPlace(context: Context, card: RecCard, place: RecPlace) {
    val query = placeQuery(card, place)
    val at = if (place.latitude != null && place.longitude != null) "${place.latitude},${place.longitude}" else "0,0"
    AppLauncher.launch(context, "maps", "geo:$at?q=${Uri.encode(query)}",
        "https://www.google.com/maps/search/?api=1&query=${Uri.encode(query)}")
}

private fun openDirections(context: Context, card: RecCard, place: RecPlace) {
    val to = if (place.latitude != null && place.longitude != null) "${place.latitude},${place.longitude}" else placeQuery(card, place)
    AppLauncher.launch(context, "maps", "google.navigation:q=${Uri.encode(to)}",
        "https://www.google.com/maps/dir/?api=1&destination=${Uri.encode(to)}")
}

private fun addToCalendar(context: Context, card: RecCard, event: RecEvent) {
    val start = event.start.toInstant().toEpochMilli()
    val intent = Intent(Intent.ACTION_INSERT).apply {
        data = CalendarContract.Events.CONTENT_URI
        putExtra(CalendarContract.Events.TITLE, card.title)
        putExtra(CalendarContract.EXTRA_EVENT_BEGIN_TIME, start)
        putExtra(CalendarContract.EXTRA_EVENT_END_TIME, event.end?.toInstant()?.toEpochMilli() ?: (start + 2 * 60 * 60 * 1000))
        (event.venue ?: card.subtitle)?.let { putExtra(CalendarContract.Events.EVENT_LOCATION, it) }
        putExtra(CalendarContract.Events.DESCRIPTION, card.url)
    }
    AppLauncher.start(context, intent)
}
