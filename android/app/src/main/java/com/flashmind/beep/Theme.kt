package com.flashmind.beep

import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.graphics.drawscope.DrawScope
import android.content.Context
import android.provider.Settings as SystemSettings
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpOffset
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em

/** The looks a user can pick. The key is what gets stored, so never rename one. */
enum class BeepStyle(val key: String, val label: String) {
    MintQuiet("mint_quiet", "Mint Quiet"),
    SoftClay("soft_clay", "Soft Clay"),
    Bubblegum("bubblegum", "Bubblegum");

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
    val userLedge: Color,
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

/** How cards and message bubbles are set off from the page. */
enum class CardLook {
    /** Outline plus hard offset shadow, same as buttons. */
    Chunky,
    /** No outline, a solid ledge underneath. */
    Ledge,
    /** Thin soft border, no shadow. */
    Hairline,
}

@Immutable
data class BeepShapes(
    /** Outline width on pressable things. */
    val outline: Dp,
    /** Hard shadow offset. Pressing moves the piece onto it. */
    val drop: DpOffset,
    val radius: Dp,
    val smallRadius: Dp,
    val card: CardLook,
    /** Whether assistant replies sit in a bubble or read as plain text. */
    val botBubble: Boolean,
)

@Immutable
data class BeepFonts(
    val display: FontFamily,
    val displayWeight: FontWeight,
    val displayUppercase: Boolean,
    val displayTracking: TextUnit,
    val body: FontFamily,
)

@Immutable
data class BeepTokens(val style: BeepStyle, val colors: BeepColors, val shapes: BeepShapes, val fonts: BeepFonts) {
    companion object {
        fun of(style: BeepStyle) = when (style) {
            BeepStyle.MintQuiet -> MintQuiet
            BeepStyle.SoftClay -> SoftClay
            BeepStyle.Bubblegum -> Bubblegum
        }
    }
}

@OptIn(ExperimentalTextApi::class)
private fun variableFamily(resource: Int, vararg weights: Int, extra: List<FontVariation.Setting> = emptyList()) =
    FontFamily(weights.map { weight ->
        Font(resource, FontWeight(weight),
            variationSettings = FontVariation.Settings(*(listOf(FontVariation.weight(weight)) + extra).toTypedArray()))
    })

private val Bricolage = variableFamily(R.font.bricolage_grotesque, 500, 600, 700, 800,
    extra = listOf(FontVariation.Setting("opsz", 48f)))
private val Rubik = variableFamily(R.font.rubik, 400, 500, 600, 700)
private val Fredoka = variableFamily(R.font.fredoka, 400, 500, 600, 700)
private val Nunito = variableFamily(R.font.nunito, 400, 500, 600, 700, 800)
private val Sniglet = FontFamily(
    Font(R.font.sniglet_regular, FontWeight.Normal),
    Font(R.font.sniglet_extrabold, FontWeight.ExtraBold),
)

private val MintQuiet = BeepTokens(
    BeepStyle.MintQuiet,
    BeepColors(
        page = Color(0xFFF4F8F5), surface = Color.White, raised = Color(0xFFE9F2EC),
        ink = Color(0xFF10201A), muted = Color(0xFF5C6E64), dim = Color(0xFF94A49B),
        line = Color(0xFF1F3A2E), softLine = Color(0xFFCFE0D6), ledge = Color(0xFF1F3A2E),
        user = Color(0xFFD5F1E3), onUser = Color(0xFF10201A), userLedge = Color(0xFF1F3A2E),
        brand = Color(0xFF2F8A69), accent = Color(0xFFF07A6A), onAccent = Color(0xFF10201A),
        ok = Color(0xFF3B9B7A), warn = Color(0xFFE0A630), error = Color(0xFFD9473A),
        tints = listOf(Color(0xFFFDE7E2), Color(0xFFE2F4EA), Color(0xFFFFF4CC)),
        code = Color(0xFFECF3EE), onCode = Color(0xFF1C2E26),
    ),
    BeepShapes(outline = 2.dp, drop = DpOffset(3.dp, 3.dp), radius = 18.dp, smallRadius = 12.dp,
        card = CardLook.Hairline, botBubble = false),
    BeepFonts(Bricolage, FontWeight.ExtraBold, displayUppercase = false, displayTracking = (-0.03).em, body = Rubik),
)

private val SoftClay = BeepTokens(
    BeepStyle.SoftClay,
    BeepColors(
        page = Color(0xFFF2EEFA), surface = Color.White, raised = Color(0xFFEAE4F8),
        ink = Color(0xFF2E2547), muted = Color(0xFF7A7194), dim = Color(0xFFABA4C2),
        line = Color.Transparent, softLine = Color(0xFFE2DBF2), ledge = Color(0xFFDCD3F0),
        user = Color(0xFFFFD3B4), onUser = Color(0xFF4A2A12), userLedge = Color(0xFFF2BC96),
        brand = Color(0xFF6E52E0), accent = Color(0xFF8B6CFF), onAccent = Color.White,
        ok = Color(0xFF2FBF7A), warn = Color(0xFFE6A23C), error = Color(0xFFE0523F),
        tints = listOf(Color(0xFFE4DCFF), Color(0xFFFFE3D0), Color(0xFFD3F5E3)),
        code = Color(0xFFF1EDFA), onCode = Color(0xFF3A3058),
    ),
    BeepShapes(outline = 0.dp, drop = DpOffset(0.dp, 5.dp), radius = 24.dp, smallRadius = 16.dp,
        card = CardLook.Ledge, botBubble = true),
    BeepFonts(Fredoka, FontWeight.SemiBold, displayUppercase = false, displayTracking = (-0.01).em, body = Nunito),
)

private val Bubblegum = BeepTokens(
    BeepStyle.Bubblegum,
    BeepColors(
        page = Color(0xFFFAF4F0), surface = Color.White, raised = Color(0xFFF4ECE6),
        ink = Color(0xFF161616), muted = Color(0xFF6E6862), dim = Color(0xFFA39C95),
        line = Color(0xFF161616), softLine = Color(0xFFE6DDD5), ledge = Color(0xFF161616),
        user = Color(0xFFFF90E8), onUser = Color(0xFF161616), userLedge = Color(0xFF161616),
        brand = Color(0xFF2563EB), accent = Color(0xFFFFC900), onAccent = Color(0xFF161616),
        ok = Color(0xFF29CC6A), warn = Color(0xFFDCAE01), error = Color(0xFFE40C0C),
        tints = listOf(Color(0xFFDBEAFE), Color(0xFFFFD3B4), Color(0xFFC7F9C6)),
        code = Color(0xFFF7F0EA), onCode = Color(0xFF161616),
    ),
    BeepShapes(outline = 2.dp, drop = DpOffset(3.dp, 3.dp), radius = 16.dp, smallRadius = 10.dp,
        card = CardLook.Chunky, botBubble = true),
    BeepFonts(Sniglet, FontWeight.ExtraBold, displayUppercase = true, displayTracking = 0.em, body = FontFamily.Default),
)

val LocalBeep = staticCompositionLocalOf { MintQuiet }

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
    val tokens = BeepTokens.of(Appearance.style.value)
    val c = tokens.colors
    val scheme = lightColorScheme(
        primary = c.brand, onPrimary = Color.White, primaryContainer = c.user, onPrimaryContainer = c.onUser,
        secondary = c.brand, onSecondary = Color.White, secondaryContainer = c.user, onSecondaryContainer = c.onUser,
        background = c.page, onBackground = c.ink, surface = c.page, onSurface = c.ink,
        surfaceVariant = c.raised, onSurfaceVariant = c.muted,
        surfaceContainerLowest = c.surface, surfaceContainerLow = c.surface, surfaceContainer = c.surface,
        surfaceContainerHigh = c.surface, surfaceContainerHighest = c.raised,
        outline = c.softLine, outlineVariant = c.softLine, error = c.error,
        inverseSurface = c.ink, inverseOnSurface = c.page,
    )
    MaterialTheme(colorScheme = scheme, typography = typographyFor(tokens.fonts)) {
        CompositionLocalProvider(LocalBeep provides tokens, content = content)
    }
}

private fun typographyFor(fonts: BeepFonts): Typography {
    val base = Typography()
    fun TextStyle.body() = copy(fontFamily = fonts.body)
    fun TextStyle.display() = copy(fontFamily = fonts.display, fontWeight = fonts.displayWeight, letterSpacing = fonts.displayTracking)
    return base.copy(
        displayLarge = base.displayLarge.display(), displayMedium = base.displayMedium.display(), displaySmall = base.displaySmall.display(),
        headlineLarge = base.headlineLarge.display(), headlineMedium = base.headlineMedium.display(), headlineSmall = base.headlineSmall.display(),
        titleLarge = base.titleLarge.display(), titleMedium = base.titleMedium.body(), titleSmall = base.titleSmall.body(),
        bodyLarge = base.bodyLarge.body(), bodyMedium = base.bodyMedium.body(), bodySmall = base.bodySmall.body(),
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

/** Text for display use, uppercased when the look asks for it. */
@Composable
@ReadOnlyComposable
fun displayText(text: String) = if (Beep.fonts.displayUppercase) text.uppercase() else text

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

/** Ledge colour for a piece filled with [fill]: the look's ledge for plain surfaces, a darker shade otherwise. */
@Composable
@ReadOnlyComposable
private fun ledgeFor(fill: Color): Color {
    val t = LocalBeep.current
    return when {
        t.shapes.outline > 0.dp -> t.colors.ledge
        fill == t.colors.surface -> t.colors.ledge
        fill == t.colors.user -> t.colors.userLedge
        else -> lerp(fill, Color.Black, 0.18f)
    }
}

/** Draws [shape] in [color] behind the content, shifted by [offset]. */
private fun Modifier.hardShadow(shape: Shape, color: Color, offset: DrawScope.() -> Offset) = drawBehind {
    val o = offset()
    if (o == Offset.Zero) return@drawBehind
    val outline = shape.createOutline(size, layoutDirection, this)
    translate(o.x, o.y) { drawOutline(outline, color) }
}

/**
 * A button that sits on a hard shadow (or a clay ledge) and sinks onto it when pressed.
 * The shadow stays put while the face moves, which is what makes it feel like a key.
 */
@Composable
fun ChunkyButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    color: Color = Beep.colors.surface,
    contentColor: Color = Beep.colors.ink,
    shape: Shape = CircleShape,
    enabled: Boolean = true,
    contentPadding: PaddingValues = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
    content: @Composable RowScope.() -> Unit,
) {
    val t = LocalBeep.current
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val sink by animateFloatAsState(if (pressed && enabled) 1f else 0f,
        spring(dampingRatio = 0.55f, stiffness = Spring.StiffnessHigh), label = "chunky-sink")
    val drop = t.shapes.drop
    val ledge = ledgeFor(color)
    Row(
        modifier
            .graphicsLayer { translationX = drop.x.toPx() * sink; translationY = drop.y.toPx() * sink }
            .alpha(if (enabled) 1f else 0.45f)
            .hardShadow(shape, ledge) {
                if (!enabled) Offset.Zero else Offset(drop.x.toPx() * (1 - sink), drop.y.toPx() * (1 - sink))
            }
            .then(if (t.shapes.outline > 0.dp) Modifier.border(t.shapes.outline, t.colors.line, shape) else Modifier)
            .clip(shape)
            .background(color)
            .clickable(interactionSource = interaction, indication = null, enabled = enabled, onClick = onClick)
            .padding(contentPadding),
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CompositionLocalProvider(LocalContentColor provides contentColor) { content() }
    }
}

/** Sets a piece of content off from the page the way the current look does it. */
fun Modifier.cardFrame(look: CardLook, shape: Shape, fill: Color, ledge: Color, line: Color, softLine: Color,
                       outline: Dp, drop: DpOffset): Modifier = when (look) {
    CardLook.Chunky -> hardShadow(shape, ledge) { Offset(drop.x.toPx(), drop.y.toPx()) }
        .border(outline, line, shape).clip(shape).background(fill)
    CardLook.Ledge -> hardShadow(shape, ledge) { Offset(drop.x.toPx(), drop.y.toPx()) }
        .clip(shape).background(fill)
    CardLook.Hairline -> border(1.5.dp, softLine, shape).clip(shape).background(fill)
}

/** Frames a piece in the current look. */
@Composable
fun Modifier.beepFrame(shape: Shape, fill: Color, look: CardLook = Beep.shapes.card): Modifier {
    val t = LocalBeep.current
    return cardFrame(look, shape, fill, ledgeFor(fill), t.colors.line, t.colors.softLine, t.shapes.outline, t.shapes.drop)
}

/** The frame for things you touch, like the composer. Quiet looks still give these weight. */
@Composable
@ReadOnlyComposable
fun pressableLook(): CardLook = Beep.shapes.card.let { if (it == CardLook.Hairline) CardLook.Chunky else it }

/** A card for content (places, links, code). Not pressable on its own. */
@Composable
fun BeepCard(
    modifier: Modifier = Modifier,
    color: Color = Beep.colors.surface,
    shape: Shape = RoundedCornerShape(Beep.shapes.radius),
    look: CardLook = Beep.shapes.card,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.beepFrame(shape, color, look), content = content)
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
        val scale = 0.86f + 0.14f * v
        scaleX = scale; scaleY = scale
        translationY = (1 - v) * 10.dp.toPx()
        transformOrigin = origin
    }
}
