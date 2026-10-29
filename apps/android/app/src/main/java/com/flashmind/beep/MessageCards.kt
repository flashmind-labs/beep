package com.flashmind.beep

import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.compose.animation.*
import androidx.compose.animation.core.EaseInOutCubic
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.Image
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import kotlinx.coroutines.launch

// Short names for the current theme's colors.
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val TextPrimary: Color @Composable @ReadOnlyComposable get() = Beep.colors.ink
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

@Composable
private fun PlaceCard(data: org.json.JSONObject, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val name = data.optString("name", "Unknown place")
    val rating = data.optDouble("rating", 0.0)
    val address = data.optString("address", "")
    val lat = data.optDouble("latitude", 0.0)
    val lng = data.optDouble("longitude", 0.0)

    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.padding(14.dp)) {
            Text(name, style = displayStyle(if (compact) 15.sp else 17.sp), color = TextPrimary)
            if (rating > 0) {
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val fullStars = rating.toInt()
                    repeat(fullStars) {
                        Icon(Icons.Outlined.Star, null, Modifier.size(14.dp), tint = Beep.colors.warn)
                    }
                    repeat(5 - fullStars) {
                        Icon(Icons.Outlined.StarBorder, null, Modifier.size(14.dp), tint = TextDim)
                    }
                    Spacer(Modifier.width(6.dp))
                    Text("%.1f".format(rating), fontSize = 12.sp, color = TextMuted)
                }
            }
            if (address.isNotBlank()) {
                Spacer(Modifier.height(4.dp))
                Text(address, fontSize = if (compact) 12.sp else 13.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(12.dp))
            ChunkyButton(
                onClick = {
                    val uri = if (lat != 0.0 && lng != 0.0) "geo:$lat,$lng?q=${Uri.encode(name)}"
                    else "geo:0,0?q=${Uri.encode(name)}"
                    AppLauncher.start(context, Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
                },
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
                shape = RoundedCornerShape(Beep.shapes.smallRadius),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
            ) {
                Icon(Icons.Outlined.Map, null, Modifier.size(16.dp))
                Text("Open in Maps", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

@Composable
private fun LinkCard(data: org.json.JSONObject, onBrowse: (String) -> Unit, compact: Boolean) {
    val title = data.optString("title", "")
    val description = data.optString("description", "")
    val url = data.optString("url", "")

    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.fillMaxWidth().clickable(enabled = url.isNotBlank()) { onBrowse(url) }.padding(14.dp)) {
            if (title.isNotBlank()) {
                Text(title, fontWeight = FontWeight.SemiBold, fontSize = if (compact) 14.sp else 15.sp, color = TextPrimary, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (description.isNotBlank()) {
                if (title.isNotBlank()) Spacer(Modifier.height(3.dp))
                Text(description, fontSize = if (compact) 12.sp else 13.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (url.isNotBlank()) {
                Spacer(Modifier.height(6.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(13.dp), tint = Moss)
                    Spacer(Modifier.width(4.dp))
                    Text(url.removePrefix("https://").removePrefix("http://").take(40), fontSize = 12.sp, color = Moss, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun ActionButtonRow(data: org.json.JSONObject, repository: ChatRepository) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val buttonsArr = data.optJSONArray("buttons") ?: return
    val buttons = (0 until buttonsArr.length()).mapNotNull { buttonsArr.optJSONObject(it) }
    val openLink = LocalOpenLink.current ?: repository::openBrowser

    // Room below for the hard shadow, which the scroll container would otherwise clip.
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(bottom = 6.dp, end = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        buttons.forEachIndexed { index, button ->
            val label = button.optString("label", "Action")
            val action = button.optString("action", "")
            val isPrimary = index == 0
            ChunkyButton(
                onClick = {
                    when (action) {
                        "open_browser" -> openLink(button.optString("url", ""))
                        "open_maps" -> {
                            val query = button.optString("query", label)
                            AppLauncher.start(context, Intent(Intent.ACTION_VIEW, Uri.parse("geo:0,0?q=${Uri.encode(query)}")))
                        }
                        "open_app" -> repository.pendingIntent.value = AppIntent(
                            app = button.optString("app", ""),
                            uri = button.optString("uri", ""),
                            fallbackUrl = button.optString("fallback_url", ""))
                    }
                },
                color = if (isPrimary) Beep.colors.accent else Surface,
                contentColor = if (isPrimary) Beep.colors.onAccent else TextPrimary,
                shape = RoundedCornerShape(Beep.shapes.smallRadius),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
            ) {
                Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

@Composable
internal fun TaskProgressCard(steps: List<TaskStep>) {
    val noMotion = reduceMotion()
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            steps.forEach { step ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                        when (step.status) {
                            "pending" -> Box(Modifier.size(14.dp).border(1.5.dp, Border, CircleShape))
                            "in_progress" -> {
                                val pulse = rememberInfiniteTransition(label = "step-pulse")
                                val scale by if (noMotion) remember { mutableStateOf(1f) } else pulse.animateFloat(
                                    initialValue = 0.7f, targetValue = 1f,
                                    animationSpec = infiniteRepeatable(tween(600, easing = EaseInOutCubic), RepeatMode.Reverse),
                                    label = "step-scale")
                                Box(Modifier.size(12.dp).graphicsLayer { scaleX = scale; scaleY = scale }
                                    .background(Beep.colors.accent, CircleShape))
                            }
                            "completed" -> Box(Modifier.size(18.dp).background(Beep.colors.ok, CircleShape), contentAlignment = Alignment.Center) {
                                Icon(Icons.Outlined.Check, "Completed", Modifier.size(12.dp), tint = Color.White)
                            }
                            "failed" -> Box(Modifier.size(18.dp).background(Beep.colors.error, CircleShape), contentAlignment = Alignment.Center) {
                                Icon(Icons.Outlined.Close, "Failed", Modifier.size(12.dp), tint = Color.White)
                            }
                        }
                    }
                    Spacer(Modifier.width(10.dp))
                    Text(
                        step.label,
                        fontSize = 14.sp,
                        fontWeight = if (step.status == "in_progress") FontWeight.Medium else FontWeight.Normal,
                        color = when (step.status) {
                            "completed", "in_progress" -> TextPrimary
                            "failed" -> Beep.colors.error
                            else -> TextMuted
                        }
                    )
                }
            }
        }
    }
}

internal fun toolIconFor(name: String): androidx.compose.ui.graphics.vector.ImageVector {
    val lower = name.lowercase()
    return when {
        "search" in lower || "web" in lower -> Icons.Outlined.Search
        "weather" in lower || "forecast" in lower -> Icons.Outlined.Cloud
        "calendar" in lower || "schedule" in lower || "event" in lower -> Icons.Outlined.CalendarMonth
        "location" in lower || "map" in lower || "place" in lower || "nearby" in lower -> Icons.Outlined.LocationOn
        "email" in lower || "mail" in lower -> Icons.Outlined.Email
        "notification" in lower || "alert" in lower -> Icons.Outlined.Notifications
        "contact" in lower || "person" in lower -> Icons.Outlined.Person
        "memory" in lower || "remember" in lower -> Icons.Outlined.Psychology
        "file" in lower || "document" in lower || "read" in lower -> Icons.Outlined.Description
        "app" in lower || "launch" in lower -> Icons.Outlined.Launch
        name == "node" -> Icons.Outlined.Dns
        "browser" in lower || "page" in lower || "browse" in lower -> Icons.Outlined.Language
        "clipboard" in lower || "copy" in lower -> Icons.Outlined.ContentCopy
        "image" in lower || "vision" in lower || "screenshot" in lower -> Icons.Outlined.Image
        else -> Icons.Outlined.Build
    }
}

@Composable
internal fun CardView(card: Card, repository: ChatRepository, compact: Boolean) {
    when (card.type) {
        "place" -> PlaceCard(card.data, compact)
        "link" -> LinkCard(card.data, onBrowse = LocalOpenLink.current ?: repository::openBrowser, compact)
        "action_buttons" -> ActionButtonRow(card.data, repository)
    }
}
