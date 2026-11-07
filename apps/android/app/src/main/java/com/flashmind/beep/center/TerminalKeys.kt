package com.flashmind.beep.center

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.Beep

private sealed interface KeyCell {
    data class Key(val title: String, val key: TermKey) : KeyCell
    data class Mod(val title: String, val mod: TermMod) : KeyCell
    data object Gap : KeyCell
}

/** Two rows of nine. The empty cells keep the arrows an inverted T. */
private val keyRows = listOf(
    listOf(KeyCell.Key("Esc", TermKey.Esc), KeyCell.Key("Tab", TermKey.Tab), KeyCell.Key("|", TermKey.Char('|')), KeyCell.Key("/", TermKey.Char('/')),
        KeyCell.Key("~", TermKey.Char('~')), KeyCell.Key("-", TermKey.Char('-')), KeyCell.Gap, KeyCell.Key("↑", TermKey.Up), KeyCell.Gap),
    listOf(KeyCell.Mod("Ctrl", TermMod.Ctrl), KeyCell.Mod("Alt", TermMod.Alt), KeyCell.Mod("Shift", TermMod.Shift), KeyCell.Key("Home", TermKey.Home),
        KeyCell.Key("End", TermKey.End), KeyCell.Gap, KeyCell.Key("←", TermKey.Left), KeyCell.Key("↓", TermKey.Down), KeyCell.Key("→", TermKey.Right)),
)

/** The key rows above the keyboard: 28dp keys, 4dp apart. */
@Composable
internal fun TerminalKeyBar(mods: Set<TermMod>, onKey: (TermKey) -> Unit, onMod: (TermMod) -> Unit) {
    Column(Modifier.fillMaxWidth().background(Color(0xFF17181B)).padding(start = 5.dp, end = 5.dp, top = 5.dp, bottom = 6.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp)) {
        keyRows.forEach { row ->
            Row(Modifier.fillMaxWidth().height(28.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                row.forEach { cell ->
                    when (cell) {
                        KeyCell.Gap -> Spacer(Modifier.weight(1f))
                        is KeyCell.Key -> KeyButton(cell.title, false, Modifier.weight(1f)) { onKey(cell.key) }
                        is KeyCell.Mod -> KeyButton(cell.title, cell.mod in mods, Modifier.weight(1f)) { onMod(cell.mod) }
                    }
                }
            }
        }
    }
}

@Composable
private fun KeyButton(title: String, on: Boolean, modifier: Modifier, onClick: () -> Unit) {
    Box(modifier.fillMaxHeight().background(if (on) Beep.colors.accent else Color(0xFF26272B), RoundedCornerShape(5.dp))
        .clickable(role = Role.Button, onClick = onClick).semantics { selected = on }, contentAlignment = Alignment.Center) {
        Text(title, fontSize = 11.sp, fontFamily = FontFamily.Monospace, maxLines = 1, color = if (on) Beep.colors.onAccent else Color(0xFFD0D3D8))
    }
}
