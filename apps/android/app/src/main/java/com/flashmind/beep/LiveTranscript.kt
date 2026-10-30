package com.flashmind.beep

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Mic
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** Lines a collapsed live transcript shows. */
private const val SHOWN_LINES = 4

/**
 * The stretches of [messages] said aloud in live mode: each starts at a spoken prompt and runs through
 * its replies and the spoken prompts after it, up to a typed prompt or a routine. Keyed by the id of the
 * first message; a reply with a card still waiting for the user ends the stretch, so the card shows.
 */
internal fun liveStretches(messages: List<ChatMessage>): Map<String, List<ChatMessage>> {
    val stretches = LinkedHashMap<String, List<ChatMessage>>()
    var current: MutableList<ChatMessage>? = null
    for (message in messages) {
        val joins = when (message.role) {
            "user" -> message.spoken
            "assistant" -> current != null && message.approvals.none { it.waiting }
            else -> false
        }
        if (!joins) { current = null; continue }
        val stretch = current ?: mutableListOf<ChatMessage>().also { stretches[message.id] = it; current = it }
        stretch += message
    }
    return stretches
}

private sealed interface TranscriptLine {
    data class Said(val you: Boolean, val at: Long, val text: String) : TranscriptLine
    data class Step(val text: String) : TranscriptLine
}

private fun lines(stretch: List<ChatMessage>): List<TranscriptLine> {
    var at = 0L
    return stretch.flatMap { message ->
        if (message.role == "user") {
            at = message.sentAt
            listOf(TranscriptLine.Said(true, at, message.text))
        } else {
            val steps = message.toolCalls.filter { it.name !in QUIET_TOOLS }.map { TranscriptLine.Step(doneToolLabel(it)) }
            steps + listOfNotNull(message.text.takeIf { it.isNotBlank() }?.let { TranscriptLine.Said(false, at, it) })
        }
    }
}

private val timeFormat: DateTimeFormatter = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)

private fun clock(at: Long): String =
    if (at == 0L) "" else timeFormat.format(Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()))

/** A live stretch of the chat as one transcript: who said what, and the steps Beep took between. */
@Composable
internal fun LiveTranscript(stretch: List<ChatMessage>, modifier: Modifier = Modifier) {
    val t = Beep.colors
    val lines = remember(stretch) { lines(stretch) }
    var all by rememberSaveable(stretch.first().id) { mutableStateOf(false) }
    val prompts = stretch.filter { it.role == "user" && it.sentAt != 0L }
    val start = prompts.firstOrNull()?.sentAt ?: 0L
    val minutes = ((prompts.lastOrNull()?.sentAt ?: start) - start) / 60_000 + 1
    val cut = !all && lines.size > SHOWN_LINES
    Column(modifier.fillMaxWidth().beepFrame(RoundedCornerShape(Beep.shapes.radius), t.surface)) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.Outlined.Mic, null, Modifier.size(14.dp), tint = t.accent)
            Text("Live conversation", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            if (start != 0L) Text("${clock(start)} · $minutes min", fontSize = 13.sp, color = t.muted)
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(t.softLine))
        Column(Modifier.padding(start = 14.dp, end = 14.dp, top = 12.dp, bottom = if (lines.size > SHOWN_LINES) 0.dp else 12.dp)
            .then(if (cut) Modifier.heightIn(max = 400.dp).fade(t.surface) else Modifier),
            verticalArrangement = Arrangement.spacedBy(8.dp)) {
            (if (cut) lines.take(SHOWN_LINES) else lines).forEach { line ->
                when (line) {
                    is TranscriptLine.Step -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Box(Modifier.size(4.dp).background(t.dim, CircleShape))
                        Text(line.text, fontSize = 13.sp, color = t.dim)
                    }
                    is TranscriptLine.Said -> Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(if (line.you) "You" else "Beep", fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                                color = if (line.you) t.muted else t.accent)
                            Text(clock(line.at), fontSize = 11.sp, color = t.dim)
                        }
                        Text(line.text, fontSize = 15.sp, lineHeight = 21.sp)
                    }
                }
            }
        }
        if (lines.size > SHOWN_LINES) Text(if (all) "Show less" else "Show all ${lines.size} lines", fontSize = 13.sp, color = t.muted,
            modifier = Modifier.fillMaxWidth().clickable { all = !all }.padding(horizontal = 14.dp, vertical = 10.dp))
    }
}

/** Fades the bottom of a cut transcript into [color], so it reads as continuing. */
private fun Modifier.fade(color: Color): Modifier = drawWithContent {
    drawContent()
    val height = 32.dp.toPx()
    drawRect(Brush.verticalGradient(listOf(Color.Transparent, color), startY = size.height - height, endY = size.height),
        topLeft = Offset(0f, size.height - height))
}
