package com.flashmind.beep

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.LinkAnnotation
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
    @Test fun linkInsideBold() {
        val s = formattedMessage("1. **[Cassis](https://example.com/t/cassis/2952)**: routing", colors)
        assertEquals("1. Cassis: routing", s.text)
        val link = s.getLinkAnnotations(0, s.length).single()
        assertEquals("Cassis", s.text.substring(link.start, link.end))
        assertEquals("https://example.com/t/cassis/2952", (link.item as LinkAnnotation.Url).url)
        assertEquals("Cassis", styled("**[Cassis](https://example.com)**").single { it.second.fontWeight == FontWeight.SemiBold }.first)
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
    private fun math(text: String) = formattedMessage(text, colors).let { s ->
        s.getStringAnnotations("androidx.compose.foundation.text.inlineContent", 0, s.length).map { it.item } }
    @Test fun inlineMath() {
        assertEquals(listOf("x^2"), math("so \$x^2\$ grows"))
        assertEquals(listOf("a_1 * b_2"), math("\$a_1 * b_2\$"))
        assertEquals(listOf("\\frac{1}{2}"), math("half is \\(\\frac{1}{2}\\)."))
        assertEquals(listOf("E=mc^2"), math("\$\$E=mc^2\$\$ inline"))
        assertEquals("so \$x^2\$ grows", formattedMessage("so \$x^2\$ grows", colors).text)
        assertTrue(math("run `\$x\$` now").isEmpty())
    }
    @Test fun dollarsStayText() {
        assertTrue(math("costs \$5 and \$10").isEmpty())
        assertTrue(math("from \$5-\$10 a month").isEmpty())
        assertTrue(math("US\$5 or \$ 7\$").isEmpty())
    }
}
