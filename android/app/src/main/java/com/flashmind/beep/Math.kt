package com.flashmind.beep

import android.util.LruCache
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import ru.noties.jlatexmath.JLatexMathDrawable

/** The annotation tag `appendInlineContent` writes; each one's item is the LaTeX of an inline formula. */
private const val INLINE_CONTENT_TAG = "androidx.compose.foundation.text.inlineContent"

/** A formula laid out once: the drawable, its size in pixels, and how far its baseline sits from the top. */
private class Formula(val drawable: JLatexMathDrawable, val width: Int, val height: Int, val baseline: Int)

// Streaming re-renders the last paragraph on every token, so keep laid out formulas around.
private val formulas = LruCache<String, Formula>(64)

/** Lays out [latex], or returns null when it does not parse. */
private fun formula(latex: String, textSize: Float, color: Int): Formula? {
    val key = "$textSize|$color|$latex"
    formulas.get(key)?.let { return it }
    val drawable = try {
        JLatexMathDrawable.builder(latex).textSize(textSize).color(color).padding(0).build()
    } catch (e: Exception) {
        return null
    }
    val width = drawable.intrinsicWidth
    val height = drawable.intrinsicHeight
    if (width <= 0 || height <= 0) return null
    drawable.setBounds(0, 0, width, height)
    return Formula(drawable, width, height, height - drawable.icon().iconDepth).also { formulas.put(key, it) }
}

/**
 * Text from [formattedMessage] with its inline formulas drawn in place. A formula is
 * centred on the maths axis, a quarter em above the baseline, so `x^2` and `\frac{a}{b}`
 * both sit on the line. One that does not parse shows its source in monospace.
 */
@Composable
internal fun MathText(text: AnnotatedString, color: Color, fontSize: TextUnit, lineHeight: TextUnit, modifier: Modifier = Modifier) {
    val sources = remember(text) { text.getStringAnnotations(INLINE_CONTENT_TAG, 0, text.length).map { it.item }.distinct() }
    if (sources.isEmpty()) {
        Text(text, modifier, color = color, fontSize = fontSize, lineHeight = lineHeight)
        return
    }
    val density = LocalDensity.current
    val measurer = rememberTextMeasurer()
    val codeStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = fontSize * 0.88f, color = color)
    val inline = remember(sources, color, fontSize, density) {
        val textSize = with(density) { fontSize.toPx() }
        sources.associateWith { latex ->
            val f = formula(latex, textSize, color.toArgb())
            if (f == null) {
                val source = "\$$latex\$"
                val size = measurer.measure(source, codeStyle).size
                InlineTextContent(placeholder(density, size.width, size.height)) {
                    Text(source, style = codeStyle, softWrap = false)
                }
            } else {
                val axis = f.baseline - textSize / 4
                val half = maxOf(axis, f.height - axis)
                InlineTextContent(placeholder(density, f.width, (2 * half).toInt())) {
                    Canvas(Modifier.size(with(density) { f.width.toDp() }, with(density) { (2 * half).toDp() })) {
                        drawIntoCanvas { canvas ->
                            canvas.nativeCanvas.save()
                            canvas.nativeCanvas.translate(0f, half - axis)
                            f.drawable.draw(canvas.nativeCanvas)
                            canvas.nativeCanvas.restore()
                        }
                    }
                }
            }
        }
    }
    Text(text, modifier, color = color, fontSize = fontSize, lineHeight = lineHeight, inlineContent = inline)
}

private fun placeholder(density: Density, width: Int, height: Int) = with(density) {
    Placeholder(width.toSp(), height.toSp(), PlaceholderVerticalAlign.TextCenter)
}

/**
 * A display formula on its own line, centred, scrolling sideways when it is wider than
 * the reply. Its source shows instead while it is still streaming or when it does not parse.
 */
@Composable
internal fun MathBlockView(latex: String, closed: Boolean, color: Color, compact: Boolean) {
    val density = LocalDensity.current
    val fontSize = if (compact) 16.sp else 18.sp
    val f = remember(latex, closed, color, fontSize, density) {
        if (closed) formula(latex, with(density) { fontSize.toPx() }, color.toArgb()) else null
    }
    if (f == null) {
        Text(latex, Modifier.fillMaxWidth().background(Beep.colors.code, RoundedCornerShape(Beep.shapes.smallRadius))
            .padding(horizontal = 12.dp, vertical = 10.dp),
            fontFamily = FontFamily.Monospace, fontSize = if (compact) 13.sp else 14.sp, color = Beep.colors.onCode)
        return
    }
    Box(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(vertical = 4.dp), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(with(density) { f.width.toDp() }, with(density) { f.height.toDp() })) {
            drawIntoCanvas { f.drawable.draw(it.nativeCanvas) }
        }
    }
}
