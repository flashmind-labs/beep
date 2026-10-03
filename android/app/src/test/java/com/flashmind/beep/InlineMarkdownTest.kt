package com.flashmind.beep

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import org.junit.Assert.*
import org.junit.Test

class InlineMarkdownTest {
    private val colors = InlineColors(Color.Blue, Color.Gray, Color.Black, Color.White)
    private fun styled(text: String) = formattedMessage(text, colors).let { s -> s.spanStyles.filter { it.start < it.end }.map { s.text.substring(it.start, it.end) to it.item } }
    private fun italics(text: String) = styled(text).filter { it.second.fontStyle == FontStyle.Italic }.map { it.first }

    @Test fun singleAsterisksAndUnderscoresAreItalic() {
        assertEquals("an italic word", formattedMessage("an *italic* word", colors).text)
        assertEquals(listOf("italic"), italics("an *italic* word"))
        assertEquals(listOf("also italic"), italics("and _also italic_ here"))
    }
    @Test fun boldAndBoldItalic() {
        val bold = styled("a **bold** b")
        assertEquals("bold", bold.single { it.second.fontWeight == FontWeight.SemiBold }.first)
        assertTrue(italics("a **bold** b").isEmpty())
        val both = styled("***both***").single { it.second.fontWeight == FontWeight.SemiBold }
        assertEquals("both" to FontStyle.Italic, both.first to both.second.fontStyle)
    }
    @Test fun strikethrough() {
        assertEquals(listOf("gone"), styled("~~gone~~ kept").filter { it.second.textDecoration == TextDecoration.LineThrough }.map { it.first })
    }
    @Test fun notItalic() {
        assertTrue(italics("snake_case_name and __init__").isEmpty())
        assertTrue(italics("2 * 3 * 4").isEmpty())
        assertTrue(italics("run `a *b* c` now").isEmpty())
        assertEquals("• item one", formattedMessage("* item one", colors).text)
        assertEquals("  • nested x", formattedMessage("  * nested *x*", colors).text)
        assertEquals(listOf("x"), italics("* item *x*"))
    }
}
