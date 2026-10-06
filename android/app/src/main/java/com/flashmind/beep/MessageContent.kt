package com.flashmind.beep

import androidx.compose.runtime.LaunchedEffect
import android.content.ClipboardManager
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.ui.draw.drawBehind
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.draw.clip
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.drop

// Short names for the current theme's colors.
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val TextPrimary: Color @Composable @ReadOnlyComposable get() = Beep.colors.ink
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

private sealed class MsgBlock {
    data class Text(val content: String) : MsgBlock()
    data class Code(val language: String, val code: String) : MsgBlock()
    data class Table(val headers: List<String>, val rows: List<List<String>>) : MsgBlock()
    data class Quote(val content: String) : MsgBlock()
    data object Rule : MsgBlock()
    data class Picture(val url: String, val alt: String) : MsgBlock()
    /** Display math; [closed] is false while the closing delimiter has not streamed in yet. */
    data class Math(val latex: String, val closed: Boolean) : MsgBlock()
}

private val mathClosers = mapOf("$$" to "$$", "\\[" to "\\]")

private val imageToken = Regex("""!\[([^\]]*)]\((https://[^\s)]+|data:image/[^\s)]+)\)""")

private fun parseBlocks(text: String): List<MsgBlock> {
    val blocks = mutableListOf<MsgBlock>()
    val lines = text.split('\n')
    var i = 0
    val pending = StringBuilder()
    fun flush() { if (pending.isNotEmpty()) { blocks.add(MsgBlock.Text(pending.toString().trimEnd())); pending.clear() } }
    while (i < lines.size) {
        val line = lines[i]
        if (line.trimStart().startsWith("```")) {
            flush()
            val lang = line.trimStart().removePrefix("```").trim()
            i++
            val code = StringBuilder()
            while (i < lines.size && !lines[i].trimStart().startsWith("```")) { if (code.isNotEmpty()) code.append('\n'); code.append(lines[i]); i++ }
            if (i < lines.size) i++
            blocks.add(MsgBlock.Code(lang, code.toString()))
            continue
        }
        // $$ or \[ starting a line opens display math, up to the matching $$ or \].
        val opener = mathClosers.keys.firstOrNull { line.trimStart().startsWith(it) }
        if (opener != null) {
            val closer = mathClosers.getValue(opener)
            val first = line.trimStart().removePrefix(opener)
            val latex = StringBuilder()
            var rest: String? = null
            var j = i
            var body = first
            while (true) {
                val end = body.indexOf(closer)
                if (end >= 0) { latex.append(body, 0, end); rest = body.substring(end + closer.length); break }
                latex.append(body)
                if (++j >= lines.size) break
                latex.append('\n'); body = lines[j]
            }
            if (latex.isNotBlank()) {
                flush()
                blocks.add(MsgBlock.Math(latex.toString().trim(), rest != null))
                if (!rest.isNullOrBlank()) pending.append(rest.trim())
                i = j + 1; continue
            }
        }
        if (line.matches(Regex("^\\s*[-*_]{3,}\\s*$")) && pending.isBlank()) { flush(); blocks.add(MsgBlock.Rule); i++; continue }
        if (line.trimStart().startsWith("|") && line.trimEnd().endsWith("|")) {
            val tableLines = mutableListOf(line)
            var j = i + 1
            while (j < lines.size && lines[j].trimStart().startsWith("|") && lines[j].trimEnd().endsWith("|")) { tableLines.add(lines[j]); j++ }
            if (tableLines.size >= 3 && tableLines[1].contains("---")) {
                flush()
                val headers = tableLines[0].split("|").filter { it.isNotBlank() }.map { it.trim() }
                val rows = tableLines.drop(2).map { row -> row.split("|").filter { it.isNotBlank() }.map { it.trim() } }
                blocks.add(MsgBlock.Table(headers, rows)); i = j; continue
            }
        }
        if (line.trimStart().startsWith("> ") || line.trimStart() == ">") {
            flush()
            val quoteLines = mutableListOf<String>()
            while (i < lines.size && (lines[i].trimStart().startsWith("> ") || lines[i].trimStart() == ">")) {
                quoteLines.add(lines[i].trimStart().removePrefix("> ").removePrefix(">")); i++
            }
            blocks.add(MsgBlock.Quote(quoteLines.joinToString("\n"))); continue
        }
        // An image breaks the paragraph it sits in: text before it, the image, then the rest.
        if (imageToken.containsMatchIn(line)) {
            var rest = line
            while (true) {
                val match = imageToken.find(rest) ?: break
                val before = rest.substring(0, match.range.first)
                if (before.isNotBlank()) { if (pending.isNotEmpty()) pending.append('\n'); pending.append(before.trimEnd()) }
                flush()
                blocks.add(MsgBlock.Picture(match.groupValues[2], match.groupValues[1]))
                rest = rest.substring(match.range.last + 1).trimStart()
            }
            if (rest.isNotBlank()) pending.append(rest)
            i++; continue
        }
        if (pending.isNotEmpty()) pending.append('\n')
        pending.append(line); i++
    }
    flush()
    return blocks
}

/** Colors the inline formatter needs, read from the theme once per composition. */
internal data class InlineColors(val link: Color, val muted: Color, val code: Color, val onCode: Color)

@Composable
@ReadOnlyComposable
private fun inlineColors() = Beep.colors.let { InlineColors(it.brand, it.muted, it.code, it.onCode) }

@Composable
internal fun MessageContent(text: String, role: String, compact: Boolean, padded: Boolean = role == "user") {
    val blocks = remember(text) { parseBlocks(text) }
    val inline = inlineColors()
    val color = if (role == "user") Beep.colors.onUser else TextPrimary
    val padding = if (padded) PaddingValues(horizontal = if (compact) 12.dp else 16.dp, vertical = if (compact) 9.dp else 12.dp)
        else PaddingValues(horizontal = 2.dp, vertical = 2.dp)
    // One container for the whole message, so a selection can run across paragraphs, code and quotes.
    SelectionContainer { Column(Modifier.padding(padding), verticalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 10.dp)) {
        blocks.forEach { block ->
            when (block) {
                is MsgBlock.Text -> MathText(remember(block.content, inline) { formattedMessage(block.content, inline) }, color = color,
                    fontSize = if (compact) 14.sp else 16.sp, lineHeight = if (compact) 21.sp else 25.sp)
                is MsgBlock.Math -> MathBlockView(block.latex, block.closed, color, compact)
                is MsgBlock.Code -> CodeBlockView(block.language, block.code, compact)
                is MsgBlock.Table -> TableView(block.headers, block.rows, compact)
                is MsgBlock.Quote -> BlockQuoteView(block.content, compact)
                MsgBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = 6.dp), color = Border)
                is MsgBlock.Picture -> ReplyImage(block.url, block.alt, compact)
            }
        }
    } }
}

@Composable
private fun CodeBlockView(language: String, code: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(Beep.colors.code)) {
        DisableSelection { Row(Modifier.fillMaxWidth().padding(start = 14.dp, end = 8.dp, top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(language.ifBlank { "code" }, fontSize = 11.sp, color = TextMuted, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Medium)
            Spacer(Modifier.weight(1f))
            Row(Modifier.clip(CircleShape).clickable {
                (context as? MainActivity)?.suppressNextClipboard = true
                context.getSystemService(android.content.ClipboardManager::class.java)
                    .setPrimaryClip(android.content.ClipData.newPlainText("Code", code))
                copied = true
            }.padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, null,
                    Modifier.size(13.dp), tint = if (copied) Beep.colors.ok else TextMuted)
                Spacer(Modifier.width(5.dp))
                Text(if (copied) "Copied" else "Copy", fontSize = 11.sp, color = if (copied) Beep.colors.ok else TextMuted)
            }
        } }
        Text(code, modifier = Modifier.horizontalScroll(rememberScrollState()).padding(start = 14.dp, end = 14.dp, top = 4.dp, bottom = 14.dp),
            fontFamily = FontFamily.Monospace, fontSize = if (compact) 12.sp else 13.sp,
            lineHeight = if (compact) 18.sp else 20.sp, color = Beep.colors.onCode)
    }
}

@Composable
private fun TableView(headers: List<String>, rows: List<List<String>>, compact: Boolean) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius))
        .border(1.5.dp, Border, RoundedCornerShape(Beep.shapes.smallRadius)).background(Surface)
        .horizontalScroll(rememberScrollState())) {
        Row(Modifier.background(Beep.colors.code).padding(vertical = 10.dp, horizontal = 4.dp)) {
            headers.forEach { header ->
                Text(header, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                    fontSize = if (compact) 11.sp else 12.sp, fontWeight = FontWeight.SemiBold,
                    color = TextPrimary, maxLines = 2)
            }
        }
        rows.forEachIndexed { index, row ->
            if (index > 0) HorizontalDivider(color = Border, thickness = 1.dp)
            Row(Modifier.padding(vertical = 9.dp, horizontal = 4.dp)) {
                row.forEach { cell ->
                    Text(cell, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                        fontSize = if (compact) 11.sp else 12.sp, color = TextPrimary,
                        lineHeight = if (compact) 16.sp else 18.sp, maxLines = 4)
                }
            }
        }
    }
}

@Composable
private fun BlockQuoteView(content: String, compact: Boolean) {
    val bar = Beep.colors.user
    val inline = inlineColors()
    Box(Modifier.fillMaxWidth().drawBehind {
        drawRoundRect(bar, size = androidx.compose.ui.geometry.Size(4.dp.toPx(), size.height),
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(2.dp.toPx()))
    }) {
        MathText(remember(content, inline) { formattedMessage(content, inline) },
            modifier = Modifier.padding(start = 16.dp, end = 10.dp, top = 4.dp, bottom = 4.dp),
            fontSize = if (compact) 13.sp else 15.sp, lineHeight = if (compact) 19.sp else 23.sp,
            color = TextMuted)
    }
}

// Links, bare urls, ***bold italic***, **bold**, *italic*, _italic_, ~~strike~~, `code`, and math as $$x$$, $x$ or
// \(x\). The leftmost match wins, so asterisks inside code or math stay literal; italics need a non-space just
// inside and no word character just outside, which keeps `2 * 3 * 4`, snake_case and __dunder__ plain. A single $
// needs a non-space just inside and no digit just after the closing one, so "$5 and $10" stays text.
private val inlineTokens = Regex("""\[([^\]]+)\]\((https?://[^\s)]+)\)|(https?://[^\s<>]+)|\*\*\*(?!\s)(.+?)(?<!\s)\*\*\*|\*\*(?!\s)(.+?)(?<!\s)\*\*""" +
    """|(?<![\w*])\*(?![\s*])(.+?)(?<![\s*])\*(?![\w*])|(?<![\w_])_(?![\s_])(.+?)(?<![\s_])_(?![\w_])|~~(.+?)~~|`([^`\n]+)`""" +
    """|\$\$([^$\n]+?)\$\$|(?<![\\$\w])\$(?![\s$])([^$\n]+?)(?<![\s\\])\$(?![\d$\w])|\\\((.+?)\\\)""")

// Inline markdown plus headings and bullets, line by line.
internal fun formattedMessage(text: String, colors: InlineColors) = buildAnnotatedString {
    val lines = text.split('\n')
    var firstLine = true
    for (line in lines) {
        if (!firstLine) append("\n")
        firstLine = false
        val heading = Regex("^#{1,4} +").find(line)
        val content = line.removePrefix(heading?.value.orEmpty())
            .replace(Regex("^( *)[-*+] +"), "$1\u2022 ")
        pushStyle(if (heading != null) SpanStyle(fontWeight = FontWeight.Bold, fontSize = 17.sp, letterSpacing = (-0.3).sp) else SpanStyle())
        var offset = 0
        for (match in inlineTokens.findAll(content)) {
            append(content.substring(offset, match.range.first))
            when {
                match.groups[4] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, fontStyle = FontStyle.Italic)) { append(match.groups[4]!!.value) }
                match.groups[5] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(match.groups[5]!!.value) }
                match.groups[6] != null || match.groups[7] != null -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append((match.groups[6] ?: match.groups[7])!!.value) }
                match.groups[8] != null -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough, color = colors.muted)) { append(match.groups[8]!!.value) }
                match.groups[10] != null || match.groups[11] != null || match.groups[12] != null -> {
                    val latex = (match.groups[10] ?: match.groups[11] ?: match.groups[12])!!.value.trim()
                    appendInlineContent(latex, "\$$latex\$")
                }
                match.groups[9] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = colors.code, color = colors.onCode, fontSize = 14.sp)) { append("\u200A${match.groups[9]!!.value}\u200A") }
                else -> {
                    val url = (match.groups[2]?.value ?: match.groups[3]!!.value).trimEnd('.', ',', ';')
                    withLink(LinkAnnotation.Url(url, TextLinkStyles(style = SpanStyle(color = colors.link, textDecoration = TextDecoration.Underline)))) {
                        append(match.groups[1]?.value ?: url)
                    }
                    if (match.groups[3] != null) append(match.value.removePrefix(url))
                }
            }
            offset = match.range.last + 1
        }
        append(content.substring(offset)); pop()
    }
}
