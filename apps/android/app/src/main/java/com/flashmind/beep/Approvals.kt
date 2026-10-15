package com.flashmind.beep

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** A tool call that waits for the user: what it will do, then Deny or Allow. Answered, it folds to one line. */
@Composable
fun ApprovalCard(prompt: ApprovalPrompt, onAnswer: (String) -> Unit, modifier: Modifier = Modifier) {
    val t = Beep.colors
    val approval = prompt.approval
    if (!prompt.waiting) {
        val outcome = when (prompt.answer) {
            "always" -> "Allowed, and always allowed from now on"
            "allow" -> "Allowed once"
            "deny" -> "Denied"
            else -> "Not answered, you wrote again, so not done"
        }
        Row(modifier.padding(horizontal = 4.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Icon(Icons.Outlined.Shield, null, Modifier.size(14.dp), tint = t.muted)
            Text("${approval.app}, ${approval.action}: $outcome", fontSize = 12.sp, color = t.muted)
        }
        return
    }
    var always by remember(prompt.queryId) { mutableStateOf(false) }
    BeepCard(modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                AppLogo(approval.app.lowercase().replace(" ", ""), approval.app, size = 32.dp)
                Spacer(Modifier.width(10.dp))
                Column(Modifier.weight(1f)) {
                    Text("${capitalize(approval.action)}?", fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = t.ink)
                    Text("${approval.app} asks before acting", fontSize = 12.sp, color = t.muted)
                }
                Icon(Icons.Outlined.Shield, null, Modifier.size(18.dp), tint = t.muted)
            }
            // Many records scroll inside the card, so Deny and Allow stay on screen.
            if (approval.fields.isNotEmpty()) Column(Modifier.fillMaxWidth().heightIn(max = 280.dp)
                .clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(t.raised)
                .verticalScroll(rememberScrollState()).padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                approval.fields.forEach { field ->
                    Row {
                        Text(field.label, fontSize = 12.sp, color = t.muted, modifier = Modifier.width(76.dp))
                        Text(field.value, fontSize = 13.sp, color = t.ink, lineHeight = 18.sp, modifier = Modifier.weight(1f))
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(Modifier.weight(1f).heightIn(min = 40.dp)
                    .clickable(remember { MutableInteractionSource() }, indication = null, role = Role.Checkbox) { always = !always },
                    verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(18.dp).clip(RoundedCornerShape(5.dp))
                        .then(if (always) Modifier.background(t.ok) else Modifier.border(1.5.dp, t.muted, RoundedCornerShape(5.dp))),
                        contentAlignment = Alignment.Center) {
                        if (always) Icon(Icons.Outlined.Check, null, Modifier.size(13.dp), tint = t.page)
                    }
                    Spacer(Modifier.width(8.dp))
                    Text("Always allow ${approval.action}", fontSize = 12.sp, color = t.muted, lineHeight = 16.sp)
                }
                ChunkyButton(onClick = { onAnswer("deny") }, contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
                    Text("Deny", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
                ChunkyButton(onClick = { onAnswer(if (always) "always" else "allow") }, color = t.accent, contentColor = t.onAccent,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
                    Text("Allow", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

private fun capitalize(text: String) = text.replaceFirstChar { it.uppercase() }
