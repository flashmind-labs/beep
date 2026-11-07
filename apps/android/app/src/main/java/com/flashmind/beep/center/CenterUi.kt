package com.flashmind.beep.center

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.Beep
import androidx.compose.foundation.selection.toggleable

// The command center's building blocks, sized as in mockups/android/06-command-center.css.

enum class CcKind { Plain, Wait, Bad, Ok }

@Composable
internal fun CcKind.color(): Color = when (this) {
    CcKind.Plain -> Beep.colors.muted
    CcKind.Wait -> Beep.colors.warn
    CcKind.Bad -> Beep.colors.error
    CcKind.Ok -> Beep.colors.ok
}

/** The 48dp bar: a title on the left at a tab root, after a back button on a pushed screen. */
@Composable
internal fun CcTop(title: String, back: (() -> Unit)? = null, trailing: @Composable RowScope.() -> Unit = {}) {
    val line = Beep.colors.softLine
    Row(Modifier.fillMaxWidth().height(48.dp).drawWithContent {
        drawContent()
        drawLine(line, Offset(0f, size.height), Offset(size.width, size.height), strokeWidth = 1.dp.toPx())
    }.padding(start = if (back == null) 16.dp else 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        if (back != null) CcIconButton(Icons.AutoMirrored.Outlined.ArrowBack, "Back", onClick = back)
        Text(title, Modifier.weight(1f), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        trailing()
    }
}

@Composable
internal fun CcIconButton(icon: ImageVector, label: String, badge: Int = 0, onClick: () -> Unit) {
    Box(Modifier.size(36.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius)).clickable(role = Role.Button, onClick = onClick)
        .semantics { contentDescription = if (badge > 0) "$label, $badge" else label }, contentAlignment = Alignment.Center) {
        Icon(icon, null, Modifier.size(19.dp), tint = Beep.colors.muted)
        if (badge > 0) Text(badge.toString(), Modifier.align(Alignment.TopEnd).padding(top = 4.dp, end = 3.dp)
            .defaultMinSize(minWidth = 15.dp, minHeight = 15.dp).background(Beep.colors.warn, RoundedCornerShape(8.dp)).padding(horizontal = 4.dp),
            fontSize = 9.5.sp, fontWeight = FontWeight.Bold, color = Color(0xFF111111), textAlign = TextAlign.Center)
    }
}

@Composable
internal fun CcPill(onClick: () -> Unit, content: @Composable RowScope.() -> Unit) {
    Row(Modifier.height(26.dp).clip(CircleShape).border(1.dp, Beep.colors.softLine, CircleShape).clickable(role = Role.Button, onClick = onClick)
        .padding(horizontal = 9.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp), content = content)
}

/** A scrolling page body: 12dp insets, 14dp between blocks. */
@Composable
internal fun CcBody(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp), content = content)
}

@Composable
internal fun CcSection(title: String, count: Int? = null, action: String? = null, onAction: () -> Unit = {}, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(title, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = Beep.colors.muted)
                if (count != null) Text(count.toString(), fontSize = 12.sp, fontWeight = FontWeight.Medium, color = Beep.colors.dim)
            }
            if (action != null) Text(action, Modifier.clickable(role = Role.Button, onClick = onAction), fontSize = 12.sp, color = Beep.colors.brand)
        }
        content()
    }
}

/** Rows separated by 1dp lines inside a bordered, rounded surface. */
@Composable
internal fun CcPanel(content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(Beep.shapes.radius)
    Column(Modifier.fillMaxWidth().clip(shape).background(Beep.colors.surface).border(1.dp, Beep.colors.softLine, shape), content = content)
}

/** A 46dp row: a lead, a title with a muted one-line subtitle, and a right side. */
@Composable
internal fun CcRow(title: String, subtitle: String = "", mono: Boolean = false, chevron: Boolean = false, onClick: (() -> Unit)? = null,
                   lead: (@Composable () -> Unit)? = null, right: (@Composable RowScope.() -> Unit)? = null) {
    val line = Beep.colors.softLine
    Row(Modifier.fillMaxWidth().drawWithContent {
        drawContent()
        drawLine(line, Offset(0f, 0f), Offset(size.width, 0f), strokeWidth = 1.dp.toPx())
    }.then(if (onClick == null) Modifier else Modifier.clickable(role = Role.Button, onClick = onClick))
        .heightIn(min = 46.dp).padding(horizontal = 11.dp, vertical = 9.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        lead?.invoke()
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 13.sp, fontWeight = if (mono) FontWeight.Normal else FontWeight.Medium,
                fontFamily = if (mono) FontFamily.Monospace else null, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle.isNotEmpty()) Text(subtitle, fontSize = 11.5.sp, color = Beep.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            right?.invoke(this)
            if (chevron) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(14.dp), tint = Beep.colors.dim)
        }
    }
}

/** The 24dp square that leads a row: a short label, colored by kind. */
@Composable
internal fun CcKindTile(text: String, kind: CcKind = CcKind.Plain) {
    Box(Modifier.size(24.dp).background(Beep.colors.raised, RoundedCornerShape(6.dp)), contentAlignment = Alignment.Center) {
        Text(text, fontSize = 10.sp, fontWeight = FontWeight.Bold, color = kind.color())
    }
}

/** The agent's tile. These three color pairs are fixed, as in the mock. */
@Composable
internal fun CcAgentTile(agent: String) {
    val (text, back, fore) = when (agent) {
        "codex" -> Triple("CX", Color(0xFF1F2A3A), Color(0xFF8AB4F8))
        "gemini" -> Triple("GM", Color(0xFF20303A), Color(0xFF7DD3C0))
        else -> Triple(if (agent == "claude") "CC" else agent.take(2).uppercase(), Color(0xFF3B2A20), Color(0xFFF0A070))
    }
    Box(Modifier.size(24.dp).background(back, RoundedCornerShape(6.dp)), contentAlignment = Alignment.Center) {
        Text(text, fontSize = 9.sp, fontWeight = FontWeight.Bold, color = fore)
    }
}

@Composable
internal fun CcTag(text: String, kind: CcKind = CcKind.Plain) {
    val color = kind.color()
    Text(text, Modifier.height(20.dp).background(if (kind == CcKind.Plain) Beep.colors.raised else color.copy(alpha = 0.14f), CircleShape)
        .padding(horizontal = 7.dp).wrapContentHeight(Alignment.CenterVertically), fontSize = 11.sp, color = color, maxLines = 1)
}

enum class CcButtonStyle { Plain, Primary, Danger }

@Composable
internal fun CcButton(title: String, style: CcButtonStyle = CcButtonStyle.Plain, big: Boolean = false, leading: Boolean = false, onClick: () -> Unit) {
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    val primary = style == CcButtonStyle.Primary
    Box(Modifier.then(if (big || leading) Modifier.fillMaxWidth() else Modifier).height(if (big) 40.dp else if (leading) 36.dp else 30.dp)
        .clip(shape).background(if (primary) Beep.colors.accent else Beep.colors.raised)
        .border(1.dp, if (primary) Beep.colors.accent else Beep.colors.softLine, shape)
        .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 11.dp),
        contentAlignment = if (leading) Alignment.CenterStart else Alignment.Center) {
        Text(title, fontSize = if (big) 14.sp else 12.5.sp, fontWeight = FontWeight.Medium, maxLines = 1,
            color = when (style) { CcButtonStyle.Primary -> Beep.colors.onAccent; CcButtonStyle.Danger -> Beep.colors.error; else -> Beep.colors.ink })
    }
}

@Composable
internal fun <T> CcSegmented(items: List<Triple<T, String, Int?>>, selection: T, onSelect: (T) -> Unit) {
    val shape = RoundedCornerShape(Beep.shapes.radius)
    Row(Modifier.fillMaxWidth().background(Beep.colors.surface, shape).border(1.dp, Beep.colors.softLine, shape).padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        items.forEach { (value, label, count) ->
            val on = value == selection
            Row(Modifier.weight(1f).height(28.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius))
                .background(if (on) Beep.colors.raised else Color.Transparent)
                .clickable(role = Role.Tab) { onSelect(value) }.semantics { selected = on },
                horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                Text(label, fontSize = 12.5.sp, fontWeight = if (on) FontWeight.Medium else FontWeight.Normal, maxLines = 1,
                    color = if (on) Beep.colors.ink else Beep.colors.muted)
                if (count != null) Text(count.toString(), fontSize = 12.5.sp, color = Beep.colors.muted)
            }
        }
    }
}

@Composable
internal fun CcChips(items: List<String>, selection: String, onSelect: (String) -> Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items.forEach { item ->
            val on = item == selection
            Text(item, Modifier.height(28.dp).clip(CircleShape).background(if (on) Beep.colors.accent.copy(alpha = 0.16f) else Color.Transparent)
                .border(1.dp, if (on) Beep.colors.accent else Beep.colors.softLine, CircleShape)
                .clickable(role = Role.Tab) { onSelect(item) }.padding(horizontal = 10.dp).wrapContentHeight(Alignment.CenterVertically),
                fontSize = 12.sp, color = if (on) Beep.colors.ink else Beep.colors.muted)
        }
    }
}

@Composable
internal fun CcDot(online: Boolean = true, warn: Boolean = false) {
    Box(Modifier.size(7.dp).background(if (warn) Beep.colors.warn else if (online) Beep.colors.ok else Beep.colors.dim, CircleShape))
}

@Composable
internal fun CcToggle(checked: Boolean, label: String, onChange: (Boolean) -> Unit) {
    Box(Modifier.width(38.dp).height(22.dp).clip(CircleShape).background(if (checked) Beep.colors.accent else Beep.colors.raised)
        .border(1.dp, if (checked) Beep.colors.accent else Beep.colors.softLine, CircleShape)
        .toggleable(checked, role = Role.Switch, onValueChange = onChange).semantics { contentDescription = label }.padding(2.dp)) {
        Box(Modifier.align(if (checked) Alignment.CenterEnd else Alignment.CenterStart).size(16.dp)
            .background(if (checked) Beep.colors.onAccent else Beep.colors.muted, CircleShape))
    }
}

@Composable
internal fun CcBar(fraction: Double, warn: Boolean = false) {
    Box(Modifier.fillMaxWidth().height(4.dp).background(Beep.colors.raised, CircleShape)) {
        Box(Modifier.fillMaxWidth(fraction.coerceIn(0.0, 1.0).toFloat()).fillMaxHeight().background(if (warn) Beep.colors.warn else Beep.colors.accent, CircleShape))
    }
}

@Composable
internal fun CcEmpty(title: String, detail: String) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 22.dp), horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
        Text(detail, fontSize = 13.sp, color = Beep.colors.muted, textAlign = TextAlign.Center)
    }
}

@Composable
internal fun CcNote(text: String) {
    Text(text, fontSize = 12.sp, lineHeight = 18.sp, color = Beep.colors.muted)
}

/** A machine of the horizontal strip. */
@Composable
internal fun CcStripCell(name: String, detail: String, online: Boolean = true, onClick: () -> Unit) {
    val shape = RoundedCornerShape(Beep.shapes.radius)
    Column(Modifier.widthIn(min = 132.dp).clip(shape).background(Beep.colors.surface).border(1.dp, Beep.colors.softLine, shape)
        .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 10.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            CcDot(online)
            Text(name, fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Text(detail, fontSize = 11.5.sp, color = Beep.colors.muted, maxLines = 1)
    }
}
