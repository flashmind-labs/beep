package com.flashmind.beep.center

import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.CodingErrorAction

sealed interface TermKey {
    data object Esc : TermKey
    data object Tab : TermKey
    data object Up : TermKey
    data object Down : TermKey
    data object Left : TermKey
    data object Right : TermKey
    data object Home : TermKey
    data object End : TermKey
    data class Char(val value: kotlin.Char) : TermKey
}

enum class TermMod { Ctrl, Alt, Shift }

/** The bytes a key sends. Arrows, Home and End carry the xterm modifier number (Shift 2, Alt 3, Ctrl 5). */
fun termBytes(key: TermKey, mods: Set<TermMod>): ByteArray {
    var number = 1
    if (TermMod.Shift in mods) number += 1
    if (TermMod.Alt in mods) number += 2
    if (TermMod.Ctrl in mods) number += 4
    fun csi(final: kotlin.Char) = (if (number == 1) "\u001b[$final" else "\u001b[1;$number$final").toByteArray()
    return when (key) {
        TermKey.Esc -> byteArrayOf(0x1b)
        TermKey.Tab -> if (TermMod.Shift in mods) "\u001b[Z".toByteArray() else byteArrayOf(0x09)
        TermKey.Up -> csi('A')
        TermKey.Down -> csi('B')
        TermKey.Right -> csi('C')
        TermKey.Left -> csi('D')
        TermKey.Home -> csi('H')
        TermKey.End -> csi('F')
        is TermKey.Char -> {
            var out = (if (TermMod.Shift in mods) key.value.toString().uppercase() else key.value.toString()).toByteArray()
            // Ctrl with a letter or one of @ [ \ ] ^ _ is the control byte.
            if (TermMod.Ctrl in mods && out.size == 1 && out[0].toInt() in 0x40..0x7f) out = byteArrayOf((out[0].toInt() and 0x1f).toByte())
            if (TermMod.Alt in mods) byteArrayOf(0x1b) + out else out
        }
    }
}

/** Ctrl, Alt and Shift stay armed for one key. */
class TermKeyState {
    var mods: Set<TermMod> = emptySet()
        private set

    fun toggle(mod: TermMod) { mods = if (mod in mods) mods - mod else mods + mod }

    /**
     * Bytes from the soft keyboard. Only a lone printable ASCII byte takes the armed modifiers;
     * anything longer (pastes, mouse and device replies from the emulator) passes unchanged and
     * leaves them armed.
     */
    fun applyTyped(typed: ByteArray): ByteArray {
        if (mods.isEmpty() || typed.size != 1 || typed[0] < 0x20 || typed[0] >= 0x7f) return typed
        return press(TermKey.Char(typed[0].toInt().toChar()))
    }

    fun press(key: TermKey): ByteArray {
        val out = termBytes(key, mods)
        mods = emptySet()
        return out
    }
}

/**
 * Turns input bytes into text for keys(). A character split across chunks waits for its rest.
 * A chunk that is not UTF-8 (an X10 mouse report past column 95) cannot travel as text and is dropped.
 */
class Utf8Input {
    private val decoder = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)
    private var pending = ByteArray(0)

    fun text(bytes: ByteArray): String? {
        val input = ByteBuffer.wrap(pending + bytes)
        val output = CharBuffer.allocate(input.remaining() + 1)
        val result = decoder.decode(input, output, false)
        if (result.isError) { pending = ByteArray(0); decoder.reset(); return null }
        pending = ByteArray(input.remaining()).also { input.get(it) }
        output.flip()
        return output.toString()
    }
}
