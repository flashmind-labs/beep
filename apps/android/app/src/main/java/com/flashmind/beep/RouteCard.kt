package com.flashmind.beep

import android.content.Context
import android.net.Uri
import androidx.compose.animation.animateContentSize
import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
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
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.DirectionsWalk
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Directions
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.RouteCard
import com.flashmind.beep.proto.RouteLeg
import com.flashmind.beep.proto.RouteOption
import com.flashmind.beep.proto.RoutePoint
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.ceil
import kotlin.math.log2
import kotlin.math.max
import kotlin.math.pow
import kotlin.math.roundToInt

/** Where a route ends on screen when the server sends no colour for a line. */
private val CORAL = Color(0xFFF07A6A)

/** Text on a station code's white badge, in either theme. */
private val CODE_INK = Color(0xFF10201A)

/** A get_route call's routes: a sketch of the chosen one, its legs, and Maps. Fetched by call id. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun RouteCardView(card: ToolCard.Route, compact: Boolean, repository: ChatRepository) {
    val t = LocalBeep.current
    val context = LocalContext.current
    val loaded by produceState<RouteCard?>(null, card.callId) {
        value = runCatching { withContext(Dispatchers.IO) { repository.route(card.callId) } }.getOrNull()
    }
    var pick by rememberSaveable(card.callId) { mutableIntStateOf(0) }
    val routes = loaded?.routes.orEmpty()
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        val route = routes.getOrNull(pick) ?: routes.firstOrNull()
        Box(Modifier.fillMaxWidth().height(if (compact) 120.dp else 140.dp).background(t.colors.page),
            contentAlignment = Alignment.Center) {
            when {
                route != null -> RouteSketch(route, t.colors.muted, t.colors.surface, t.colors.ink, t.colors.brand, t.colors.ledge,
                    t.fonts.body)
                loaded == null -> CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp, color = t.colors.muted)
                else -> Text("Route no longer available", fontSize = 13.sp, color = t.colors.muted)
            }
        }
        if (route == null) return@BeepCard
        Column(Modifier.padding(start = 14.dp, end = 14.dp, top = 12.dp, bottom = 14.dp)) {
            Text(head(route), style = displayStyle(if (compact) 15.sp else 17.sp), color = t.colors.ink)
            if (routes.size > 1) {
                FlowRow(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    routes.forEachIndexed { i, option ->
                        FilterChip(selected = i == pick, onClick = { pick = i },
                            label = { Text(chip(option), fontSize = 12.sp) })
                    }
                }
            }
            Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                route.legs.forEachIndexed { i, leg ->
                    if (leg.mode == "walk") WalkLeg(leg) else RideLeg(leg, "${card.callId}:$pick:$i")
                }
            }
            Text(if (route.live) "Live times" else "Timetable times, delays not included", fontSize = 11.sp,
                color = t.colors.muted, modifier = Modifier.padding(top = 10.dp))
            Spacer(Modifier.height(10.dp))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                CardButton("Directions", Icons.Outlined.Directions, primary = true) { openTransit(context, route) }
                CardButton("Open in Maps", Icons.Outlined.Map) { openEnd(context, route) }
            }
        }
    }
}

private fun head(route: RouteOption): String {
    val changes = route.transfers.toInt()
    val tail = if (changes > 0) " · $changes change${if (changes > 1) "s" else ""}" else ""
    return "${route.departs} to ${route.arrives} · ${route.minutes} min$tail"
}

private fun chip(route: RouteOption): String {
    val changes = route.transfers.toInt()
    return "${route.minutes} min" + if (changes > 0) ", $changes change${if (changes > 1) "s" else ""}" else ""
}

@Composable
private fun WalkLeg(leg: RouteLeg) {
    val t = LocalBeep.current
    val km = if (leg.meters >= 1000u) ", ${"%.1f".format(leg.meters.toDouble() / 1000)} km" else ""
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.width(34.dp), contentAlignment = Alignment.Center) {
            Icon(Icons.AutoMirrored.Outlined.DirectionsWalk, null, Modifier.size(16.dp), tint = t.colors.muted)
        }
        Text("Walk ${leg.minutes} min$km to ${leg.to.ifBlank { "destination" }}", fontSize = 13.sp, color = t.colors.muted)
    }
}

/** A station code as Tokyo's signs draw it: a white badge ringed in the line's colour. */
@Composable
private fun StationCode(code: String, color: Color) {
    if (code.isBlank()) return
    Box(Modifier.padding(end = 6.dp).widthIn(min = 28.dp).height(17.dp).background(Color.White, RoundedCornerShape(4.dp))
        .border(2.dp, color, RoundedCornerShape(4.dp)).padding(horizontal = 3.dp), contentAlignment = Alignment.Center) {
        Text(code, fontSize = 10.sp, lineHeight = 10.sp, fontWeight = FontWeight.Bold, color = CODE_INK, maxLines = 1)
    }
}

/** A clock time, a station code and the rest of a stop's line. */
@Composable
private fun StopRow(time: String, code: String, color: Color, bold: Boolean, text: @Composable () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(time, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = if (bold) FontWeight.SemiBold else null,
            color = if (bold) LocalBeep.current.colors.ink else LocalBeep.current.colors.muted, modifier = Modifier.widthIn(min = 40.dp))
        StationCode(code, color)
        text()
    }
}

/** A ride; [key] remembers whether its stops are open. */
@Composable
private fun RideLeg(leg: RouteLeg, key: String) {
    val t = LocalBeep.current
    val color = parse(leg.color) ?: t.colors.brand
    val onColor = parse(leg.textColor) ?: if (color.luminance() > 0.5f) Color(0xFF10201A) else Color.White
    val badge = leg.line.ifBlank { leg.name }.ifBlank { leg.mode.replaceFirstChar(Char::uppercase) }
    val bold = SpanStyle(fontWeight = FontWeight.SemiBold)
    var open by rememberSaveable(key) { mutableStateOf(false) }
    Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.widthIn(min = 34.dp).height(22.dp).background(color, RoundedCornerShape(Beep.shapes.smallRadius))
            .border(1.5.dp, t.colors.ledge, RoundedCornerShape(Beep.shapes.smallRadius)).padding(horizontal = 5.dp),
            contentAlignment = Alignment.Center) {
            Text(badge, fontSize = 11.sp, fontWeight = FontWeight.Bold, color = onColor, maxLines = 1)
        }
        Box(Modifier.width(3.dp).fillMaxHeight().background(color))
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(buildAnnotatedString {
                withStyle(bold) { append(leg.name.ifBlank { badge }) }
                if (leg.headsign.isNotBlank()) withStyle(SpanStyle(color = t.colors.muted)) { append(" toward ${leg.headsign}") }
            }, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
            StopRow(leg.departs, leg.fromCode, color, bold = true) {
                Text(leg.from + if (leg.fromTrack.isNotBlank()) ", platform ${leg.fromTrack}" else "",
                    fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
            }
            if (leg.stops > 0u) {
                val stops = "${leg.stops} stop${if (leg.stops > 1u) "s" else ""}"
                if (leg.between.isEmpty()) {
                    Text(stops, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.muted)
                } else {
                    Row(Modifier.clickable { open = !open }.padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, if (open) "Hide stops" else "Show stops",
                            Modifier.size(16.dp).rotate(if (open) 90f else 0f), tint = t.colors.muted)
                        Text(stops, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.muted)
                    }
                }
                if (open) {
                    leg.between.forEach { stop ->
                        StopRow(stop.time, stop.code, color, bold = false) {
                            Text(stop.name, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.muted)
                        }
                    }
                }
            }
            StopRow(leg.arrives, leg.toCode, color, bold = true) {
                Text(leg.to.ifBlank { "destination" }, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
            }
        }
    }
}

private fun parse(hex: String): Color? =
    hex.removePrefix("#").takeIf { it.length == 6 }?.toLongOrNull(16)?.let { Color(0xFF000000 or it) }

/**
 * The route over Esri's gray map tiles: walks dotted, rides in their line's colour with a small dot at each stop
 * passed and a ringed one at each end, labelled with the station's name and codes, a dot where it starts and a
 * coral one where it ends. Drawn in Web Mercator, like the tiles.
 */
@Composable
private fun RouteSketch(route: RouteOption, walk: Color, page: Color, ink: Color, start: Color, ledge: Color, font: FontFamily) {
    val measurer = rememberTextMeasurer()
    val points = route.legs.flatMap { it.points }
    if (points.isEmpty()) return
    val dark = page.luminance() < 0.4f
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val density = LocalDensity.current
        val width = with(density) { maxWidth.roundToPx() }.coerceIn(1, 1400)
        val height = with(density) { maxHeight.roundToPx() }.coerceIn(1, 900)
        val fit = remember(route, width, height) { RouteFit(points, width, height, with(density) { 14.dp.toPx() }) }
        val map by produceState<Bitmap?>(null, fit, dark) {
            value = MapTiles.area(fit.x, fit.y, fit.zoom, (width / fit.shrink).roundToInt(), (height / fit.shrink).roundToInt(),
                dark, TileStyle.Quiet)
        }
        map?.let { Image(it.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = ContentScale.FillBounds) }
        Canvas(Modifier.fillMaxSize()) {
            route.legs.forEach { leg ->
                if (leg.points.isEmpty()) return@forEach
                val path = Path().apply {
                    leg.points.forEachIndexed { i, p -> fit.at(p).let { if (i == 0) moveTo(it.x, it.y) else lineTo(it.x, it.y) } }
                }
                if (leg.mode == "walk") {
                    drawPath(path, walk, style = Stroke(2.5.dp.toPx(), cap = StrokeCap.Round,
                        pathEffect = PathEffect.dashPathEffect(floatArrayOf(0.1f, 6.dp.toPx()))))
                } else {
                    drawPath(path, parse(leg.color) ?: start, style = Stroke(5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                }
            }
            val rides = route.legs.filter { it.mode != "walk" && it.points.isNotEmpty() }
            rides.forEach { leg ->
                val color = parse(leg.color) ?: start
                leg.between.forEach { stop ->
                    drawCircle(page, 2.2.dp.toPx(), fit.at(stop.at))
                    drawCircle(color, 2.2.dp.toPx(), fit.at(stop.at), style = Stroke(1.4.dp.toPx()))
                }
            }
            rides.forEach { leg ->
                listOf(leg.points.first(), leg.points.last()).forEach { p ->
                    drawCircle(page, 4.dp.toPx(), fit.at(p))
                    drawCircle(ink, 4.dp.toPx(), fit.at(p), style = Stroke(1.8.dp.toPx()))
                }
            }
            drawCircle(start, 5.dp.toPx(), fit.at(points.first()))
            drawCircle(page, 7.dp.toPx(), fit.at(points.last()))
            drawCircle(CORAL, 6.dp.toPx(), fit.at(points.last()))
            drawLabels(stationLabels(rides, fit, 18.dp.toPx(), start), measurer, font, page, ink, ledge)
        }
        if (map != null) {
            Text("Esri, HERE, Garmin, © OpenStreetMap", fontSize = 8.sp, color = walk,
                modifier = Modifier.align(Alignment.BottomEnd).background(page.copy(alpha = 0.7f))
                    .padding(horizontal = 4.dp, vertical = 1.dp))
        }
    }
}

private class StationCodeMark(val code: String, val color: Color)

/** A station to get on or off at, with the codes of the lines that stop there. */
private class StationLabel(val at: Offset, val name: String, val codes: MutableList<StationCodeMark>)

/** One label per station: a change at one station, ends closer than [near] px, shares a label. */
private fun stationLabels(rides: List<RouteLeg>, fit: RouteFit, near: Float, fallback: Color): List<StationLabel> {
    val labels = mutableListOf<StationLabel>()
    rides.forEach { leg ->
        val color = parse(leg.color) ?: fallback
        listOf(Triple(leg.points.first(), leg.from, leg.fromCode), Triple(leg.points.last(), leg.to, leg.toCode)).forEach { (p, name, code) ->
            val at = fit.at(p)
            val same = labels.firstOrNull { (it.at - at).getDistance() < near }
            when {
                same != null -> if (code.isNotBlank() && same.codes.none { it.code == code }) same.codes += StationCodeMark(code, color)
                name.isNotBlank() -> labels += StationLabel(at, name,
                    if (code.isNotBlank()) mutableListOf(StationCodeMark(code, color)) else mutableListOf())
            }
        }
    }
    return labels
}

/** Each label beside its stop, toward the middle, then above or below; left out when nowhere has room. */
private fun DrawScope.drawLabels(labels: List<StationLabel>, measurer: TextMeasurer, font: FontFamily, page: Color, ink: Color,
    ledge: Color) {
    val placed = mutableListOf<Rect>()
    val height = 18.dp.toPx()
    val pad = 4.dp.toPx()
    val codePad = 3.dp.toPx()
    val codeHeight = 13.dp.toPx()
    labels.forEach { label ->
        val name = measurer.measure(label.name, TextStyle(fontFamily = font, fontSize = 10.sp, fontWeight = FontWeight.SemiBold, color = ink))
        val codes = label.codes.map {
            measurer.measure(it.code, TextStyle(fontFamily = font, fontSize = 9.sp, fontWeight = FontWeight.Bold, color = CODE_INK))
        }
        val width = 2 * pad + codes.sumOf { (it.size.width + 2 * codePad + pad).toDouble() }.toFloat() + name.size.width
        val off = 7.dp.toPx()
        val sides = if (label.at.x < size.width / 2) listOf(1, -1) else listOf(-1, 1)
        val box = sides.flatMap { side ->
            listOf(0f, -0.85f, 0.85f).map { shift ->
                val left = if (side > 0) label.at.x + off else label.at.x - off - width
                Rect(Offset(left, label.at.y - height / 2 + shift * height), Size(width, height))
            }
        }.firstOrNull { r ->
            r.left >= 0 && r.right <= size.width && r.top >= 0 && r.bottom <= size.height && placed.none { it.overlaps(r) }
        } ?: return@forEach
        placed += box
        val corner = CornerRadius(5.dp.toPx())
        drawRoundRect(page.copy(alpha = 0.92f), box.topLeft, box.size, corner)
        drawRoundRect(ledge, box.topLeft, box.size, corner, style = Stroke(1.dp.toPx()))
        var x = box.left + pad
        codes.forEachIndexed { i, text ->
            val badge = Rect(Offset(x, box.top + (height - codeHeight) / 2), Size(text.size.width + 2 * codePad, codeHeight))
            drawRoundRect(Color.White, badge.topLeft, badge.size, CornerRadius(3.dp.toPx()))
            drawRoundRect(label.codes[i].color, badge.topLeft, badge.size, CornerRadius(3.dp.toPx()), style = Stroke(1.5.dp.toPx()))
            drawText(text, topLeft = Offset(x + codePad, box.top + (height - text.size.height) / 2))
            x = badge.right + pad
        }
        drawText(name, topLeft = Offset(x, box.top + (height - name.size.height) / 2))
    }
}

/** The route's points fitted to [width] x [height] px with [pad] around them, in Web Mercator. */
private class RouteFit(points: List<RoutePoint>, private val width: Int, private val height: Int, pad: Float) {
    private val xs = points.map { mercatorX(it.longitude) }
    private val ys = points.map { mercatorY(it.latitude) }
    val x = (xs.min() + xs.max()) / 2
    val y = (ys.min() + ys.max()) / 2

    /** Pixels per world width, never closer than street level. */
    private val scale = minOf((width - 2 * pad) / max(xs.max() - xs.min(), 1e-12),
        (height - 2 * pad) / max(ys.max() - ys.min(), 1e-12), 256 * 2.0.pow(17))

    /** The tiles' zoom: the one just past the scale, so they are drawn shrunk and sharp. Esri's stop at 16. */
    val zoom = ceil(log2(scale / 256)).toInt().coerceIn(0, 16)

    /** How the map is drawn against the tiles' own size, between a half and one unless the zoom ran out. */
    val shrink = scale / (256 * 2.0.pow(zoom))

    fun at(p: RoutePoint) = Offset(
        (width / 2.0 + (mercatorX(p.longitude) - x) * scale).toFloat(),
        (height / 2.0 + (mercatorY(p.latitude) - y) * scale).toFloat(),
    )
}

private fun RouteOption.ends(): Pair<RoutePoint, RoutePoint>? {
    val points = legs.flatMap { it.points }
    return if (points.isEmpty()) null else points.first() to points.last()
}

private fun RoutePoint.query() = "$latitude,$longitude"

/** Google Maps transit directions between the route's ends, which Maps plans again with live times. */
private fun openTransit(context: Context, route: RouteOption) {
    val (from, to) = route.ends() ?: return
    val url = "https://www.google.com/maps/dir/?api=1&origin=${from.query()}&destination=${to.query()}&travelmode=transit"
    AppLauncher.launch(context, "maps", url, url)
}

private fun openEnd(context: Context, route: RouteOption) {
    val (_, to) = route.ends() ?: return
    val name = route.legs.lastOrNull()?.to?.takeIf(String::isNotBlank)
    val uri = "geo:${to.query()}?q=${to.query()}" + name?.let { "(${Uri.encode(it)})" }.orEmpty()
    AppLauncher.launch(context, "maps", uri, "https://www.google.com/maps/search/?api=1&query=${to.query()}")
}
