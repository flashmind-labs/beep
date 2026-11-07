package com.flashmind.beep.center

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun bytes(vararg values: Int) = ByteArray(values.size) { values[it].toByte() }
private fun text(value: String) = value.toByteArray()

class TermBytesTest {
    @Test fun ctrlCIs03() {
        assertArrayEquals(bytes(0x03), termBytes(TermKey.Char('c'), setOf(TermMod.Ctrl)))
    }

    @Test fun shiftUpIsCsi12A() {
        assertArrayEquals(bytes(0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x41), termBytes(TermKey.Up, setOf(TermMod.Shift)))
        assertArrayEquals(text("\u001b[1;5A"), termBytes(TermKey.Up, setOf(TermMod.Ctrl)))
        assertArrayEquals(text("\u001b[1;3D"), termBytes(TermKey.Left, setOf(TermMod.Alt)))
        assertArrayEquals(text("\u001b[B"), termBytes(TermKey.Down, emptySet()))
    }

    @Test fun altXIsEscX() {
        assertArrayEquals(bytes(0x1b, 0x78), termBytes(TermKey.Char('x'), setOf(TermMod.Alt)))
    }

    @Test fun stickyCtrlClearsAfterOneKey() {
        val keys = TermKeyState()
        keys.toggle(TermMod.Ctrl)
        assertTrue(TermMod.Ctrl in keys.mods)
        assertArrayEquals(bytes(0x03), keys.press(TermKey.Char('c')))
        assertTrue(keys.mods.isEmpty())
        assertArrayEquals(bytes(0x63), keys.press(TermKey.Char('c')))
    }

    @Test fun homeEndEscTabBytes() {
        assertArrayEquals(text("\u001b[H"), termBytes(TermKey.Home, emptySet()))
        assertArrayEquals(text("\u001b[F"), termBytes(TermKey.End, emptySet()))
        assertArrayEquals(bytes(0x1b), termBytes(TermKey.Esc, emptySet()))
        assertArrayEquals(bytes(0x09), termBytes(TermKey.Tab, emptySet()))
    }

    @Test fun armedCtrlAppliesToOneTypedByte() {
        val keys = TermKeyState()
        keys.toggle(TermMod.Ctrl)
        assertArrayEquals(bytes(0x03), keys.applyTyped(bytes(0x63)))
        assertTrue(keys.mods.isEmpty())
    }

    @Test fun emulatorRepliesAndPastesPassUnchanged() {
        val keys = TermKeyState()
        keys.toggle(TermMod.Ctrl)
        val reply = text("\u001b[?62;c")
        assertArrayEquals(reply, keys.applyTyped(reply))
        assertArrayEquals(text("ls"), keys.applyTyped(text("ls")))
        assertTrue(TermMod.Ctrl in keys.mods)
    }

    @Test fun inputThatIsNotUtf8IsDroppedAndSplitCharactersWait() {
        val input = Utf8Input()
        assertNull(input.text(bytes(0xff, 0x41)))
        val euro = text("€")
        assertEquals("", input.text(euro.copyOfRange(0, 2)))
        assertEquals("€", input.text(euro.copyOfRange(2, 3)))
        assertEquals("ok", input.text(text("ok")))
    }
}
