package com.flashmind.beep

import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.CodingErrorAction

/** A bounded plain-text view of shell output. Color and terminal title sequences stay out of the text. */
internal class TerminalText(private val columns: Int = 80, private val maxLines: Int = 1000) {
    private val decoder = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPLACE)
    private var pending = byteArrayOf()
    private val lines = mutableListOf(StringBuilder())
    private var row = 0
    private var column = 0
    private var savedRow = 0
    private var savedColumn = 0
    private var state = State.Text
    private val sequence = StringBuilder()
    private enum class State { Text, Escape, Csi, Osc, OscEscape }

    fun append(bytes: ByteArray): String {
        val input = ByteBuffer.wrap(pending + bytes)
        val output = CharBuffer.allocate(input.remaining().coerceAtLeast(1))
        decoder.decode(input, output, false)
        pending = ByteArray(input.remaining()).also { input.get(it) }
        output.flip()
        while (output.hasRemaining()) accept(output.get())
        return lines.joinToString("\n") { it.toString().trimEnd() }.trimEnd('\n')
    }

    private fun accept(c: Char) {
        when (state) {
            State.Text -> when (c) {
                '\u001b' -> state = State.Escape
                '\r' -> column = 0
                '\n' -> { column = 0; moveTo(row + 1) }
                '\b' -> column = (column - 1).coerceAtLeast(0)
                '\t' -> repeat(8 - column % 8) { put(' ') }
                else -> if (c >= ' ' && c != '\u007f') put(c)
            }
            State.Escape -> when (c) {
                '[' -> { sequence.clear(); state = State.Csi }
                ']' -> state = State.Osc
                '7' -> { savedRow = row; savedColumn = column; state = State.Text }
                '8' -> { moveTo(savedRow); column = savedColumn; state = State.Text }
                else -> state = State.Text
            }
            State.Csi -> if (c in '@'..'~') { command(c); state = State.Text }
                else if (sequence.length < 128) sequence.append(c) else state = State.Text
            State.Osc -> when (c) { '\u0007' -> state = State.Text; '\u001b' -> state = State.OscEscape }
            State.OscEscape -> { state = if (c == '\\') State.Text else State.Osc }
        }
    }

    private fun put(c: Char) {
        if (column >= columns) { column = 0; moveTo(row + 1) }
        val line = lines[row]
        while (line.length <= column) line.append(' ')
        line.setCharAt(column++, c)
    }

    private fun moveTo(target: Int) {
        row = target.coerceIn(0, maxLines)
        while (lines.size <= row) lines.add(StringBuilder())
        if (lines.size > maxLines) {
            lines.removeAt(0); row--; savedRow = (savedRow - 1).coerceAtLeast(0)
        }
    }

    private fun command(c: Char) {
        if (sequence.startsWith("?")) return
        val args = sequence.split(';').map { it.toIntOrNull() ?: 0 }
        val amount = (args.firstOrNull() ?: 0).coerceAtLeast(1)
        val screenTop = (lines.size - 24).coerceAtLeast(0)
        when (c) {
            'A' -> moveTo((row - amount).coerceAtLeast(screenTop))
            'B' -> moveTo((row + amount).coerceAtMost(screenTop + 23))
            'C' -> column = (column + amount).coerceAtMost(columns - 1)
            'D' -> column = (column - amount).coerceAtLeast(0)
            'G' -> column = (amount - 1).coerceAtMost(columns - 1)
            'H', 'f' -> { moveTo(screenTop + (amount - 1).coerceAtMost(23)); column = ((args.getOrNull(1) ?: 1) - 1).coerceIn(0, columns - 1) }
            'J' -> when (args.firstOrNull()) {
                2, 3 -> { lines.clear(); lines.add(StringBuilder()); row = 0; column = 0 }
                0 -> { lines[row].setLength(column.coerceAtMost(lines[row].length)); while (lines.size > row + 1) lines.removeAt(lines.lastIndex) }
            }
            'K' -> when (args.firstOrNull()) {
                2 -> lines[row].clear()
                1 -> { val line = lines[row]; for (i in 0 until minOf(column + 1, line.length)) line.setCharAt(i, ' ') }
                else -> lines[row].setLength(column.coerceAtMost(lines[row].length))
            }
            's' -> { savedRow = row; savedColumn = column }
            'u' -> { moveTo(savedRow); column = savedColumn }
        }
    }
}
