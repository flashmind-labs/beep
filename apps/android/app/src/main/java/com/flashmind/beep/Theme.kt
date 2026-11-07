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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
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
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
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
data class BeepTokens(val colors: BeepColors, val shapes: BeepShapes, val fonts: BeepFonts) {
    companion object {
        fun of(skin: Skin) = BeepTokens(skin.colors, BeepShapes(skin.radius, skin.radius * 0.75f), BeepFonts(Inter, FontWeight.SemiBold, 0.em, Inter))
    }
}

@OptIn(ExperimentalTextApi::class)
private val Inter = FontFamily(listOf(400, 500, 600, 700).map { weight ->
    Font(R.font.inter, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight)))
})

internal const val LINEAR_ACCENT = 0xFF5E6AD2

/** A desktop skin's tokens as the phone's colors. */
internal fun skinColors(page: Long, surface: Long, raised: Long, ink: Long, muted: Long, dim: Long, line: Long,
    accent: Long, onAccent: Long, ok: Long, warn: Long, error: Long, brand: Long, user: Long, tint: Long) = BeepColors(
    page = Color(page), surface = Color(surface), raised = Color(raised), ink = Color(ink),
    muted = Color(muted), dim = Color(dim), line = Color(line), softLine = Color(line), ledge = Color(line),
    user = Color(user), onUser = Color(ink), brand = Color(brand), accent = Color(accent), onAccent = Color(onAccent),
    ok = Color(ok), warn = Color(warn), error = Color(error),
    tints = listOf(Color(raised), Color(surface), Color(tint)), code = Color(raised), onCode = Color(ink),
)

/** One of the desktop's skins. */
@Immutable
data class Skin(val id: String, val name: String, val light: Boolean, val colors: BeepColors, val radius: Dp)

object Skins {
    const val SYSTEM = "system"
    val all: List<Skin> get() = skinTable

    /** The picked skin; "system", and any old or unknown value, is Linear in dark mode and Light in light mode. */
    fun resolve(choice: String?, dark: Boolean): Skin {
        val id = if (choice == null || all.none { it.id == choice }) (if (dark) "linear" else "paper") else choice
        return all.first { it.id == id }
    }
}

val LocalBeep = staticCompositionLocalOf { BeepTokens.of(Skins.resolve(Skins.SYSTEM, true)) }

object BeepSpace {
    val page = 16.dp
    val row = 12.dp
}

/** Shorthand for the current tokens. */
object Beep {
    val colors: BeepColors @Composable @ReadOnlyComposable get() = LocalBeep.current.colors
    val shapes: BeepShapes @Composable @ReadOnlyComposable get() = LocalBeep.current.shapes
    val fonts: BeepFonts @Composable @ReadOnlyComposable get() = LocalBeep.current.fonts
}

/** The picked skin, shared by every activity and saved on the device. */
object Appearance {
    private const val PREFS = "appearance"
    private const val KEY = "style"
    val choice = mutableStateOf(Skins.SYSTEM)
    private var loaded = false

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        choice.value = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null) ?: Skins.SYSTEM
    }

    fun set(context: Context, value: String) {
        choice.value = value
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, value).apply()
    }
}

@Composable
fun BeepTheme(content: @Composable () -> Unit) {
    Appearance.load(LocalContext.current)
    val skin = Skins.resolve(Appearance.choice.value, isSystemInDarkTheme())
    val tokens = BeepTokens.of(skin)
    val c = tokens.colors
    val scheme = (if (skin.light) lightColorScheme() else darkColorScheme()).copy(
        primary = c.accent, onPrimary = c.onAccent, primaryContainer = c.user, onPrimaryContainer = c.onUser,
        secondary = c.accent, onSecondary = c.onAccent, secondaryContainer = c.user, onSecondaryContainer = c.onUser,
        background = c.page, onBackground = c.ink, surface = c.surface, onSurface = c.ink, surfaceTint = Color.Transparent,
        surfaceVariant = c.raised, onSurfaceVariant = c.muted,
        surfaceContainerLowest = c.surface, surfaceContainerLow = c.surface, surfaceContainer = c.surface,
        surfaceContainerHigh = c.surface, surfaceContainerHighest = c.raised,
        outline = c.softLine, outlineVariant = c.softLine, error = c.error,
        inverseSurface = c.ink, inverseOnSurface = c.page,
    )
    val r = tokens.shapes.radius
    MaterialTheme(colorScheme = scheme, shapes = Shapes(
        extraSmall = RoundedCornerShape(4.dp), small = RoundedCornerShape(tokens.shapes.smallRadius), medium = RoundedCornerShape(r),
        large = RoundedCornerShape(r), extraLarge = RoundedCornerShape(r)), typography = typographyFor(tokens.fonts)) {
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

/** The mock's compact header and quiet bottom divider. */
@Composable
internal fun Modifier.beepBar(): Modifier {
    val line = Beep.colors.softLine
    return drawWithContent {
        drawContent()
        drawLine(line, androidx.compose.ui.geometry.Offset(0f, size.height),
            androidx.compose.ui.geometry.Offset(size.width, size.height), strokeWidth = 1.dp.toPx())
    }
}

/** A compact visual switch inside a full touch target. */
@Composable
internal fun BeepSwitch(checked: Boolean, onCheckedChange: (Boolean) -> Unit, label: String,
    modifier: Modifier = Modifier, enabled: Boolean = true) {
    androidx.compose.foundation.layout.Box(modifier.size(48.dp).alpha(if (enabled) 1f else 0.45f)
        .semantics { contentDescription = label }
        .toggleable(value = checked, enabled = enabled, role = androidx.compose.ui.semantics.Role.Switch, onValueChange = onCheckedChange),
        contentAlignment = Alignment.Center) {
        androidx.compose.foundation.layout.Box(Modifier.size(width = 40.dp, height = 24.dp)
            .background(if (checked) Beep.colors.ok else Beep.colors.softLine, androidx.compose.foundation.shape.CircleShape)
            .border(1.dp, Beep.colors.softLine, androidx.compose.foundation.shape.CircleShape)) {
            androidx.compose.foundation.layout.Box(Modifier.align(Alignment.CenterStart).offset(x = if (checked) 20.dp else 3.dp)
                .size(17.dp).background(Beep.colors.surface, androidx.compose.foundation.shape.CircleShape)
                .border(1.dp, Beep.colors.softLine, androidx.compose.foundation.shape.CircleShape))
        }
    }
}
