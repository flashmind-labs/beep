package com.flashmind.beep

import android.content.Context
import android.provider.Settings as SystemSettings
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.Shapes
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em

/** The looks a user can pick. The key is what gets stored, so never rename one. */
enum class BeepStyle(val key: String, val label: String) {
    MintQuiet("mint_quiet", "Mint Quiet"),
    SoftClay("soft_clay", "Soft Clay"),
    Bubblegum("bubblegum", "Bubblegum"),
    Rodeo("rodeo", "Rodeo");

    companion object {
        fun fromKey(key: String?) = entries.firstOrNull { it.key == key } ?: MintQuiet
    }
}

@Immutable
data class BeepColors(
    /** Screen background. */
    val page: Color,
    /** Cards, sheets, input fields. */
    val surface: Color,
    /** Quiet fill for chips, pills and code blocks. */
    val raised: Color,
    val ink: Color,
    val muted: Color,
    val dim: Color,
    /** Outline on pressable things. Transparent when the look has none. */
    val line: Color,
    /** Dividers and quiet card edges. */
    val softLine: Color,
    /** Hard shadow or clay ledge under surface-coloured pieces. */
    val ledge: Color,
    val user: Color,
    val onUser: Color,
    /** Links, connection state, brand marks. */
    val brand: Color,
    /** The main action, such as send. */
    val accent: Color,
    val onAccent: Color,
    val ok: Color,
    val warn: Color,
    val error: Color,
    /** Tool chip fills, cycled per call. */
    val tints: List<Color>,
    val code: Color,
    val onCode: Color,
)

@Immutable
data class BeepShapes(
    val radius: Dp,
    val smallRadius: Dp,
)

@Immutable
data class BeepFonts(
    val display: FontFamily,
    val displayWeight: FontWeight,
    val displayTracking: TextUnit,
    val body: FontFamily,
)

@Immutable
data class BeepTokens(val style: BeepStyle, val colors: BeepColors, val shapes: BeepShapes, val fonts: BeepFonts) {
    companion object {
        fun of(style: BeepStyle, dark: Boolean = false) = linearTokens(style, dark)
    }
}

@OptIn(ExperimentalTextApi::class)
private val Inter = FontFamily(listOf(400, 500, 600, 700).map { weight ->
    Font(R.font.inter, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight)))
})

internal const val LINEAR_ACCENT = 0xFF5E6AD2

/** Linear's dark palette, with a matching light palette and saved accent choices. */
private fun linearTokens(style: BeepStyle, dark: Boolean): BeepTokens {
    val page = Color(if (dark) 0xFF0F1011 else 0xFFF7F8FA)
    val surface = Color(if (dark) 0xFF151617 else 0xFFFFFFFF)
    val raised = Color(if (dark) 0xFF1C1D1F else 0xFFF0F1F4)
    val ink = Color(if (dark) 0xFFE3E4E6 else 0xFF202126)
    val border = Color(if (dark) 0xFF222326 else 0xFFE1E3E8)
    val accent = Color(when (style) {
        BeepStyle.MintQuiet -> LINEAR_ACCENT
        BeepStyle.SoftClay -> 0xFF8B6CD2
        BeepStyle.Bubblegum -> if (dark) 0xFFE18BC7 else 0xFF9C357F
        BeepStyle.Rodeo -> if (dark) 0xFF8A8F98 else 0xFF676C76
    })
    return BeepTokens(style, BeepColors(
        page = page, surface = surface, raised = raised, ink = ink,
        muted = Color(if (dark) 0xFF8A8F98 else 0xFF676C76),
        dim = Color(if (dark) 0xFF5B5F65 else 0xFF767B85),
        line = border, softLine = border, ledge = border,
        user = lerp(page, accent, 0.22f), onUser = ink,
        brand = Color(if (dark) 0xFF8A9CF0 else 0xFF4F5BC0), accent = accent, onAccent = Color.White,
        ok = Color(if (dark) 0xFF4CB782 else 0xFF268557),
        warn = Color(if (dark) 0xFFF2C94C else 0xFF946B00),
        error = Color(if (dark) 0xFFEB5757 else 0xFFC33838),
        tints = listOf(raised, surface, lerp(page, accent, 0.12f)), code = raised, onCode = ink,
    ), BeepShapes(radius = 8.dp, smallRadius = 6.dp),
        BeepFonts(Inter, FontWeight.SemiBold, 0.em, Inter))
}

val LocalBeep = staticCompositionLocalOf { linearTokens(BeepStyle.MintQuiet, false) }

object BeepSpace {
    val page = 16.dp
    val row = 12.dp
}

/** Shorthand for the current tokens. */
object Beep {
    val colors: BeepColors @Composable @ReadOnlyComposable get() = LocalBeep.current.colors
    val shapes: BeepShapes @Composable @ReadOnlyComposable get() = LocalBeep.current.shapes
    val fonts: BeepFonts @Composable @ReadOnlyComposable get() = LocalBeep.current.fonts
    val style: BeepStyle @Composable @ReadOnlyComposable get() = LocalBeep.current.style
}

/** The picked look, shared by every activity and saved on the device. */
object Appearance {
    private const val PREFS = "appearance"
    private const val KEY = "style"
    val style = mutableStateOf(BeepStyle.MintQuiet)
    private var loaded = false

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        style.value = BeepStyle.fromKey(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null))
    }

    fun set(context: Context, value: BeepStyle) {
        style.value = value
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, value.key).apply()
    }
}

@Composable
fun BeepTheme(content: @Composable () -> Unit) {
    val context = LocalContext.current
    remember { Appearance.load(context.applicationContext) }
    val dark = isSystemInDarkTheme()
    val tokens = BeepTokens.of(Appearance.style.value, dark)
    val c = tokens.colors
    val scheme = (if (dark) darkColorScheme() else lightColorScheme()).copy(
        primary = c.accent, onPrimary = c.onAccent, primaryContainer = c.user, onPrimaryContainer = c.onUser,
        secondary = c.accent, onSecondary = c.onAccent, secondaryContainer = c.user, onSecondaryContainer = c.onUser,
        background = c.page, onBackground = c.ink, surface = c.surface, onSurface = c.ink, surfaceTint = Color.Transparent,
        surfaceVariant = c.raised, onSurfaceVariant = c.muted,
        surfaceContainerLowest = c.surface, surfaceContainerLow = c.surface, surfaceContainer = c.surface,
        surfaceContainerHigh = c.surface, surfaceContainerHighest = c.raised,
        outline = c.softLine, outlineVariant = c.softLine, error = c.error,
        inverseSurface = c.ink, inverseOnSurface = c.page,
    )
    MaterialTheme(colorScheme = scheme, shapes = Shapes(
        extraSmall = RoundedCornerShape(4.dp), small = RoundedCornerShape(6.dp), medium = RoundedCornerShape(8.dp),
        large = RoundedCornerShape(8.dp), extraLarge = RoundedCornerShape(8.dp)), typography = typographyFor(tokens.fonts)) {
        CompositionLocalProvider(LocalBeep provides tokens, content = content)
    }
}

private fun typographyFor(fonts: BeepFonts): Typography {
    val base = Typography()
    fun TextStyle.body() = copy(fontFamily = fonts.body, letterSpacing = 0.sp)
    fun TextStyle.display() = copy(fontFamily = fonts.display, fontWeight = fonts.displayWeight, letterSpacing = fonts.displayTracking)
    return base.copy(
        displayLarge = base.displayLarge.display(), displayMedium = base.displayMedium.display(), displaySmall = base.displaySmall.display(),
        headlineLarge = base.headlineLarge.display(), headlineMedium = base.headlineMedium.display(), headlineSmall = base.headlineSmall.display(),
        titleLarge = base.titleLarge.display().copy(fontSize = 18.sp, lineHeight = 24.sp), titleMedium = base.titleMedium.body(), titleSmall = base.titleSmall.body(),
        bodyLarge = base.bodyLarge.body().copy(fontSize = 14.sp, lineHeight = 21.sp), bodyMedium = base.bodyMedium.body().copy(fontSize = 13.sp, lineHeight = 20.sp), bodySmall = base.bodySmall.body(),
        labelLarge = base.labelLarge.body(), labelMedium = base.labelMedium.body(), labelSmall = base.labelSmall.body(),
    )
}

/** Big headline text in the look's display face. */
@Composable
@ReadOnlyComposable
fun displayStyle(size: TextUnit, lineHeight: TextUnit = TextUnit.Unspecified): TextStyle {
    val f = Beep.fonts
    return TextStyle(fontFamily = f.display, fontWeight = f.displayWeight, fontSize = size,
        lineHeight = lineHeight, letterSpacing = f.displayTracking)
}

/** Text for headings. */
@Composable
@ReadOnlyComposable
fun displayText(text: String) = text

/** Returns true when the system "Remove animations" / "Animator duration scale = 0" setting is on. */
@Composable
fun reduceMotion(): Boolean {
    val context = LocalContext.current
    return remember(context) {
        try {
            SystemSettings.Global.getFloat(context.contentResolver, SystemSettings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
        } catch (_: Exception) { false }
    }
}

/** A bordered button with press feedback. */
@Composable
fun ChunkyButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    color: Color = Beep.colors.surface,
    contentColor: Color = Beep.colors.ink,
    shape: Shape = RoundedCornerShape(Beep.shapes.smallRadius),
    enabled: Boolean = true,
    contentPadding: PaddingValues = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
    onLongClick: (() -> Unit)? = null,
    content: @Composable RowScope.() -> Unit,
) {
    val t = LocalBeep.current
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    Row(
        modifier
            .alpha(if (!enabled) 0.45f else if (pressed) 0.75f else 1f)
            .border(1.dp, t.colors.softLine, shape)
            .clip(shape)
            .background(color)
            .combinedClickable(interactionSource = interaction, indication = null, enabled = enabled, onLongClick = onLongClick, onClick = onClick)
            .padding(contentPadding),
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CompositionLocalProvider(LocalContentColor provides contentColor) { content() }
    }
}

/** Frames content with Linear's border and surface. */
@Composable
fun Modifier.beepFrame(shape: Shape, fill: Color): Modifier =
    border(1.dp, Beep.colors.softLine, shape).clip(shape).background(fill)

/** A card for content (places, links, code). Not pressable on its own. */
@Composable
fun BeepCard(
    modifier: Modifier = Modifier,
    color: Color = Beep.colors.surface,
    shape: Shape = RoundedCornerShape(Beep.shapes.radius),
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.beepFrame(shape, color), content = content)
}

/**
 * Springs a piece in from slightly below and smaller. Plays once per [key]; pass
 * `play = false` for content that was already on screen.
 */
@Composable
fun Modifier.popIn(key: Any, play: Boolean = true, origin: TransformOrigin = TransformOrigin(0.5f, 1f)): Modifier {
    val noMotion = reduceMotion()
    val progress = remember(key) { Animatable(if (play && !noMotion) 0f else 1f) }
    LaunchedEffect(key) {
        progress.animateTo(1f, spring(dampingRatio = 0.55f, stiffness = 420f))
    }
    return graphicsLayer {
        val v = progress.value
        alpha = v.coerceIn(0f, 1f)
        val scale = 0.98f + 0.02f * v
        scaleX = scale; scaleY = scale
        translationY = (1 - v) * 4.dp.toPx()
        transformOrigin = origin
    }
}
