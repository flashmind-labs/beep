package com.flashmind.beep

import android.content.Context
import android.net.Uri
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material.icons.outlined.Directions
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.proto.RouteCard
import com.flashmind.beep.proto.RouteLeg
import com.flashmind.beep.proto.RouteOption
import com.flashmind.beep.proto.RoutePoint
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.max

/** Where a route ends on screen when the server sends no colour for a line. */
private val CORAL = Color(0xFFF07A6A)

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
        Box(Modifier.fillMaxWidth().height(if (compact) 80.dp else 96.dp).background(t.colors.page)
            .padding(horizontal = 8.dp, vertical = 6.dp), contentAlignment = Alignment.Center) {
            when {
                route != null -> RouteSketch(route, t.colors.muted, t.colors.surface, t.colors.ink, t.colors.brand)
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
                route.legs.forEach { leg -> if (leg.mode == "walk") WalkLeg(leg) else RideLeg(leg) }
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

@Composable
private fun RideLeg(leg: RouteLeg) {
    val t = LocalBeep.current
    val color = parse(leg.color) ?: t.colors.brand
    val onColor = parse(leg.textColor) ?: if (color.luminance() > 0.5f) Color(0xFF10201A) else Color.White
    val badge = leg.line.ifBlank { leg.name }.ifBlank { leg.mode.replaceFirstChar(Char::uppercase) }
    val bold = SpanStyle(fontWeight = FontWeight.SemiBold)
    Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.widthIn(min = 34.dp).height(22.dp).background(color, RoundedCornerShape(6.dp))
            .border(1.5.dp, t.colors.ledge, RoundedCornerShape(6.dp)).padding(horizontal = 5.dp),
            contentAlignment = Alignment.Center) {
            Text(badge, fontSize = 11.sp, fontWeight = FontWeight.Bold, color = onColor, maxLines = 1)
        }
        Box(Modifier.width(3.dp).fillMaxHeight().background(color))
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(buildAnnotatedString {
                withStyle(bold) { append(leg.name.ifBlank { badge }) }
                if (leg.headsign.isNotBlank()) withStyle(SpanStyle(color = t.colors.muted)) { append(" toward ${leg.headsign}") }
            }, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
            Text(buildAnnotatedString {
                withStyle(bold) { append(leg.departs) }
                append(" ${leg.from}")
                if (leg.fromTrack.isNotBlank()) append(", platform ${leg.fromTrack}")
            }, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
            if (leg.stops > 0u) {
                Text("${leg.stops} stop${if (leg.stops > 1u) "s" else ""}", fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.muted)
            }
            Text(buildAnnotatedString {
                withStyle(bold) { append(leg.arrives) }
                append(" ${leg.to.ifBlank { "destination" }}")
            }, fontSize = 13.sp, lineHeight = 18.sp, color = t.colors.ink)
        }
    }
}

private fun parse(hex: String): Color? =
    hex.removePrefix("#").takeIf { it.length == 6 }?.toLongOrNull(16)?.let { Color(0xFF000000 or it) }

/**
 * The route's shape fitted to the box: walks dotted, rides in their line's colour with a stop at each end,
 * a dot where it starts and a coral one where it ends. Longitude is squeezed by latitude so shapes keep their angles.
 */
@Composable
private fun RouteSketch(route: RouteOption, walk: Color, page: Color, ink: Color, start: Color) {
    val points = route.legs.flatMap { it.points }
    if (points.isEmpty()) return
    val squeeze = cos(points.map { it.latitude }.average() * PI / 180)
    val xs = points.map { it.longitude * squeeze }
    val ys = points.map { it.latitude }
    val (left, right) = xs.min() to xs.max()
    val (bottom, top) = ys.min() to ys.max()
    Canvas(Modifier.fillMaxSize()) {
        val pad = 8.dp.toPx()
        val span = max(right - left, top - bottom).takeIf { it > 0 } ?: 1.0
        val scale = minOf((size.width - 2 * pad) / max(right - left, span * 1e-3), (size.height - 2 * pad) / max(top - bottom, span * 1e-3))
        val offsetX = (size.width - (right - left) * scale) / 2
        val offsetY = (size.height - (top - bottom) * scale) / 2
        fun at(p: RoutePoint) = Offset(
            (offsetX + (p.longitude * squeeze - left) * scale).toFloat(),
            (offsetY + (top - p.latitude) * scale).toFloat(),
        )
        route.legs.forEach { leg ->
            if (leg.points.isEmpty()) return@forEach
            val path = Path().apply {
                leg.points.forEachIndexed { i, p -> at(p).let { if (i == 0) moveTo(it.x, it.y) else lineTo(it.x, it.y) } }
            }
            if (leg.mode == "walk") {
                drawPath(path, walk, style = Stroke(2.5.dp.toPx(), cap = StrokeCap.Round,
                    pathEffect = PathEffect.dashPathEffect(floatArrayOf(0.1f, 6.dp.toPx()))))
            } else {
                drawPath(path, parse(leg.color) ?: start, style = Stroke(5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
        route.legs.filter { it.mode != "walk" && it.points.isNotEmpty() }.forEach { leg ->
            listOf(leg.points.first(), leg.points.last()).forEach { p ->
                drawCircle(page, 4.dp.toPx(), at(p))
                drawCircle(ink, 4.dp.toPx(), at(p), style = Stroke(1.8.dp.toPx()))
            }
        }
        drawCircle(start, 5.dp.toPx(), at(points.first()))
        drawCircle(page, 7.dp.toPx(), at(points.last()))
        drawCircle(CORAL, 6.dp.toPx(), at(points.last()))
    }
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
