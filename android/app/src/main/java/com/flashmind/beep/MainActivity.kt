package com.flashmind.beep

import com.flashmind.beep.proto.MemorySource
import kotlin.math.roundToInt
import androidx.activity.compose.BackHandler
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.draw.alpha
import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.content.pm.PackageManager
import android.location.Geocoder
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.animation.*
import androidx.compose.animation.core.EaseInOutCubic
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import kotlin.math.abs
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.activity.compose.rememberLauncherForActivityResult
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.graphics.Outline
import android.view.Gravity
import android.view.View
import android.view.ViewOutlineProvider
import android.view.WindowInsets
import android.util.Base64
import androidx.compose.foundation.Image
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import android.provider.Settings as SystemSettings
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalDate
import java.time.format.DateTimeFormatter

private fun java.io.InputStream.readProfileBytes(): ByteArray {
    val buffer = ByteArray(65537)
    var count = 0
    while (count < buffer.size) {
        val read = read(buffer, count, buffer.size - count)
        if (read < 0) break
        count += read
    }
    return buffer.copyOf(count)
}

// Short names for the current theme's colors.
/** Mirrors the server's MEMORY_THRESHOLD: above it the learner reads the turn. */
private const val MEMORY_THRESHOLD = 0.8
private val MEMORY_TOOLS = setOf("memory_store", "memory_update", "memory_forget")
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Elevated: Color @Composable @ReadOnlyComposable get() = Beep.colors.raised
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val Coral: Color @Composable @ReadOnlyComposable get() = Beep.colors.error
private val TextPrimary: Color @Composable @ReadOnlyComposable get() = Beep.colors.ink
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted
private val TextDim: Color @Composable @ReadOnlyComposable get() = Beep.colors.dim

/** Load, resize to at most [maxSize] px on the longest side, and JPEG-encode as a base64 data URI. */
internal fun encodeImageUri(context: Context, uri: Uri, maxSize: Int = 1024): String? {
    return try {
        val bitmap = if (Build.VERSION.SDK_INT >= 28) {
            ImageDecoder.decodeBitmap(ImageDecoder.createSource(context.contentResolver, uri)) { decoder, _, _ ->
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                decoder.isMutableRequired = true
            }
        } else {
            @Suppress("DEPRECATION")
            android.provider.MediaStore.Images.Media.getBitmap(context.contentResolver, uri)
        }
        val scale = maxSize.toFloat() / maxOf(bitmap.width, bitmap.height).coerceAtLeast(1)
        val resized = if (scale < 1f) {
            Bitmap.createScaledBitmap(bitmap, (bitmap.width * scale).toInt(), (bitmap.height * scale).toInt(), true)
        } else bitmap
        val stream = java.io.ByteArrayOutputStream()
        resized.compress(Bitmap.CompressFormat.JPEG, 80, stream)
        val encoded = Base64.encodeToString(stream.toByteArray(), Base64.NO_WRAP)
        "data:image/jpeg;base64,$encoded"
    } catch (_: Exception) { null }
}

/** Decode a base64 data URI to a Bitmap for display. */
internal fun decodeImageBase64(dataUri: String): Bitmap? {
    return try {
        val b64 = dataUri.substringAfter(",", "")
        if (b64.isEmpty()) return null
        val bytes = Base64.decode(b64, Base64.DEFAULT)
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
    } catch (_: Exception) { null }
}

open class MainActivity : ComponentActivity() {
    protected open val bubble = false
    protected open val panel = false
    /** Set while the user is granting "Display over other apps" so Beep floats when they come back. */
    private var pendingFloat = false
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (it && SystemBubble.show(this)) moveTaskToBack(true) else SystemBubble.settings(this)
    }
    private val locationPermission = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { grants ->
        if (grants.values.any { it }) startLocationUpdates()
    }
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private val pendingFocusInput = mutableStateOf(false)
    private val pendingOpenApps = mutableStateOf(false)
    private val clipboardContent = mutableStateOf<String?>(null)
    /** Set to true right before the app copies text, to suppress showing the banner for our own copies. */
    var suppressNextClipboard = false
    private var clipboardListener: ClipboardManager.OnPrimaryClipChangedListener? = null
    @android.annotation.SuppressLint("MissingPermission")
    fun startLocationUpdates() {
        val repository = (application as BeepApplication).repository
        val locationManager = getSystemService(LocationManager::class.java) ?: return
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED) return
        val provider = when {
            Build.VERSION.SDK_INT >= 31 && locationManager.isProviderEnabled(LocationManager.FUSED_PROVIDER) -> LocationManager.FUSED_PROVIDER
            locationManager.isProviderEnabled(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
            locationManager.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
            else -> return
        }
        try {
            locationManager.requestLocationUpdates(provider, 60 * 1000L, 25f, object : LocationListener {
                override fun onLocationChanged(location: Location) { latestFix = location }
            })
        } catch (_: Exception) {}
        repository.startLocationUpdates {
            val loc = latestFix ?: try {
                locationManager.getLastKnownLocation(provider)
                    ?: locationManager.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?: locationManager.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            } catch (_: Exception) { null }
            loc?.let { ChatRepository.Fix(it.latitude, it.longitude, it.accuracy, placeName(it.latitude, it.longitude)) }
        }
    }
    @Volatile private var latestFix: Location? = null
    private var namedPlace: Pair<String, String?>? = null

    /** "Shinjuku, Tokyo, Japan" from the phone's geocoder, reused within about 100 m. */
    @Suppress("DEPRECATION")
    private suspend fun placeName(latitude: Double, longitude: Double): String? {
        val key = "%.3f,%.3f".format(java.util.Locale.ROOT, latitude, longitude)
        namedPlace?.takeIf { it.first == key }?.let { return it.second }
        if (!Geocoder.isPresent()) return null
        val address = withContext(Dispatchers.IO) {
            runCatching { Geocoder(this@MainActivity).getFromLocation(latitude, longitude, 1)?.firstOrNull() }.getOrNull()
        } ?: return null
        val name = listOfNotNull(address.locality ?: address.subAdminArea ?: address.subLocality, address.adminArea, address.countryName)
            .filter { it.isNotBlank() }.distinct().joinToString(", ").ifEmpty { null }
        namedPlace = key to name
        return name
    }
    /** A notice about an app, or the `beep://apps` link Composio returns to after sign-in. */
    private fun opensApps(intent: Intent?): Boolean =
        intent?.getBooleanExtra("open_apps", false) == true ||
            (intent?.data?.scheme == "beep" && intent.data?.host == "apps")

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra("conversation_id")?.let { (application as BeepApplication).repository.select(it) }
        if (intent.getBooleanExtra("focus_input", false)) pendingFocusInput.value = true
        if (opensApps(intent)) pendingOpenApps.value = true
    }
    override fun onResume() {
        super.onResume()
        val repository = (application as BeepApplication).repository
        // From the bubble, the app the user was just in; from Beep itself, none.
        repository.foregroundApp = if (bubble) ScreenContext.foreground(this) else null
        if (pendingFloat) {
            pendingFloat = false
            float()
        }
        if (repository.settings.value.configured && !repository.connected.value) repository.reconnect()
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboardListener?.let { cm.removePrimaryClipChangedListener(it) }
        val listener = ClipboardManager.OnPrimaryClipChangedListener {
            if (suppressNextClipboard) { suppressNextClipboard = false; return@OnPrimaryClipChangedListener }
            val clip = cm.primaryClip?.getItemAt(0)?.text?.toString()
            if (!clip.isNullOrBlank() && clip.length in 11..5000) {
                clipboardContent.value = clip
            }
        }
        clipboardListener = listener
        cm.addPrimaryClipChangedListener(listener)
    }
    override fun onPause() {
        super.onPause()
        clipboardListener?.let {
            (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).removePrimaryClipChangedListener(it)
        }
        clipboardListener = null
    }
    override fun onDestroy() {
        super.onDestroy()
        networkCallback?.let { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT))
        val repository = (application as BeepApplication).repository
        // Network change listener for auto-reconnect
        val cm = getSystemService(ConnectivityManager::class.java)
        networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                if (repository.settings.value.configured && !repository.connected.value) repository.reconnect()
            }
        }
        cm.registerNetworkCallback(NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build(), networkCallback!!)
        // Start location updates if permission was already granted (from onboarding)
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED) {
            startLocationUpdates()
        }
        intent?.getStringExtra("conversation_id")?.let { repository.select(it) }
        if (intent?.getBooleanExtra("focus_input", false) == true) pendingFocusInput.value = true
        if (opensApps(intent)) pendingOpenApps.value = true
        setContent {
            BeepTheme {
                BeepApp(repository, bubble, locationPermission = locationPermission, focusInput = pendingFocusInput, openApps = pendingOpenApps, clipboardContent = clipboardContent, onFloat = {
                    if (Bubble.kind(this) == BubbleKind.System || Bubble.allowed(this) || Bubble.asked(this)) float()
                    else { Bubble.markAsked(this); pendingFloat = true; Bubble.settings(this) }
                }, onExpand = {
                    startActivity(Intent(this, MainActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                    })
                    if (panel) finish()
                }, onBubbleSettings = { Bubble.settings(this) })
            }
        }
    }
    override fun onStart() {
        super.onStart()
        if (!bubble) Bubble.appShown(true)
        (application as BeepApplication).repository.chatScreenShown(true)
    }
    override fun onStop() {
        super.onStop()
        if (!bubble) Bubble.appShown(false)
        (application as BeepApplication).repository.chatScreenShown(false)
    }
    /** Float Beep: the overlay bubble when picked and allowed, otherwise the system bubble. */
    private fun float() {
        if (Bubble.kind(this) == BubbleKind.Overlay && Bubble.show(this)) {
            SystemBubble.hide(this)
            if (!bubble) moveTaskToBack(true)
        } else if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else if (SystemBubble.show(this)) {
            if (!bubble) moveTaskToBack(true)
        } else SystemBubble.settings(this)
    }
}
/** The compact chat in a system bubble, used when Beep may not draw over other apps. */
class BubbleActivity : MainActivity() {
    override val bubble = true
}
/** The compact chat that opens from the bubble, as a rounded panel under it. */
class PanelActivity : MainActivity() {
    override val bubble = true
    override val panel = true
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val density = resources.displayMetrics.density
        val metrics = windowManager.currentWindowMetrics
        val width = metrics.bounds.width() - 2 * (12 * density).toInt()
        val fullHeight = (metrics.bounds.height() * 0.62f).toInt()
        val gap = (8 * density).toInt()
        val shortest = (200 * density).toInt()
        window.setLayout(width, fullHeight)
        window.attributes = window.attributes.apply { gravity = Gravity.TOP; y = Bubble.panelTop(this@PanelActivity) }
        // A floating window is not resized for the keyboard, so shrink it to end just above the keyboard.
        // The inset is how far the keyboard overlaps the window, which is zero once it has shrunk.
        window.decorView.setOnApplyWindowInsetsListener { view, insets ->
            val height = window.attributes.height
            val next = when {
                !insets.isVisible(WindowInsets.Type.ime()) -> fullHeight
                else -> (height - insets.getInsets(WindowInsets.Type.ime()).bottom.let { if (it > 0) it + gap else 0 }).coerceAtLeast(shortest)
            }
            if (next != height) window.setLayout(width, next)
            view.onApplyWindowInsets(insets)
        }
        window.decorView.outlineProvider = object : ViewOutlineProvider() {
            override fun getOutline(view: View, outline: Outline) {
                outline.setRoundRect(0, 0, view.width, view.height, 24 * resources.displayMetrics.density)
            }
        }
        window.decorView.clipToOutline = true
    }
    override fun onStart() { super.onStart(); Bubble.panelShown(this) }
    override fun onStop() { super.onStop(); Bubble.panelHidden(this) }
}

private enum class Screen { Chat, Memories, Crons, Integrations, CustomIntegrations, Settings }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BeepApp(repository: ChatRepository, bubble: Boolean,
                    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>? = null,
                    focusInput: MutableState<Boolean> = mutableStateOf(false),
                    openApps: MutableState<Boolean> = mutableStateOf(false),
                    clipboardContent: MutableState<String?> = mutableStateOf(null),
                    onFloat: () -> Unit, onExpand: () -> Unit, onBubbleSettings: () -> Unit) {
    val state by repository.state.collectAsStateWithLifecycle()
    val config by repository.settings.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    val pendingAppIntent by repository.pendingIntent.collectAsStateWithLifecycle()
    val pendingAlarm by repository.pendingAlarm.collectAsStateWithLifecycle()
    val browserOpen by repository.browserOpen.collectAsStateWithLifecycle()
    var settingsOpen by rememberSaveable { mutableStateOf(false) }
    var drawerOpen by remember { mutableStateOf(false) }
    var bubbleMenu by remember { mutableStateOf(false) }
    var historyQuery by rememberSaveable { mutableStateOf("") }
    var screen by rememberSaveable { mutableStateOf(Screen.Chat) }
    // A notice about an app that lost its login opens Apps to reconnect it.
    LaunchedEffect(openApps.value) {
        if (openApps.value) {
            openApps.value = false
            screen = Screen.Integrations
        }
    }
    // Chats left by starting a new one, newest last, so a swipe right can go back to them.
    // The trail only holds while the user stays on the chat the swipes led to: opening one from the
    // list, a notification or a share starts over, and it lives no longer than this screen.
    var visited by remember { mutableStateOf(listOf<String>()) }
    var trailEnd by remember { mutableStateOf<String?>(null) }
    fun openChat(id: String) {
        visited = emptyList()
        repository.select(id)
    }
    fun startNewChat() {
        if (state.current.messages.isEmpty()) return
        visited = visited.filter { it != state.current.id } + state.current.id
        repository.newChat()
        trailEnd = repository.state.value.current.id
    }
    fun previousChat(): String? {
        if (trailEnd != state.current.id) return null
        val ids = state.conversations.map { it.id }.toSet()
        return visited.lastOrNull { it in ids && it != state.current.id }
    }
    fun goBack() {
        val id = previousChat() ?: run { drawerOpen = true; return }
        visited = visited.dropLastWhile { it != id }.dropLast(1)
        repository.select(id)
        trailEnd = id
    }
    val context = androidx.compose.ui.platform.LocalContext.current
    // Handle pending app launch intents
    LaunchedEffect(pendingAppIntent) {
        pendingAppIntent?.let { intent ->
            AppLauncher.launch(context, intent.app, intent.uri, intent.fallbackUrl)
            repository.pendingIntent.value = null
        }
    }
    LaunchedEffect(pendingAlarm) {
        pendingAlarm?.let { alarm ->
            AppLauncher.setAlarm(context, alarm)
            repository.pendingAlarm.value = null
        }
    }
    if (settingsOpen || !config.configured) {
        ConnectionScreen(config, canGoBack = config.configured, onBack = { settingsOpen = false }, onConnect = {
            withContext(Dispatchers.IO) { repository.connect(it) }; settingsOpen = false
        })
        return
    }
    // Show onboarding permissions screen once after first connection
    val prefs = remember { context.getSharedPreferences("connection", android.content.Context.MODE_PRIVATE) }
    var onboardingDone by rememberSaveable { mutableStateOf(prefs.getBoolean("onboarding_done", false)) }
    if (!onboardingDone && !bubble) {
        OnboardingScreen(locationPermission = locationPermission, onDone = {
            prefs.edit().putBoolean("onboarding_done", true).apply()
            onboardingDone = true
        })
        return
    }
    val activeScreen = if (bubble) Screen.Chat else screen
    BackHandler(enabled = activeScreen != Screen.Chat) {
        screen = if (screen == Screen.CustomIntegrations) Screen.Integrations else Screen.Chat
    }
    Crossfade(targetState = activeScreen, animationSpec = tween(300), label = "screen-transition") { currentScreen ->
        when (currentScreen) {
            Screen.Memories -> MemoriesScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Crons -> CronsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Integrations -> AppsScreen(repository, onBack = { screen = Screen.Chat }, onCustom = { screen = Screen.CustomIntegrations })
            Screen.CustomIntegrations -> IntegrationsScreen(repository, onBack = { screen = Screen.Integrations })
            Screen.Settings -> SettingsScreen(status, onBack = { screen = Screen.Chat },
                onBubbleSettings = onBubbleSettings, onConnection = { settingsOpen = true })
            Screen.Chat -> {
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(drawerOpen) {
        if (drawerOpen) {
            // The composer keeps focus behind the drawer otherwise, and the keyboard covers the list.
            focusManager.clearFocus()
            keyboard?.hide()
            repository.refreshHistory(); drawerState.open()
        } else drawerState.close()
    }
    LaunchedEffect(drawerState.currentValue) { drawerOpen = drawerState.currentValue == DrawerValue.Open }
    BackHandler(enabled = drawerOpen) { drawerOpen = false }
    // Closed, horizontal swipes belong to the chat (new chat or back), so the drawer only takes them to close.
    ModalNavigationDrawer(drawerState = drawerState, gesturesEnabled = drawerOpen, drawerContent = {
        ModalDrawerSheet(modifier = Modifier.widthIn(max = 320.dp).fillMaxWidth(0.9f), drawerContainerColor = Ink,
            drawerShape = RoundedCornerShape(topEnd = Beep.shapes.radius, bottomEnd = Beep.shapes.radius)) {
            Spacer(Modifier.height(if (bubble) 8.dp else 20.dp))
            Text(displayText("Your chats"), Modifier.padding(horizontal = 20.dp, vertical = 12.dp), style = displayStyle(26.sp))
            OutlinedTextField(historyQuery, { historyQuery = it }, placeholder = { Text("Find a conversation", fontSize = 13.sp) },
                leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss),
                shape = RoundedCornerShape(Beep.shapes.radius), modifier = Modifier.padding(horizontal = 20.dp, vertical = 14.dp).fillMaxWidth())
            var contextMenuChat by remember { mutableStateOf<String?>(null) }
            var showRenameDialog by remember { mutableStateOf<Conversation?>(null) }
            var showDeleteDialog by remember { mutableStateOf<Conversation?>(null) }
            // Chats picked for bulk delete; selection mode is on while it is not null.
            var picked by remember { mutableStateOf<Set<String>?>(null) }
            var confirmBulkDelete by remember { mutableStateOf(false) }
            LaunchedEffect(drawerOpen) { if (!drawerOpen) picked = null }
            BackHandler(enabled = picked != null) { picked = null }
            val drawerScope = rememberCoroutineScope()
            val visibleChats = state.conversations.filter { !it.isUnused && it.title.contains(historyQuery, ignoreCase = true) }
            picked?.let { ids ->
                Row(Modifier.padding(start = 20.dp, end = 8.dp, bottom = 4.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text("${ids.size} selected", Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                    val all = visibleChats.map { it.id }.toSet()
                    TextButton(onClick = { picked = if (ids.containsAll(all)) emptySet() else ids + all }) {
                        Text(if (ids.containsAll(all) && all.isNotEmpty()) "None" else "All")
                    }
                    IconButton(onClick = { confirmBulkDelete = true }, enabled = ids.isNotEmpty()) {
                        Icon(Icons.Outlined.Delete, "Delete selected", tint = if (ids.isNotEmpty()) Coral else TextMuted)
                    }
                    IconButton(onClick = { picked = null }) { Icon(Icons.Outlined.Close, "Done selecting") }
                }
            }
            LazyColumn(Modifier.weight(1f)) {
                items(visibleChats, key = { it.id }) { chat ->
                    Box(Modifier.padding(horizontal = 12.dp, vertical = 2.dp)) {
                        val selecting = picked
                        val toggle = { selecting?.let { picked = if (chat.id in it) it - chat.id else it + chat.id } }
                        val highlighted = if (selecting != null) chat.id in selecting else state.current.id == chat.id
                        // NavigationDrawerItem consumes the press itself, so it never reports a long press.
                        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius))
                            .background(if (highlighted) Beep.colors.user else Color.Transparent)
                            .combinedClickable(
                                onClick = { if (selecting != null) toggle() else { openChat(chat.id); drawerOpen = false } },
                                onLongClick = { if (selecting != null) toggle() else contextMenuChat = chat.id })
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            when {
                                selecting != null -> Icon(if (chat.id in selecting) Icons.Outlined.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                                    null, Modifier.size(18.dp), tint = if (chat.id in selecting) Coral else TextMuted)
                                else -> Icon(if (chat.pinned) Icons.Outlined.PushPin else Icons.Outlined.ChatBubbleOutline, null, Modifier.size(18.dp), tint = if (chat.pinned) Moss else TextMuted)
                            }
                            Text(chat.title, maxLines = 2, fontSize = 14.sp, overflow = TextOverflow.Ellipsis,
                                color = if (highlighted) Beep.colors.onUser else Color.Unspecified)
                        }
                        DropdownMenu(expanded = contextMenuChat == chat.id, onDismissRequest = { contextMenuChat = null },
                            containerColor = Surface) {
                            DropdownMenuItem(text = { Text("Select") },
                                leadingIcon = { Icon(Icons.Outlined.CheckCircle, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuChat = null; picked = setOf(chat.id) })
                            DropdownMenuItem(text = { Text("Rename") },
                                leadingIcon = { Icon(Icons.Outlined.Edit, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuChat = null; showRenameDialog = chat })
                            DropdownMenuItem(text = { Text(if (chat.pinned) "Unpin" else "Pin") },
                                leadingIcon = { Icon(Icons.Outlined.PushPin, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuChat = null; drawerScope.launch { runCatching { repository.pinSession(chat.id, !chat.pinned) } } })
                            DropdownMenuItem(text = { Text("Delete", color = Coral) },
                                leadingIcon = { Icon(Icons.Outlined.Delete, null, Modifier.size(18.dp), tint = Coral) },
                                onClick = { contextMenuChat = null; showDeleteDialog = chat })
                        }
                    }
                }
            }
            // Rename dialog
            showRenameDialog?.let { chat ->
                var renameText by remember(chat.id) { mutableStateOf(chat.title) }
                AlertDialog(
                    onDismissRequest = { showRenameDialog = null },
                    confirmButton = { TextButton(onClick = {
                        val title = renameText.trim()
                        if (title.isNotEmpty()) { drawerScope.launch { runCatching { repository.renameSession(chat.id, title) } }; showRenameDialog = null }
                    }) { Text("Rename") } },
                    dismissButton = { TextButton(onClick = { showRenameDialog = null }) { Text("Cancel") } },
                    title = { Text("Rename conversation") },
                    text = { OutlinedTextField(renameText, { renameText = it }, singleLine = true, shape = RoundedCornerShape(12.dp),
                        modifier = Modifier.fillMaxWidth()) },
                    containerColor = Surface)
            }
            // Delete confirmation dialog
            showDeleteDialog?.let { chat ->
                AlertDialog(
                    onDismissRequest = { showDeleteDialog = null },
                    confirmButton = { TextButton(onClick = {
                        drawerScope.launch { runCatching { repository.deleteSession(chat.id) } }; showDeleteDialog = null
                    }) { Text("Delete", color = Coral) } },
                    dismissButton = { TextButton(onClick = { showDeleteDialog = null }) { Text("Cancel") } },
                    title = { Text("Delete conversation?") },
                    text = { Text("This will permanently delete this conversation and all its messages.") },
                    containerColor = Surface)
            }
            if (confirmBulkDelete) {
                val ids = picked.orEmpty()
                AlertDialog(
                    onDismissRequest = { confirmBulkDelete = false },
                    confirmButton = { TextButton(onClick = {
                        confirmBulkDelete = false; picked = null
                        drawerScope.launch { runCatching { repository.deleteSessions(ids) } }
                    }) { Text("Delete", color = Coral) } },
                    dismissButton = { TextButton(onClick = { confirmBulkDelete = false }) { Text("Cancel") } },
                    title = { Text(if (ids.size == 1) "Delete 1 conversation?" else "Delete ${ids.size} conversations?") },
                    text = { Text("This will permanently delete them and all their messages.") },
                    containerColor = Surface)
            }
            if (!bubble) {
                HorizontalDivider(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), color = Border)
                Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    DrawerTile("Memories", Icons.Outlined.Psychology, Beep.colors.tints[0], Modifier.weight(1f)) { screen = Screen.Memories; drawerOpen = false }
                    DrawerTile("Routines", Icons.Outlined.Schedule, Beep.colors.tints[1], Modifier.weight(1f)) { screen = Screen.Crons; drawerOpen = false }
                    DrawerTile("Apps", Icons.Outlined.Extension, Beep.colors.tints[2], Modifier.weight(1f)) { screen = Screen.Integrations; drawerOpen = false }
                }
                Spacer(Modifier.height(8.dp))
            }
            Row(Modifier.padding(horizontal = 10.dp).padding(bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = { if (bubble) settingsOpen = true else screen = Screen.Settings; drawerOpen = false }) {
                    Icon(Icons.Outlined.Settings, null, Modifier.size(18.dp), tint = TextMuted); Spacer(Modifier.width(8.dp))
                    Text(if (bubble) "Connection" else "Settings", color = TextMuted)
                }
                Spacer(Modifier.weight(1f))
                ConnectionDot(connected, status); Spacer(Modifier.width(6.dp))
                Text(status, fontSize = 11.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(end = 12.dp).widthIn(max = 140.dp))
            }
        }
    }) {
        Scaffold(containerColor = Ink, topBar = {
            if (bubble) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(displayText("Beep"), style = displayStyle(18.sp))
                    Spacer(Modifier.width(8.dp))
                    ConnectionDot(connected, status)
                    Spacer(Modifier.weight(1f))
                    IconButton(onClick = onExpand) { Icon(Icons.Outlined.OpenInFull, "Open full app", Modifier.size(18.dp)) }
                    Box {
                        IconButton(onClick = { bubbleMenu = true }) { Icon(Icons.Outlined.MoreVert, "Chat options", Modifier.size(20.dp)) }
                        DropdownMenu(expanded = bubbleMenu, onDismissRequest = { bubbleMenu = false }) {
                            DropdownMenuItem(text = { Text("New conversation") }, onClick = { startNewChat(); bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Conversations") }, onClick = { drawerOpen = true; bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Connection settings") }, onClick = { settingsOpen = true; bubbleMenu = false })
                        }
                    }
                }
            } else TopAppBar(title = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column {
                        Text(displayText("Beep"), style = displayStyle(24.sp))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ConnectionDot(connected, status)
                            if (!connected) {
                                Spacer(Modifier.width(6.dp))
                                Text("Tap to reconnect", fontSize = 12.sp, color = TextMuted,
                                    fontFamily = Beep.fonts.body, fontWeight = FontWeight.Medium,
                                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.clickable { repository.reconnect() })
                            }
                        }
                    }
                }
            }, navigationIcon = {
                ChunkyButton(onClick = { drawerOpen = true }, shape = RoundedCornerShape(Beep.shapes.smallRadius),
                    modifier = Modifier.padding(start = 12.dp, end = 10.dp).size(42.dp), contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.Outlined.Menu, "Conversations", Modifier.size(20.dp))
                }
            },
                actions = {
                    IconButton(onClick = onFloat, Modifier.padding(end = 6.dp)) { Icon(Icons.Outlined.BubbleChart, "Float chat", tint = TextMuted) }
                }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
        }) { padding ->
            SwipeChats(canStartNew = state.current.messages.isNotEmpty(), hasPrevious = previousChat() != null,
                onNew = { startNewChat() }, onBack = { goBack() }, modifier = Modifier.padding(padding).fillMaxSize()) {
                // Each chat gets its own list, so rows of the chat being left never animate under the next one.
                key(state.current.id) {
                    ChatScreen(state.current, repository, Modifier.fillMaxSize(), compact = bubble, focusInput = focusInput, clipboardContent = clipboardContent)
                }
            }
        }
    } // end ModalNavigationDrawer
    } // end Screen.Chat
    } // end when
    } // end Crossfade
    // Browser overlay
    AnimatedVisibility(visible = browserOpen,
        enter = fadeIn() + slideInVertically { it },
        exit = fadeOut() + slideOutVertically { it }) {
        BrowserScreen(onClose = { repository.closeBrowser() }) { modifier ->
            ChatScreen(state.current, repository, modifier, compact = true)
        }
    }
}

@Composable
private fun ChatScreen(chat: Conversation, repository: ChatRepository, modifier: Modifier, compact: Boolean,
                       focusInput: MutableState<Boolean> = mutableStateOf(false),
                       clipboardContent: MutableState<String?> = mutableStateOf(null)) {
    var draft by rememberSaveable(chat.id) { mutableStateOf("") }
    var attachedImage by remember(chat.id) { mutableStateOf<String?>(null) }
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val imageContext = androidx.compose.ui.platform.LocalContext.current
    val photoPicker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            scope.launch(Dispatchers.IO) {
                val encoded = encodeImageUri(imageContext, uri)
                if (encoded != null) attachedImage = encoded
            }
        }
    }
    val inputFocusRequester = remember { FocusRequester() }
    LaunchedEffect(focusInput.value) {
        if (focusInput.value) {
            focusInput.value = false
            inputFocusRequester.requestFocus()
        }
    }
    // Follow the reply until the user drags away from the bottom; reaching the bottom again resumes it.
    // Only a drag counts: our own scrolling while a reply streams in must not stop following.
    var followLatest by remember(chat.id) { mutableStateOf(true) }
    val dragged by list.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(list, chat.id) {
        snapshotFlow { dragged to list.canScrollForward }.collect { (dragging, canForward) ->
            if (!canForward) followLatest = true else if (dragging) followLatest = false
        }
    }
    LaunchedEffect(chat.id) { if (chat.messages.isNotEmpty()) list.scrollToEnd(animated = false) }
    LaunchedEffect(chat.messages.size, chat.messages.lastOrNull()?.text?.length, chat.status) {
        if (chat.messages.isNotEmpty() && followLatest) list.scrollToEnd(animated = true)
    }
    // The keyboard and a growing input resize the list every frame, so stay at the bottom without animating.
    LaunchedEffect(list, chat.id) {
        snapshotFlow { list.layoutInfo.viewportSize }.drop(1).collect {
            if (followLatest && chat.messages.isNotEmpty()) list.scrollToEnd(animated = false)
        }
    }
    // Auto-dismiss clipboard banner after 10 seconds
    val clipText = clipboardContent.value
    LaunchedEffect(clipText) {
        if (clipText != null) { delay(10_000); clipboardContent.value = null }
    }
    // Dismiss clipboard banner when user starts typing
    LaunchedEffect(draft) { if (draft.isNotBlank()) clipboardContent.value = null }
    fun submit() {
        val hasContent = draft.isNotBlank() || attachedImage != null
        if (hasContent && !chat.busy) {
            followLatest = true
            val text = draft.ifBlank { "What is this?" }
            repository.send(text, attachedImage)
            draft = ""
            attachedImage = null
        }
    }
    // While nothing of the reply is written yet, its progress sits in the conversation where the reply will appear.
    val workingInline = chat.busy && chat.status.isNotEmpty() &&
        chat.messages.lastOrNull().let { it == null || it.role != "assistant" || it.text.isBlank() }
    Column(modifier.imePadding()) {
        // Clipboard suggestion banner
        ClipboardBanner(clipboardContent, onAction = { action, text ->
            clipboardContent.value = null
            val message = when (action) {
                "ask" -> text
                "translate" -> "Translate this:\n\n$text"
                "summarize" -> "Summarize this:\n\n$text"
                else -> text
            }
            followLatest = true
            repository.send(message)
        }, onDismiss = { clipboardContent.value = null })
        if (chat.messages.isEmpty()) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(if (compact) 20.dp else 24.dp), verticalArrangement = Arrangement.Center) {
                if (compact) {
                    Text(displayText("Hey. What can I help with?"), style = displayStyle(20.sp))
                    Spacer(Modifier.height(18.dp))
                } else {
                    Text(buildAnnotatedString {
                        append(displayText("A little help.\n"))
                        withStyle(SpanStyle(color = Beep.colors.ok)) { append(displayText("Right here.")) }
                    }, style = displayStyle(40.sp, lineHeight = 44.sp), modifier = Modifier.popIn("hello"))
                    Spacer(Modifier.height(12.dp))
                    Text("Find an answer. Untangle a thought.\nKeep something worth remembering.", color = TextMuted, lineHeight = 23.sp)
                    Spacer(Modifier.height(28.dp))
                }
                // Personal starters from the server once they arrive; until then, templates to finish typing.
                val starters by repository.starters.collectAsState()
                LaunchedEffect(chat.id) { repository.loadStarters() }
                val items = if (starters.isNotEmpty()) starters.map { Triple(it.title, it.prompt, true) }
                    else listOf(
                        Triple("Remember something", "Remember that I prefer short, practical answers.", true),
                        Triple("Search the web", "Search the web for ", false),
                        Triple("Set a reminder", "Remind me to ", false),
                    )
                items.forEachIndexed { index, (title, prompt, complete) ->
                    var shown by remember { mutableStateOf(false) }
                    LaunchedEffect(Unit) { delay(120L + index * 90L); shown = true }
                    Box(Modifier.popIn(title to shown, play = shown).alpha(if (shown) 1f else 0f)) {
                        Suggestion(title, prompt, starterIcon(title + " " + prompt), Beep.colors.tints[index % Beep.colors.tints.size]) {
                            if (complete) {
                                followLatest = true; repository.send(it)
                                // A used starter is replaced in the background, ready for the next empty chat.
                                if (starters.isNotEmpty()) repository.loadStarters(refresh = true)
                            } else draft = it
                        }
                    }
                    if (index < items.lastIndex) Spacer(Modifier.height(12.dp))
                }
            }
        } else {
            // Only messages that arrive while the chat is open pop in; history is drawn at rest.
            val settled = remember(chat.id) { chat.messages.map { it.id }.toHashSet() }
            val radius = Beep.shapes.radius
            // The "Latest reply" button floats over the list, so showing it does not resize the list.
            Box(Modifier.weight(1f).fillMaxWidth()) {
            LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(if (compact) 12.dp else 18.dp), verticalArrangement = Arrangement.spacedBy(if (compact) 12.dp else 18.dp)) {
                itemsIndexed(chat.messages, key = { _, it -> it.id }) { index, message ->
                    val isUser = message.role == "user"
                    val fresh = message.id !in settled
                    SideEffect { settled.add(message.id) }
                    Column(Modifier.fillMaxWidth().animateItem(fadeOutSpec = null)
                        .popIn(message.id, play = fresh, origin = TransformOrigin(if (isUser) 1f else 0f, 1f)),
                        horizontalAlignment = if (isUser) Alignment.End else Alignment.Start) {
                        val isLastAssistant = !isUser && chat.messages.lastOrNull { it.role == "assistant" }?.id == message.id
                        // Consecutive replies from Beep share one avatar and name.
                        val followsBeep = chat.messages.getOrNull(index - 1)?.role == "assistant"
                        if (!isUser && !compact && !followsBeep) {
                            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(bottom = 8.dp)) {
                                BeepAvatar(isThinking = isLastAssistant && chat.busy)
                                Spacer(Modifier.width(8.dp))
                                Text(displayText("Beep"), style = displayStyle(14.sp), color = TextPrimary)
                            }
                        }
                        val bubbleShape = if (isUser) RoundedCornerShape(radius, radius, 6.dp, radius)
                            else RoundedCornerShape(6.dp, radius, radius, radius)
                        val bubbleModifier = when {
                            isUser -> Modifier.padding(start = 48.dp)
                                .then(if (Beep.shapes.card == CardLook.Hairline) Modifier.clip(bubbleShape).background(Beep.colors.user)
                                    else Modifier.beepFrame(bubbleShape, Beep.colors.user))
                            Beep.shapes.botBubble -> Modifier.padding(end = 24.dp).beepFrame(bubbleShape, Beep.colors.surface)
                            else -> Modifier.fillMaxWidth()
                        }
                        // A finished tool that has something to show becomes a card; the rest stay chips.
                        val toolCards = remember(message.toolCalls) { message.toolCalls.map { it to cardFor(it) } }
                        val chips = toolCards.filter { (call, card) -> card == null && call.name != "suggest_replies" }
                        if (chips.isNotEmpty()) {
                            Column(Modifier.padding(bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                chips.forEachIndexed { i, (tc, _) -> ToolChip(tc, Beep.colors.tints[i % Beep.colors.tints.size]) { repository.openBrowser(it) } }
                            }
                        }
                        val isStreaming = isLastAssistant && chat.busy
                        val textLen = message.text.length
                        val displayedLen by animateIntAsState(textLen, tween(if (isStreaming) 150 else 0), label = "stream-text")
                        val shownText = if (isStreaming && displayedLen < textLen) message.text.substring(0, displayedLen) else message.text
                        var menuOpen by remember(message.id) { mutableStateOf(false) }
                        val quote: () -> Unit = {
                            draft = message.text.trim().lines().take(4).joinToString("\n") { "> $it" } + "\n\n"
                            inputFocusRequester.requestFocus()
                        }
                        if (shownText.isNotBlank() || message.imageBase64 != null) Box {
                        // Long press belongs to text selection, so a tap opens the menu (copy, quote, share).
                        Column(bubbleModifier.clickable(enabled = !isStreaming, onClickLabel = "Message actions",
                            indication = null, interactionSource = remember { MutableInteractionSource() }) { menuOpen = true }) {
                            // Show attached image thumbnail in user messages
                            if (message.imageBase64 != null) {
                                val msgBitmap = remember(message.imageBase64) { decodeImageBase64(message.imageBase64) }
                                if (msgBitmap != null) {
                                    Image(msgBitmap.asImageBitmap(), "Attached image",
                                        contentScale = ContentScale.FillWidth,
                                        modifier = Modifier.fillMaxWidth().heightIn(max = 200.dp))
                                }
                            }
                            if (shownText.isNotBlank()) MessageContent(shownText, message.role, compact,
                                padded = isUser || Beep.shapes.botBubble)
                        }
                        MessageMenu(menuOpen, onDismiss = { menuOpen = false }, text = message.text, onQuote = quote)
                        }
                        if (!isStreaming) ToolCards(toolCards.mapNotNull { it.second }, compact) { repository.openBrowser(it) }
                        if (!isUser && !isStreaming && message.text.isNotBlank()) {
                            val shownLinks = toolCards.mapNotNull { (it.second as? ToolCard.Link)?.url }.toSet()
                            val links = remember(message.text) { linksIn(message.text) }.filter { it !in shownLinks }
                            if (links.isNotEmpty()) Column(Modifier.padding(top = 8.dp, end = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                links.forEach { LinkPreview(it) { url -> repository.openBrowser(url) } }
                            }
                        }
                        if (message.cards.isNotEmpty()) {
                            Spacer(Modifier.height(8.dp))
                            Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth().animateContentSize()) {
                                message.cards.forEach { card -> CardView(card, repository, compact) }
                            }
                        }
                        if (!message.taskSteps.isNullOrEmpty()) {
                            Spacer(Modifier.height(8.dp))
                            TaskProgressCard(message.taskSteps)
                        }
                        if (message.role == "assistant" && message.text.isNotBlank() && !chat.busy && !compact) Row(verticalAlignment = Alignment.CenterVertically) {
                            CopyReply(message.text, compact)
                            TextButton(onClick = quote, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
                                Icon(Icons.Outlined.FormatQuote, null, Modifier.size(14.dp), tint = TextMuted)
                                Spacer(Modifier.width(6.dp)); Text("Quote", fontSize = 11.sp, color = TextMuted)
                            }
                            // Jev's estimate that this turn held something worth remembering.
                            message.memoryScore?.let { score ->
                                val saved = message.toolCalls.any { it.name in MEMORY_TOOLS }
                                val note = if (score >= MEMORY_THRESHOLD && !saved) " · nothing new" else ""
                                Text("Memory ${(score * 100).roundToInt()}%$note", fontSize = 11.sp, color = TextDim,
                                    modifier = Modifier.padding(start = 8.dp))
                            }
                        }
                        if (isLastAssistant && !chat.busy && chat.messages.last().id == message.id) {
                            toolCards.firstNotNullOfOrNull { it.second as? ToolCard.Replies }?.let { replies ->
                                Spacer(Modifier.height(4.dp))
                                SuggestedReplies(replies) { followLatest = true; repository.send(it) }
                            }
                        }
                    }
                }
                if (workingInline) item(key = "working") {
                    Column(Modifier.fillMaxWidth().animateItem(fadeOutSpec = null).popIn("working-${chat.id}", origin = TransformOrigin(0f, 1f))) {
                        if (chat.messages.lastOrNull()?.role != "assistant" && !compact) {
                            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(bottom = 10.dp)) {
                                BeepAvatar(isThinking = true)
                                Spacer(Modifier.width(8.dp))
                                Text(displayText("Beep"), style = displayStyle(14.sp), color = TextPrimary)
                            }
                        }
                        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 4.dp)
                            .animateContentSize(spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium))) {
                            PulsingDots()
                            Spacer(Modifier.width(10.dp))
                            StatusText(chat.status)
                        }
                        ThinkingPreview(chat.thinking, Modifier.padding(end = if (compact) 0.dp else 48.dp))
                        if (!compact) ReplySkeleton(Modifier.padding(top = 12.dp, end = 48.dp))
                    }
                }
            }
            androidx.compose.animation.AnimatedVisibility(visible = !followLatest && list.canScrollForward,
                enter = fadeIn() + slideInVertically { it / 2 }, exit = fadeOut() + slideOutVertically { it / 2 },
                modifier = Modifier.align(Alignment.BottomCenter)) {
                ChunkyButton(onClick = { followLatest = true; scope.launch { list.scrollToEnd(animated = true) } },
                    modifier = Modifier.padding(vertical = 8.dp), contentPadding = PaddingValues(horizontal = 14.dp, vertical = 7.dp)) {
                    Icon(Icons.Outlined.ArrowDownward, null, Modifier.size(16.dp)); Text("Latest reply", fontSize = 12.sp, fontWeight = FontWeight.Medium)
                }
            }
            }
        }
        // The streaming text is its own progress, so "Replying" needs no line of its own.
        AnimatedVisibility(visible = chat.status.isNotEmpty() && !workingInline && !chat.status.startsWith("Replying"),
            enter = fadeIn(tween(200)) + expandVertically(tween(200)), exit = fadeOut(tween(150)) + shrinkVertically(tween(150))) {
            Column(Modifier.padding(horizontal = 20.dp, vertical = 6.dp)) {
                Row(Modifier.animateContentSize(spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium)),
                    verticalAlignment = Alignment.CenterVertically) {
                    PulsingDots()
                    Spacer(Modifier.width(10.dp))
                    StatusText(chat.status)
                }
                ThinkingPreview(chat.thinking)
            }
        }
        AnimatedVisibility(visible = chat.error.isNotEmpty(),
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 4.dp).fillMaxWidth()
                .clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(Beep.colors.error.copy(alpha = 0.10f))
                .padding(start = 14.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(chat.error, fontSize = 13.sp, color = Beep.colors.error, modifier = Modifier.weight(1f).padding(vertical = 8.dp), maxLines = 4)
                if (chat.busy) TextButton(onClick = repository::retry) { Text("Retry", color = Beep.colors.error, fontWeight = FontWeight.SemiBold) }
            }
        }
        // Voice input state
        val voiceContext = androidx.compose.ui.platform.LocalContext.current
        var listening by remember { mutableStateOf(false) }
        val speechAvailable = remember { SpeechRecognizer.isRecognitionAvailable(voiceContext) }
        val recognizer = remember { if (speechAvailable) SpeechRecognizer.createSpeechRecognizer(voiceContext) else null }
        val micPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted && recognizer != null) {
                listening = true
                startListening(recognizer, onPartial = { partial -> draft = partial },
                    onResult = { result -> draft = result; listening = false },
                    onError = { listening = false })
            }
        }
        DisposableEffect(recognizer) { onDispose { recognizer?.destroy() } }

        fun startVoice() {
            if (!speechAvailable) return
            if (listening) { recognizer?.stopListening(); listening = false; return }
            if (ContextCompat.checkSelfPermission(voiceContext, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                listening = true
                startListening(recognizer!!, onPartial = { partial -> draft = partial },
                    onResult = { result -> draft = result; listening = false },
                    onError = { listening = false })
            } else {
                micPermission.launch(Manifest.permission.RECORD_AUDIO)
            }
        }

        // "Listening..." indicator
        AnimatedVisibility(visible = listening,
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = if (compact) 12.dp else 20.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                val pulseAlpha by rememberInfiniteTransition(label = "listen-pulse").animateFloat(
                    initialValue = 0.4f, targetValue = 1f,
                    animationSpec = infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "listen-alpha")
                Box(Modifier.size(7.dp).background(Beep.colors.accent.copy(alpha = pulseAlpha), CircleShape))
                Spacer(Modifier.width(8.dp))
                Text("Listening...", fontSize = 12.sp, color = TextMuted, fontWeight = FontWeight.Medium)
            }
        }

        // Image attachment preview
        AnimatedVisibility(visible = attachedImage != null,
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            attachedImage?.let { img ->
                val previewBitmap = remember(img) { decodeImageBase64(img) }
                if (previewBitmap != null) {
                    Box(Modifier.padding(horizontal = if (compact) 12.dp else 18.dp, vertical = 8.dp)) {
                        Box(Modifier.size(72.dp).popIn(img).beepFrame(RoundedCornerShape(Beep.shapes.smallRadius), Surface, pressableLook())) {
                            Image(previewBitmap.asImageBitmap(), "Attached image",
                                contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                        }
                        ChunkyButton(onClick = { attachedImage = null }, color = Beep.colors.ink, contentColor = Beep.colors.page,
                            modifier = Modifier.size(24.dp).align(Alignment.TopEnd).offset(x = 6.dp, y = (-6).dp),
                            contentPadding = PaddingValues(0.dp)) {
                            Icon(Icons.Outlined.Close, "Remove image", Modifier.size(14.dp))
                        }
                    }
                }
            }
        }

        val buttonSize = if (compact) 44.dp else 50.dp
        val buttonShape = RoundedCornerShape(Beep.shapes.radius)
        Row(Modifier.padding(start = if (compact) 8.dp else 14.dp, end = if (compact) 10.dp else 16.dp,
            top = if (compact) 4.dp else 8.dp, bottom = if (compact) 8.dp else 12.dp).fillMaxWidth(),
            verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.weight(1f).heightIn(min = buttonSize).beepFrame(buttonShape, Surface, pressableLook()),
                verticalAlignment = Alignment.CenterVertically) {
                if (!chat.busy && draft.isBlank() && attachedImage == null) {
                    IconButton(onClick = {
                        photoPicker.launch(androidx.activity.result.PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                    }, modifier = Modifier.padding(start = 2.dp)) {
                        Icon(Icons.Outlined.Image, "Attach image", Modifier.size(22.dp), tint = TextMuted)
                    }
                }
                TextField(value = draft, onValueChange = { if (it.toByteArray().size <= 32768) draft = it },
                    modifier = Modifier.weight(1f).focusRequester(inputFocusRequester),
                    placeholder = { Text(if (listening) "Speak now..." else if (attachedImage != null) "Describe the image..." else "Ask anything...", fontSize = 15.sp, color = TextDim) },
                    textStyle = LocalTextStyle.current.copy(fontSize = 15.sp),
                    colors = TextFieldDefaults.colors(unfocusedContainerColor = Color.Transparent, focusedContainerColor = Color.Transparent,
                        unfocusedIndicatorColor = Color.Transparent, focusedIndicatorColor = Color.Transparent, cursorColor = TextPrimary),
                    maxLines = if (compact) 3 else 6,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Default))
            }
            val buttonModifier = Modifier.size(buttonSize)
            when {
                chat.busy -> ChunkyButton(onClick = repository::cancel, modifier = buttonModifier, shape = buttonShape,
                    color = Beep.colors.ink, contentColor = Beep.colors.page, contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.Outlined.Stop, "Stop reply", Modifier.size(22.dp))
                }
                draft.isNotBlank() || attachedImage != null -> {
                    // A quick squish on tap so sending feels physical.
                    val squish = remember { Animatable(1f) }
                    val noMotion = reduceMotion()
                    ChunkyButton(onClick = {
                        submit()
                        if (!noMotion) scope.launch {
                            squish.animateTo(0.82f, tween(70)); squish.animateTo(1f, spring(dampingRatio = 0.4f, stiffness = 600f))
                        }
                    }, modifier = buttonModifier.graphicsLayer { scaleX = squish.value; scaleY = squish.value }, shape = buttonShape,
                        color = Beep.colors.accent, contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(0.dp)) {
                        Icon(Icons.AutoMirrored.Outlined.Send, "Send message", Modifier.size(20.dp))
                    }
                }
                speechAvailable -> {
                    val ring by rememberInfiniteTransition(label = "mic-ring").animateFloat(
                        initialValue = 0f, targetValue = 1f,
                        animationSpec = infiniteRepeatable(tween(1100, easing = LinearOutSlowInEasing)), label = "mic-ring-progress")
                    val ringColor = Beep.colors.accent
                    ChunkyButton(onClick = { startVoice() },
                        modifier = buttonModifier.drawBehind {
                            if (listening) drawCircle(ringColor.copy(alpha = 0.45f * (1 - ring)), radius = size.minDimension * (0.5f + 0.35f * ring))
                        }, shape = buttonShape,
                        color = if (listening) Beep.colors.accent else Surface,
                        contentColor = if (listening) Beep.colors.onAccent else TextPrimary, contentPadding = PaddingValues(0.dp)) {
                        Icon(Icons.Outlined.Mic, if (listening) "Stop listening" else "Voice input", Modifier.size(22.dp))
                    }
                }
                else -> ChunkyButton(onClick = {}, enabled = false, modifier = buttonModifier, shape = buttonShape, contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send message", Modifier.size(20.dp))
                }
            }
        }

    }
}

private fun startListening(
    recognizer: SpeechRecognizer,
    onPartial: (String) -> Unit,
    onResult: (String) -> Unit,
    onError: (Int) -> Unit
) {
    recognizer.setRecognitionListener(object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onRmsChanged(rmsdB: Float) {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEndOfSpeech() {}
        override fun onError(error: Int) { onError(error) }
        override fun onResults(results: Bundle?) {
            val text = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: ""
            if (text.isNotEmpty()) onResult(text)
            else onError(SpeechRecognizer.ERROR_NO_MATCH)
        }
        override fun onPartialResults(partial: Bundle?) {
            val text = partial?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: ""
            if (text.isNotEmpty()) onPartial(text)
        }
        override fun onEvent(eventType: Int, params: Bundle?) {}
    })
    val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
        putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
        putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
    }
    recognizer.startListening(intent)
}

@Composable
private fun ClipboardBanner(clipboardContent: MutableState<String?>, onAction: (String, String) -> Unit, onDismiss: () -> Unit) {
    val clip = clipboardContent.value
    AnimatedVisibility(
        visible = clip != null,
        enter = fadeIn(tween(200)) + slideInVertically(tween(250)) { -it },
        exit = fadeOut(tween(150)) + slideOutVertically(tween(200)) { -it }
    ) {
        val text = clip ?: return@AnimatedVisibility
        val preview = if (text.length > 40) text.take(40).trim() + "..." else text.trim()
        BeepCard(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp)) {
            Column(Modifier.padding(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.ContentPaste, null, Modifier.size(14.dp), tint = Moss)
                    Spacer(Modifier.width(6.dp))
                    Text(
                        preview.replace('\n', ' '),
                        fontSize = 12.sp, color = TextMuted, maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                    Spacer(Modifier.width(4.dp))
                    Icon(
                        Icons.Outlined.Close, "Dismiss",
                        modifier = Modifier.size(18.dp).clickable { onDismiss() },
                        tint = TextMuted
                    )
                }
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ClipboardChip("Ask about this") { onAction("ask", text) }
                    ClipboardChip("Translate") { onAction("translate", text) }
                    ClipboardChip("Summarize") { onAction("summarize", text) }
                }
            }
        }
    }
}

@Composable
private fun ClipboardChip(label: String, onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp)) {
        Text(label, fontSize = 12.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun CopyReply(text: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember(text) { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    TextButton(onClick = {
        (context as? MainActivity)?.suppressNextClipboard = true
        context.getSystemService(android.content.ClipboardManager::class.java)
            .setPrimaryClip(android.content.ClipData.newPlainText("Beep reply", text))
        copied = true
    }, contentPadding = PaddingValues(horizontal = 4.dp), modifier = Modifier.heightIn(min = 48.dp)) {
        Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, if (compact) (if (copied) "Copied" else "Copy reply") else null,
            Modifier.size(14.dp), tint = if (copied) Beep.colors.ok else TextMuted)
        if (!compact) { Spacer(Modifier.width(6.dp)); Text(if (copied) "Copied" else "Copy reply", fontSize = 11.sp, color = TextMuted) }
    }
}

/** Grey lines where the reply will be written, with a soft sheen passing over them. */
@Composable
private fun ReplySkeleton(modifier: Modifier = Modifier) {
    val base = Beep.colors.raised
    val sheen = Beep.colors.surface
    val noMotion = reduceMotion()
    val shift by rememberInfiniteTransition(label = "skeleton").animateFloat(0f, 1f,
        infiniteRepeatable(tween(1300, easing = LinearEasing)), label = "skeleton-shift")
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(9.dp)) {
        listOf(1f, 0.86f, 0.55f).forEach { width ->
            Box(Modifier.fillMaxWidth(width).height(12.dp).clip(CircleShape).drawBehind {
                if (noMotion) { drawRect(base); return@drawBehind }
                val x = size.width * (shift * 2f - 0.5f)
                drawRect(Brush.linearGradient(listOf(base, sheen, base), start = Offset(x - size.width * 0.4f, 0f),
                    end = Offset(x + size.width * 0.4f, 0f)))
            })
        }
    }
}

/** Tap actions on a message. */
@Composable
private fun MessageMenu(open: Boolean, onDismiss: () -> Unit, text: String, onQuote: () -> Unit) {
    val context = androidx.compose.ui.platform.LocalContext.current
    DropdownMenu(expanded = open, onDismissRequest = onDismiss, containerColor = Surface, shape = RoundedCornerShape(Beep.shapes.smallRadius)) {
        DropdownMenuItem(text = { Text("Copy") }, leadingIcon = { Icon(Icons.Outlined.ContentCopy, null, Modifier.size(18.dp)) }, onClick = {
            (context as? MainActivity)?.suppressNextClipboard = true
            context.getSystemService(android.content.ClipboardManager::class.java)
                .setPrimaryClip(android.content.ClipData.newPlainText("Message", text))
            onDismiss()
        })
        DropdownMenuItem(text = { Text("Quote in reply") }, leadingIcon = { Icon(Icons.Outlined.FormatQuote, null, Modifier.size(18.dp)) },
            onClick = { onQuote(); onDismiss() })
        DropdownMenuItem(text = { Text("Share") }, leadingIcon = { Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)) }, onClick = {
            context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null))
            onDismiss()
        })
    }
}

// ── Rich message rendering ──────────────────────────────────────────────────


private sealed class MsgBlock {
    data class Text(val content: String) : MsgBlock()
    data class Code(val language: String, val code: String) : MsgBlock()
    data class Table(val headers: List<String>, val rows: List<List<String>>) : MsgBlock()
    data class Quote(val content: String) : MsgBlock()
    data object Rule : MsgBlock()
}

private fun parseBlocks(text: String): List<MsgBlock> {
    val blocks = mutableListOf<MsgBlock>()
    val lines = text.split('\n')
    var i = 0
    val pending = StringBuilder()
    fun flush() { if (pending.isNotEmpty()) { blocks.add(MsgBlock.Text(pending.toString().trimEnd())); pending.clear() } }
    while (i < lines.size) {
        val line = lines[i]
        if (line.trimStart().startsWith("```")) {
            flush()
            val lang = line.trimStart().removePrefix("```").trim()
            i++
            val code = StringBuilder()
            while (i < lines.size && !lines[i].trimStart().startsWith("```")) { if (code.isNotEmpty()) code.append('\n'); code.append(lines[i]); i++ }
            if (i < lines.size) i++
            blocks.add(MsgBlock.Code(lang, code.toString()))
            continue
        }
        if (line.matches(Regex("^\\s*[-*_]{3,}\\s*$")) && pending.isBlank()) { flush(); blocks.add(MsgBlock.Rule); i++; continue }
        if (line.trimStart().startsWith("|") && line.trimEnd().endsWith("|")) {
            val tableLines = mutableListOf(line)
            var j = i + 1
            while (j < lines.size && lines[j].trimStart().startsWith("|") && lines[j].trimEnd().endsWith("|")) { tableLines.add(lines[j]); j++ }
            if (tableLines.size >= 3 && tableLines[1].contains("---")) {
                flush()
                val headers = tableLines[0].split("|").filter { it.isNotBlank() }.map { it.trim() }
                val rows = tableLines.drop(2).map { row -> row.split("|").filter { it.isNotBlank() }.map { it.trim() } }
                blocks.add(MsgBlock.Table(headers, rows)); i = j; continue
            }
        }
        if (line.trimStart().startsWith("> ") || line.trimStart() == ">") {
            flush()
            val quoteLines = mutableListOf<String>()
            while (i < lines.size && (lines[i].trimStart().startsWith("> ") || lines[i].trimStart() == ">")) {
                quoteLines.add(lines[i].trimStart().removePrefix("> ").removePrefix(">")); i++
            }
            blocks.add(MsgBlock.Quote(quoteLines.joinToString("\n"))); continue
        }
        if (pending.isNotEmpty()) pending.append('\n')
        pending.append(line); i++
    }
    flush()
    return blocks
}

@Composable
private fun PlaceCard(data: org.json.JSONObject, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val name = data.optString("name", "Unknown place")
    val rating = data.optDouble("rating", 0.0)
    val address = data.optString("address", "")
    val lat = data.optDouble("latitude", 0.0)
    val lng = data.optDouble("longitude", 0.0)

    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.padding(14.dp)) {
            Text(name, style = displayStyle(if (compact) 15.sp else 17.sp), color = TextPrimary)
            if (rating > 0) {
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val fullStars = rating.toInt()
                    repeat(fullStars) {
                        Icon(Icons.Outlined.Star, null, Modifier.size(14.dp), tint = Beep.colors.warn)
                    }
                    repeat(5 - fullStars) {
                        Icon(Icons.Outlined.StarBorder, null, Modifier.size(14.dp), tint = TextDim)
                    }
                    Spacer(Modifier.width(6.dp))
                    Text("%.1f".format(rating), fontSize = 12.sp, color = TextMuted)
                }
            }
            if (address.isNotBlank()) {
                Spacer(Modifier.height(4.dp))
                Text(address, fontSize = if (compact) 12.sp else 13.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(12.dp))
            ChunkyButton(
                onClick = {
                    val uri = if (lat != 0.0 && lng != 0.0) "geo:$lat,$lng?q=${Uri.encode(name)}"
                    else "geo:0,0?q=${Uri.encode(name)}"
                    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
                },
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
                shape = RoundedCornerShape(Beep.shapes.smallRadius),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
            ) {
                Icon(Icons.Outlined.Map, null, Modifier.size(16.dp))
                Text("Open in Maps", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

@Composable
private fun LinkCard(data: org.json.JSONObject, onBrowse: (String) -> Unit, compact: Boolean) {
    val title = data.optString("title", "")
    val description = data.optString("description", "")
    val url = data.optString("url", "")

    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.fillMaxWidth().clickable(enabled = url.isNotBlank()) { onBrowse(url) }.padding(14.dp)) {
            if (title.isNotBlank()) {
                Text(title, fontWeight = FontWeight.SemiBold, fontSize = if (compact) 14.sp else 15.sp, color = TextPrimary, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (description.isNotBlank()) {
                if (title.isNotBlank()) Spacer(Modifier.height(3.dp))
                Text(description, fontSize = if (compact) 12.sp else 13.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (url.isNotBlank()) {
                Spacer(Modifier.height(6.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(13.dp), tint = Moss)
                    Spacer(Modifier.width(4.dp))
                    Text(url.removePrefix("https://").removePrefix("http://").take(40), fontSize = 12.sp, color = Moss, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun ActionButtonRow(data: org.json.JSONObject, repository: ChatRepository) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val buttonsArr = data.optJSONArray("buttons") ?: return
    val buttons = (0 until buttonsArr.length()).map { buttonsArr.getJSONObject(it) }

    // Room below for the hard shadow, which the scroll container would otherwise clip.
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(bottom = 6.dp, end = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        buttons.forEachIndexed { index, button ->
            val label = button.optString("label", "Action")
            val action = button.optString("action", "")
            val isPrimary = index == 0
            ChunkyButton(
                onClick = {
                    when (action) {
                        "open_browser" -> repository.openBrowser(button.optString("url", ""))
                        "open_maps" -> {
                            val query = button.optString("query", label)
                            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("geo:0,0?q=${Uri.encode(query)}")))
                        }
                        "open_app" -> repository.pendingIntent.value = AppIntent(
                            app = button.optString("app", ""),
                            uri = button.optString("uri", ""),
                            fallbackUrl = button.optString("fallback_url", ""))
                    }
                },
                color = if (isPrimary) Beep.colors.accent else Surface,
                contentColor = if (isPrimary) Beep.colors.onAccent else TextPrimary,
                shape = RoundedCornerShape(Beep.shapes.smallRadius),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
            ) {
                Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}

@Composable
private fun TaskProgressCard(steps: List<TaskStep>) {
    val noMotion = reduceMotion()
    BeepCard(Modifier.fillMaxWidth().animateContentSize()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            steps.forEach { step ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                        when (step.status) {
                            "pending" -> Box(Modifier.size(14.dp).border(1.5.dp, Border, CircleShape))
                            "in_progress" -> {
                                val pulse = rememberInfiniteTransition(label = "step-pulse")
                                val scale by if (noMotion) remember { mutableStateOf(1f) } else pulse.animateFloat(
                                    initialValue = 0.7f, targetValue = 1f,
                                    animationSpec = infiniteRepeatable(tween(600, easing = EaseInOutCubic), RepeatMode.Reverse),
                                    label = "step-scale")
                                Box(Modifier.size(12.dp).graphicsLayer { scaleX = scale; scaleY = scale }
                                    .background(Beep.colors.accent, CircleShape))
                            }
                            "completed" -> Box(Modifier.size(18.dp).background(Beep.colors.ok, CircleShape), contentAlignment = Alignment.Center) {
                                Icon(Icons.Outlined.Check, "Completed", Modifier.size(12.dp), tint = Color.White)
                            }
                            "failed" -> Box(Modifier.size(18.dp).background(Beep.colors.error, CircleShape), contentAlignment = Alignment.Center) {
                                Icon(Icons.Outlined.Close, "Failed", Modifier.size(12.dp), tint = Color.White)
                            }
                        }
                    }
                    Spacer(Modifier.width(10.dp))
                    Text(
                        step.label,
                        fontSize = 14.sp,
                        fontWeight = if (step.status == "in_progress") FontWeight.Medium else FontWeight.Normal,
                        color = when (step.status) {
                            "completed", "in_progress" -> TextPrimary
                            "failed" -> Beep.colors.error
                            else -> TextMuted
                        }
                    )
                }
            }
        }
    }
}

internal fun toolIconFor(name: String): androidx.compose.ui.graphics.vector.ImageVector {
    val lower = name.lowercase()
    return when {
        "search" in lower || "web" in lower -> Icons.Outlined.Search
        "weather" in lower || "forecast" in lower -> Icons.Outlined.Cloud
        "calendar" in lower || "schedule" in lower || "event" in lower -> Icons.Outlined.CalendarMonth
        "location" in lower || "map" in lower || "place" in lower || "nearby" in lower -> Icons.Outlined.LocationOn
        "email" in lower || "mail" in lower -> Icons.Outlined.Email
        "notification" in lower || "alert" in lower -> Icons.Outlined.Notifications
        "contact" in lower || "person" in lower -> Icons.Outlined.Person
        "memory" in lower || "remember" in lower -> Icons.Outlined.Psychology
        "file" in lower || "document" in lower || "read" in lower -> Icons.Outlined.Description
        "app" in lower || "launch" in lower -> Icons.Outlined.Launch
        "browser" in lower || "page" in lower || "browse" in lower -> Icons.Outlined.Language
        "clipboard" in lower || "copy" in lower -> Icons.Outlined.ContentCopy
        "image" in lower || "vision" in lower || "screenshot" in lower -> Icons.Outlined.Image
        else -> Icons.Outlined.Build
    }
}

@Composable
private fun CardView(card: Card, repository: ChatRepository, compact: Boolean) {
    when (card.type) {
        "place" -> PlaceCard(card.data, compact)
        "link" -> LinkCard(card.data, onBrowse = { repository.openBrowser(it) }, compact)
        "action_buttons" -> ActionButtonRow(card.data, repository)
    }
}

/** Colors the inline formatter needs, read from the theme once per composition. */
internal data class InlineColors(val link: Color, val muted: Color, val code: Color, val onCode: Color)

@Composable
@ReadOnlyComposable
private fun inlineColors() = Beep.colors.let { InlineColors(it.brand, it.muted, it.code, it.onCode) }

@Composable
private fun MessageContent(text: String, role: String, compact: Boolean, padded: Boolean = role == "user") {
    val blocks = remember(text) { parseBlocks(text) }
    val inline = inlineColors()
    val color = if (role == "user") Beep.colors.onUser else TextPrimary
    val padding = if (padded) PaddingValues(horizontal = if (compact) 12.dp else 16.dp, vertical = if (compact) 9.dp else 12.dp)
        else PaddingValues(horizontal = 2.dp, vertical = 2.dp)
    // One container for the whole message, so a selection can run across paragraphs, code and quotes.
    SelectionContainer { Column(Modifier.padding(padding), verticalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 10.dp)) {
        blocks.forEach { block ->
            when (block) {
                is MsgBlock.Text -> Text(remember(block.content, inline) { formattedMessage(block.content, inline) }, color = color,
                    fontSize = if (compact) 14.sp else 16.sp, lineHeight = if (compact) 21.sp else 25.sp)
                is MsgBlock.Code -> CodeBlockView(block.language, block.code, compact)
                is MsgBlock.Table -> TableView(block.headers, block.rows, compact)
                is MsgBlock.Quote -> BlockQuoteView(block.content, compact)
                MsgBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = 6.dp), color = Border)
            }
        }
    } }
}

@Composable
private fun CodeBlockView(language: String, code: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius)).background(Beep.colors.code)) {
        DisableSelection { Row(Modifier.fillMaxWidth().padding(start = 14.dp, end = 8.dp, top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(language.ifBlank { "code" }, fontSize = 11.sp, color = TextMuted, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Medium)
            Spacer(Modifier.weight(1f))
            Row(Modifier.clip(CircleShape).clickable {
                (context as? MainActivity)?.suppressNextClipboard = true
                context.getSystemService(android.content.ClipboardManager::class.java)
                    .setPrimaryClip(android.content.ClipData.newPlainText("Code", code))
                copied = true
            }.padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, null,
                    Modifier.size(13.dp), tint = if (copied) Beep.colors.ok else TextMuted)
                Spacer(Modifier.width(5.dp))
                Text(if (copied) "Copied" else "Copy", fontSize = 11.sp, color = if (copied) Beep.colors.ok else TextMuted)
            }
        } }
        Text(code, modifier = Modifier.horizontalScroll(rememberScrollState()).padding(start = 14.dp, end = 14.dp, top = 4.dp, bottom = 14.dp),
            fontFamily = FontFamily.Monospace, fontSize = if (compact) 12.sp else 13.sp,
            lineHeight = if (compact) 18.sp else 20.sp, color = Beep.colors.onCode)
    }
}

@Composable
private fun TableView(headers: List<String>, rows: List<List<String>>, compact: Boolean) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius))
        .border(1.5.dp, Border, RoundedCornerShape(Beep.shapes.smallRadius)).background(Surface)
        .horizontalScroll(rememberScrollState())) {
        Row(Modifier.background(Beep.colors.code).padding(vertical = 10.dp, horizontal = 4.dp)) {
            headers.forEach { header ->
                Text(header, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                    fontSize = if (compact) 11.sp else 12.sp, fontWeight = FontWeight.SemiBold,
                    color = TextPrimary, maxLines = 2)
            }
        }
        rows.forEachIndexed { index, row ->
            if (index > 0) HorizontalDivider(color = Border, thickness = 1.dp)
            Row(Modifier.padding(vertical = 9.dp, horizontal = 4.dp)) {
                row.forEach { cell ->
                    Text(cell, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                        fontSize = if (compact) 11.sp else 12.sp, color = TextPrimary,
                        lineHeight = if (compact) 16.sp else 18.sp, maxLines = 4)
                }
            }
        }
    }
}

@Composable
private fun BlockQuoteView(content: String, compact: Boolean) {
    val bar = Beep.colors.user
    val inline = inlineColors()
    Box(Modifier.fillMaxWidth().drawBehind {
        drawRoundRect(bar, size = androidx.compose.ui.geometry.Size(4.dp.toPx(), size.height),
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(2.dp.toPx()))
    }) {
        Text(remember(content, inline) { formattedMessage(content, inline) },
            modifier = Modifier.padding(start = 16.dp, end = 10.dp, top = 4.dp, bottom = 4.dp),
            fontSize = if (compact) 13.sp else 15.sp, lineHeight = if (compact) 19.sp else 23.sp,
            color = TextMuted)
    }
}

// Links, bare urls, ***bold italic***, **bold**, *italic*, _italic_, ~~strike~~, `code`. The leftmost match wins,
// so asterisks inside code stay literal; italics need a non-space just inside and no word character just outside,
// which keeps `2 * 3 * 4`, snake_case and __dunder__ plain.
private val inlineTokens = Regex("""\[([^\]]+)\]\((https?://[^\s)]+)\)|(https?://[^\s<>]+)|\*\*\*(?!\s)(.+?)(?<!\s)\*\*\*|\*\*(?!\s)(.+?)(?<!\s)\*\*""" +
    """|(?<![\w*])\*(?![\s*])(.+?)(?<![\s*])\*(?![\w*])|(?<![\w_])_(?![\s_])(.+?)(?<![\s_])_(?![\w_])|~~(.+?)~~|`([^`\n]+)`""")

// Inline markdown plus headings and bullets, line by line.
internal fun formattedMessage(text: String, colors: InlineColors) = buildAnnotatedString {
    val lines = text.split('\n')
    var firstLine = true
    for (line in lines) {
        if (!firstLine) append("\n")
        firstLine = false
        val heading = Regex("^#{1,4} +").find(line)
        val content = line.removePrefix(heading?.value.orEmpty())
            .replace(Regex("^( *)[-*+] +"), "$1\u2022 ")
        pushStyle(if (heading != null) SpanStyle(fontWeight = FontWeight.Bold, fontSize = 17.sp, letterSpacing = (-0.3).sp) else SpanStyle())
        var offset = 0
        for (match in inlineTokens.findAll(content)) {
            append(content.substring(offset, match.range.first))
            when {
                match.groups[4] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, fontStyle = FontStyle.Italic)) { append(match.groups[4]!!.value) }
                match.groups[5] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(match.groups[5]!!.value) }
                match.groups[6] != null || match.groups[7] != null -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append((match.groups[6] ?: match.groups[7])!!.value) }
                match.groups[8] != null -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough, color = colors.muted)) { append(match.groups[8]!!.value) }
                match.groups[9] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = colors.code, color = colors.onCode, fontSize = 14.sp)) { append("\u200A${match.groups[9]!!.value}\u200A") }
                else -> {
                    val url = (match.groups[2]?.value ?: match.groups[3]!!.value).trimEnd('.', ',', ';')
                    withLink(LinkAnnotation.Url(url, TextLinkStyles(style = SpanStyle(color = colors.link, textDecoration = TextDecoration.Underline)))) {
                        append(match.groups[1]?.value ?: url)
                    }
                    if (match.groups[3] != null) append(match.value.removePrefix(url))
                }
            }
            offset = match.range.last + 1
        }
        append(content.substring(offset)); pop()
    }
}

/**
 * Scroll to the very bottom, including the tail of a last item taller than the screen.
 * When the last item is already on screen it only scrolls by what is left of it, since
 * scrolling to the item first would line up its top and bounce a long reply up then down.
 */
private suspend fun LazyListState.scrollToEnd(animated: Boolean) {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    if (layoutInfo.visibleItemsInfo.lastOrNull()?.index != last) scrollToItem(last)
    val item = layoutInfo.visibleItemsInfo.lastOrNull() ?: return
    val overflow = item.offset + item.size - (layoutInfo.viewportEndOffset - layoutInfo.afterContentPadding)
    if (overflow > 0) { if (animated) animateScrollBy(overflow.toFloat()) else scrollBy(overflow.toFloat()) }
}

@Composable
private fun PulsingDots() {
    val noMotion = reduceMotion()
    val dot = Beep.colors.accent
    if (noMotion) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            repeat(3) { Box(Modifier.size(6.dp).background(dot, CircleShape)) }
        }
        return
    }
    val infiniteTransition = rememberInfiniteTransition(label = "dots")
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        repeat(3) { index ->
            val lift by infiniteTransition.animateFloat(
                initialValue = 0f, targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = tween(380, easing = EaseInOutCubic, delayMillis = index * 120),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-lift-$index"
            )
            Box(Modifier.size(6.dp).graphicsLayer { translationY = -4.dp.toPx() * lift }
                .background(dot.copy(alpha = 0.45f + 0.55f * lift), CircleShape))
        }
    }
}

@Composable
private fun ConnectionDot(connected: Boolean, status: String) {
    val reconnecting = !connected && status.contains("Reconnecting", ignoreCase = true)
    if (reconnecting) {
        val infiniteTransition = rememberInfiniteTransition(label = "reconnect-dot")
        val alpha by infiniteTransition.animateFloat(0.3f, 1f, infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "reconnect-alpha")
        Box(Modifier.size(7.dp).background(Beep.colors.warn.copy(alpha = alpha), CircleShape))
    } else {
        Box(Modifier.size(7.dp).background(if (connected) Beep.colors.ok else Beep.colors.error, CircleShape))
    }
}

@Composable
fun BeepAvatar(isThinking: Boolean, modifier: Modifier = Modifier) {
    val noMotion = reduceMotion()
    val infiniteTransition = rememberInfiniteTransition(label = "beep-avatar")
    val face = Beep.colors.user
    val line = Beep.colors.line
    val outline = Beep.shapes.outline
    val dot = Beep.colors.onUser

    // Idle: still. Thinking: a small bob.
    val bob by if (noMotion || !isThinking) remember { mutableStateOf(0f) } else infiniteTransition.animateFloat(
        initialValue = 0f, targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(520, easing = EaseInOutCubic), RepeatMode.Reverse),
        label = "avatar-bob"
    )

    Box(modifier.size(30.dp)
        .graphicsLayer { translationY = -2.dp.toPx() * bob; rotationZ = -6f * bob }
        .background(face, CircleShape)
        .then(if (outline > 0.dp) Modifier.border(outline, line, CircleShape) else Modifier),
        contentAlignment = Alignment.Center
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.CenterVertically) {
            repeat(3) { index ->
                val offset by if (isThinking && !noMotion) infiniteTransition.animateFloat(
                    initialValue = 0f, targetValue = -3f,
                    animationSpec = infiniteRepeatable(
                        animation = tween(400, delayMillis = index * 120, easing = EaseInOutCubic),
                        repeatMode = RepeatMode.Reverse
                    ), label = "dot-bounce-$index"
                ) else remember { mutableStateOf(0f) }
                Canvas(Modifier.size(4.dp).graphicsLayer { translationY = offset.dp.toPx() }) { drawCircle(dot) }
            }
        }
    }
}

/** An icon that hints at what a starter is about, picked from its words. */
private fun starterIcon(text: String): androidx.compose.ui.graphics.vector.ImageVector {
    val t = text.lowercase()
    fun has(vararg words: String) = words.any { it in t }
    return when {
        has("remind", "alarm", "timer") -> Icons.Outlined.Alarm
        has("remember", "note", "save") -> Icons.Outlined.BookmarkBorder
        has("mail", "inbox") -> Icons.Outlined.Email
        has("calendar", "meeting", "schedule", "week", "today", "tomorrow") -> Icons.Outlined.CalendarMonth
        has("map", "near", "place", "restaurant", "route", "where") -> Icons.Outlined.Place
        has("weather", "rain", "forecast") -> Icons.Outlined.WbSunny
        has("search", "news", "latest", "find", "look up") -> Icons.Outlined.Search
        has("plan", "idea", "brainstorm", "write", "draft") -> Icons.Outlined.Lightbulb
        else -> Icons.Outlined.AutoAwesome
    }
}

@Composable
private fun Suggestion(title: String, prompt: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tint: Color, onClick: (String) -> Unit) {
    ChunkyButton(onClick = { onClick(prompt) }, modifier = Modifier.fillMaxWidth().padding(end = 3.dp, bottom = 3.dp),
        shape = RoundedCornerShape(Beep.shapes.radius), contentPadding = PaddingValues(12.dp)) {
        Box(Modifier.size(34.dp).background(tint, RoundedCornerShape(Beep.shapes.smallRadius)), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = TextPrimary, modifier = Modifier.size(18.dp))
        }
        Spacer(Modifier.width(12.dp))
        Text(title, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = TextPrimary, modifier = Modifier.weight(1f))
        Icon(Icons.Outlined.NorthEast, null, tint = TextMuted, modifier = Modifier.size(16.dp))
    }
}

/** Swipe left starts a new chat, swipe right goes back to the last one, or opens the chat list when there is none. */
@Composable
private fun SwipeChats(canStartNew: Boolean, hasPrevious: Boolean, onNew: () -> Unit, onBack: () -> Unit,
                       modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    val density = LocalDensity.current
    val trigger = with(density) { 72.dp.toPx() }
    val limit = with(density) { 110.dp.toPx() }
    val latestNew by rememberUpdatedState(onNew)
    val latestBack by rememberUpdatedState(onBack)
    val latestCanStartNew by rememberUpdatedState(canStartNew)
    Box(modifier.pointerInput(Unit) {
        detectHorizontalDragGestures(
            onDragEnd = {
                val moved = offset.value
                if (moved <= -trigger && latestCanStartNew) latestNew() else if (moved >= trigger) latestBack()
                scope.launch { offset.animateTo(0f, spring(dampingRatio = Spring.DampingRatioMediumBouncy)) }
            },
            onDragCancel = { scope.launch { offset.animateTo(0f) } },
        ) { change, amount ->
            change.consume()
            // Damped so the chat lags the finger, and capped once past the trigger.
            val min = if (latestCanStartNew) -limit else -limit / 4
            scope.launch { offset.snapTo((offset.value + amount * 0.5f).coerceIn(min, limit)) }
        }
    }) {
        val progress = (abs(offset.value) / trigger).coerceIn(0f, 1f)
        if (offset.value < 0 && canStartNew)
            SwipeHint(Icons.Outlined.Add, "New chat", progress, Modifier.align(Alignment.CenterEnd).padding(end = 16.dp))
        if (offset.value > 0)
            SwipeHint(if (hasPrevious) Icons.AutoMirrored.Outlined.ArrowBack else Icons.Outlined.Menu,
                if (hasPrevious) "Previous" else "All chats", progress, Modifier.align(Alignment.CenterStart).padding(start = 16.dp))
        Box(Modifier.fillMaxSize().graphicsLayer { translationX = offset.value; alpha = 1f - progress * 0.25f }) { content() }
    }
}

@Composable
private fun SwipeHint(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, progress: Float, modifier: Modifier) {
    val ready = progress >= 1f
    Column(modifier.alpha(progress).graphicsLayer { scaleX = 0.8f + progress * 0.2f; scaleY = scaleX },
        horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(44.dp).background(if (ready) Beep.colors.accent else Beep.colors.surface, CircleShape)
            .border(2.dp, Beep.colors.line, CircleShape), contentAlignment = Alignment.Center) {
            Icon(icon, null, Modifier.size(22.dp), tint = if (ready) Beep.colors.onAccent else TextPrimary)
        }
        Spacer(Modifier.height(6.dp))
        Text(label, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)
    }
}

@Composable
private fun DrawerTile(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tint: Color, modifier: Modifier = Modifier, onClick: () -> Unit) {
    ChunkyButton(onClick = onClick, modifier = modifier.padding(end = 3.dp, bottom = 3.dp), color = tint,
        shape = RoundedCornerShape(Beep.shapes.smallRadius), contentPadding = PaddingValues(horizontal = 6.dp, vertical = 12.dp)) {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(icon, null, tint = TextPrimary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.height(6.dp))
            Text(label, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextPrimary, maxLines = 1,
                overflow = TextOverflow.Ellipsis)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemoriesScreen(repository: ChatRepository, onBack: () -> Unit) {
    var memories by remember { mutableStateOf<List<ChatRepository.Memory>>(emptyList()) }
    var query by rememberSaveable { mutableStateOf("") }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<ChatRepository.Memory?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { memories = withContext(Dispatchers.IO) { repository.listMemories() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    fun act(action: suspend () -> Unit) { scope.launch {
        try { withContext(Dispatchers.IO) { action() }; refresh() }
        catch (e: Exception) { error = e.message.orEmpty() }
    } }
    LaunchedEffect(Unit) { refresh() }
    val today = remember { LocalDate.now().toString() }
    val groups = memories
        .filter { query.isBlank() || it.content.contains(query, true) || it.category.contains(query, true) }
        .sortedByDescending { it.updated }
        .groupBy { it.category }
        .toSortedMap()
    val shown = groups.values.sumOf { it.size }
    editing?.let { memory ->
        var text by remember(memory.id) { mutableStateOf(memory.content) }
        AlertDialog(
            onDismissRequest = { editing = null },
            confirmButton = { TextButton(onClick = {
                val content = text.trim()
                if (content.isNotEmpty()) { act { repository.updateMemory(memory.id, content) }; editing = null }
            }) { Text("Save") } },
            dismissButton = { TextButton(onClick = { editing = null }) { Text("Cancel") } },
            title = { Text("Edit memory") },
            text = { OutlinedTextField(text, { text = it }, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) },
            containerColor = Surface)
    }
    if (confirmClear) AlertDialog(
        onDismissRequest = { confirmClear = false },
        confirmButton = { TextButton(onClick = { act { repository.clearMemories() }; confirmClear = false }) { Text("Forget all", color = Coral) } },
        dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("Cancel") } },
        title = { Text("Forget everything?") },
        text = { Text("Beep will forget all ${memories.size} memories. This cannot be undone.") },
        containerColor = Surface)
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText("Memories"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { if (memories.isNotEmpty()) TextButton(onClick = { confirmClear = true }) { Text("Clear all", color = Coral, fontSize = 13.sp) } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(query, { query = it }, placeholder = { Text("Search memories", fontSize = 13.sp) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.weight(1f),
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                Spacer(Modifier.width(12.dp))
                Text("$shown", fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp))
            if (loading && memories.isEmpty()) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (shown == 0) Text(if (memories.isEmpty()) "Nothing remembered yet.\nBeep learns as you chat, or tell it \"remember that...\"" else "No matches.",
                fontSize = 13.sp, color = TextMuted, lineHeight = 20.sp, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                groups.forEach { (category, list) ->
                    item(key = "category-$category") {
                        Text(category.replaceFirstChar { it.uppercase() }, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextDim,
                            modifier = Modifier.padding(start = 4.dp, top = 10.dp, bottom = 2.dp))
                    }
                    items(list, key = { it.id }) { memory -> MemoryCard(memory, today, onEdit = { editing = memory }, onDelete = { act { repository.deleteMemory(memory.id) } }) }
                }
            }
        }
    }
}

/** A `YYYY-MM-DD` date as "9 Oct", with the year when it is not this year. */
private fun shortDate(date: String): String = try {
    val day = LocalDate.parse(date)
    val pattern = if (day.year == LocalDate.now().year) "d MMM" else "d MMM yyyy"
    day.format(DateTimeFormatter.ofPattern(pattern))
} catch (_: Exception) { date }

@Composable
private fun MemoryCard(memory: ChatRepository.Memory, today: String, onEdit: () -> Unit, onDelete: () -> Unit) {
    val ended = memory.validUntil?.let { it < today } == true
    val starts = memory.validFrom?.takeIf { it > today }?.let { "Starts ${shortDate(it)}" }
    val expires = memory.validUntil?.let { if (ended) "Expired ${shortDate(it)}" else "Expires ${shortDate(it)}" }
    val origin = when (memory.source) {
        MemorySource.USER -> "You told Beep"
        MemorySource.EMAIL -> "From your email"
        MemorySource.SLACK -> "From Slack"
        MemorySource.LEARNED -> "Learned"
    }
    val details = listOfNotNull(origin, shortDate(memory.updated), starts)
    BeepCard(Modifier.fillMaxWidth().clickable(onClick = onEdit)) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f)) {
                Text(memory.content, fontSize = 13.sp, lineHeight = 19.sp, color = if (ended) TextMuted else Color.Unspecified)
                Spacer(Modifier.height(4.dp))
                Text(details.joinToString(" \u00b7 "), fontSize = 11.sp, color = TextDim)
                if (expires != null) {
                    Row(Modifier.padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Outlined.Schedule, null, Modifier.size(12.dp), tint = if (ended) TextDim else Beep.colors.accent)
                        Spacer(Modifier.width(4.dp))
                        Text(expires, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = if (ended) TextDim else Beep.colors.accent)
                    }
                }
            }
            IconButton(onClick = onDelete, modifier = Modifier.size(36.dp)) {
                Icon(Icons.Outlined.Close, "Forget", Modifier.size(16.dp), tint = TextDim)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CronsScreen(repository: ChatRepository, onBack: () -> Unit) {
    var crons by remember { mutableStateOf<List<ChatRepository.Cron>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    var showCreate by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { crons = withContext(Dispatchers.IO) { repository.listCrons() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    if (showCreate) {
        CreateCronDialog(onDismiss = { showCreate = false }, onCreate = { label, prompt, schedule ->
            scope.launch {
                try { withContext(Dispatchers.IO) { repository.createCron(label, prompt, schedule) }; showCreate = false; refresh() }
                catch (e: Exception) { error = e.message.orEmpty() }
            }
        })
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText("Automations"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { ChunkyButton(onClick = { showCreate = true }, modifier = Modifier.padding(end = 14.dp), color = Beep.colors.accent,
                contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(8.dp)) { Icon(Icons.Outlined.Add, "New automation", Modifier.size(20.dp)) } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (crons.isEmpty()) Text("No automations yet.\nSchedule tasks for Beep to run on a timer.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(crons, key = { it.id }) { cron ->
                    BeepCard(Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(cron.label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(cron.prompt, fontSize = 12.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 18.sp)
                                Spacer(Modifier.height(4.dp))
                                Text(cron.schedule, fontSize = 11.sp, color = TextDim, fontFamily = FontFamily.Monospace)
                            }
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Switch(checked = cron.enabled, onCheckedChange = { enabled ->
                                    scope.launch {
                                        try { withContext(Dispatchers.IO) { repository.toggleCron(cron.id, enabled) }; refresh() }
                                        catch (e: Exception) { error = e.message.orEmpty() }
                                    }
                                }, modifier = Modifier.height(28.dp), colors = SwitchDefaults.colors(checkedTrackColor = Beep.colors.ok, uncheckedTrackColor = Border))
                                IconButton(onClick = { scope.launch {
                                    try { withContext(Dispatchers.IO) { repository.deleteCron(cron.id) }; refresh() }
                                    catch (e: Exception) { error = e.message.orEmpty() }
                                } }, modifier = Modifier.size(32.dp)) {
                                    Icon(Icons.Outlined.Delete, "Delete", Modifier.size(16.dp), tint = TextDim)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun CreateCronDialog(onDismiss: () -> Unit, onCreate: (String, String, String) -> Unit) {
    var label by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var schedule by remember { mutableStateOf("") }
    AlertDialog(onDismissRequest = onDismiss, containerColor = Surface,
        title = { Text(displayText("New automation"), style = displayStyle(22.sp)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(label, { label = it }, label = { Text("Name") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Morning briefing") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(schedule, { schedule = it }, label = { Text("Schedule (cron)") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("0 9 * * *") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(prompt, { prompt = it }, label = { Text("Prompt") }, maxLines = 4, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Give me a summary of...") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
        } },
        confirmButton = { ChunkyButton(onClick = { onCreate(label, prompt, schedule) }, color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
            enabled = label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) { Text("Create", fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun IntegrationsScreen(repository: ChatRepository, onBack: () -> Unit) {
    var integrations by remember { mutableStateOf<List<ChatRepository.McpConnection>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    var showCreate by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { integrations = withContext(Dispatchers.IO) { repository.listIntegrations() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    if (showCreate) {
        CreateIntegrationDialog(onDismiss = { showCreate = false }, onCreate = { provider, label ->
            scope.launch {
                try { withContext(Dispatchers.IO) { repository.createIntegration(provider, label) }; showCreate = false; refresh() }
                catch (e: Exception) { error = e.message.orEmpty() }
            }
        })
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText("Custom connections"), style = displayStyle(24.sp)) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { ChunkyButton(onClick = { showCreate = true }, modifier = Modifier.padding(end = 14.dp), color = Beep.colors.accent,
                contentColor = Beep.colors.onAccent, contentPadding = PaddingValues(8.dp)) { Icon(Icons.Outlined.Add, "New integration", Modifier.size(20.dp)) } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (integrations.isEmpty()) Text("No integrations yet.\nConnect services like email, calendar, and more.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(integrations, key = { it.id }) { integration ->
                    BeepCard(Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(integration.label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(integration.provider, fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(4.dp))
                                Text(integration.status, fontSize = 11.sp,
                                    color = if (integration.status == "connected") Beep.colors.ok else TextDim,
                                    fontWeight = FontWeight.Medium)
                            }
                            IconButton(onClick = { scope.launch {
                                try { withContext(Dispatchers.IO) { repository.deleteIntegration(integration.id) }; refresh() }
                                catch (e: Exception) { error = e.message.orEmpty() }
                            } }, modifier = Modifier.size(32.dp)) {
                                Icon(Icons.Outlined.Delete, "Delete", Modifier.size(16.dp), tint = TextDim)
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CreateIntegrationDialog(onDismiss: () -> Unit, onCreate: (String, String) -> Unit) {
    val providers = listOf("fastmail", "composio", "custom")
    var selectedProvider by remember { mutableStateOf(providers[0]) }
    var label by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }
    AlertDialog(onDismissRequest = onDismiss, containerColor = Surface,
        title = { Text(displayText("New integration"), style = displayStyle(22.sp)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }) {
                OutlinedTextField(selectedProvider, {}, readOnly = true, label = { Text("Provider") }, singleLine = true,
                    modifier = Modifier.fillMaxWidth().menuAnchor(),
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                    providers.forEach { provider ->
                        DropdownMenuItem(text = { Text(provider) }, onClick = { selectedProvider = provider; expanded = false })
                    }
                }
            }
            OutlinedTextField(label, { label = it }, label = { Text("Label") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("My Email") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
        } },
        confirmButton = { ChunkyButton(onClick = { onCreate(selectedProvider, label) }, color = Beep.colors.accent, contentColor = Beep.colors.onAccent,
            enabled = label.isNotBlank()) { Text("Create", fontWeight = FontWeight.SemiBold) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}

@Composable
private fun OnboardingScreen(
    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>?,
    onDone: () -> Unit
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var locationGranted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
        )
    }
    var notificationGranted by remember {
        mutableStateOf(
            Build.VERSION.SDK_INT < 33 ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        )
    }
    // Re-check permissions when returning from system dialogs
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                locationGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
                    ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
                notificationGranted = Build.VERSION.SDK_INT < 33 ||
                    ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    Scaffold(containerColor = Ink) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(28.dp),
            verticalArrangement = Arrangement.Center
        ) {
            Box(Modifier.popIn("onboarding-logo").beepFrame(RoundedCornerShape(Beep.shapes.radius), Beep.colors.user, pressableLook())) {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
            }
            Spacer(Modifier.height(28.dp))
            Text(displayText("Almost ready."), style = displayStyle(36.sp, 40.sp))
            Spacer(Modifier.height(8.dp))
            Text("A couple of permissions to make Beep more useful. You can skip any of these.", color = TextMuted, lineHeight = 22.sp)
            Spacer(Modifier.height(32.dp))

            // Location permission
            PermissionCard(
                icon = Icons.Outlined.LocationOn,
                title = "Location",
                description = "Let Beep answer questions about nearby places, weather, and directions.",
                granted = locationGranted,
                onRequest = {
                    locationPermission?.launch(arrayOf(
                        Manifest.permission.ACCESS_FINE_LOCATION,
                        Manifest.permission.ACCESS_COARSE_LOCATION
                    ))
                }
            )
            Spacer(Modifier.height(12.dp))

            // Notification permission
            if (Build.VERSION.SDK_INT >= 33) {
                val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
                    notificationGranted = granted
                }
                PermissionCard(
                    icon = Icons.Outlined.Notifications,
                    title = "Notifications",
                    description = "Get notified when scheduled automations finish or when Beep has something for you.",
                    granted = notificationGranted,
                    onRequest = { notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS) }
                )
                Spacer(Modifier.height(12.dp))
            }

            Spacer(Modifier.height(20.dp))
            ChunkyButton(
                onClick = {
                    if (locationGranted) {
                        // Trigger location updates now that permission is granted
                        (context as? MainActivity)?.startLocationUpdates()
                    }
                    onDone()
                },
                modifier = Modifier.fillMaxWidth().height(56.dp),
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent
            ) {
                Text("Continue to Beep", fontWeight = FontWeight.SemiBold, fontSize = 16.sp, modifier = Modifier.weight(1f), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            }
            Spacer(Modifier.height(8.dp))
            TextButton(
                onClick = onDone,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Skip for now", color = TextMuted)
            }
        }
    }
}

@Composable
private fun PermissionCard(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    description: String,
    granted: Boolean,
    onRequest: () -> Unit
) {
    BeepCard(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = if (granted) Beep.colors.ok else TextMuted, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                Text(description, fontSize = 12.sp, color = TextMuted, lineHeight = 17.sp)
            }
            Spacer(Modifier.width(12.dp))
            if (granted) {
                Icon(Icons.Outlined.CheckCircle, "Granted", tint = Beep.colors.ok, modifier = Modifier.size(24.dp))
            } else {
                ChunkyButton(onClick = onRequest, color = Beep.colors.user, contentColor = Beep.colors.onUser,
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp)) {
                    Text("Allow", fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ConnectionScreen(initial: ConnectionSettings, canGoBack: Boolean, onBack: () -> Unit, onConnect: suspend (ConnectionSettings) -> Unit) {
    var nodeId by remember { mutableStateOf(initial.nodeId) }
    var token by remember { mutableStateOf(initial.token) }
    var reveal by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current
    val profilePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                val config = withContext(Dispatchers.IO) {
                    val raw = context.contentResolver.openInputStream(uri)?.use { it.readProfileBytes() } ?: error("Cannot open profile")
                    require(raw.size <= 65536) { "Profile is too large" }; ConnectionSettings.fromJson(raw.toString(Charsets.UTF_8))
                }
                nodeId = config.nodeId; token = config.token; error = ""
            } catch (e: Exception) { error = e.message.orEmpty() }
        }
    }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text(displayText(if (canGoBack) "Connection" else "Welcome to Beep"), style = displayStyle(24.sp)) },
            navigationIcon = { if (canGoBack) IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize().imePadding(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(66.dp))
                Spacer(Modifier.height(24.dp))
                Text(displayText("Your assistant.\nYour space."), style = displayStyle(38.sp, 42.sp))
                Spacer(Modifier.height(12.dp))
                Text("Connect to your Beep server to bring your conversations and memory along.", color = TextMuted, lineHeight = 23.sp)
            }
            item { ChunkyButton(onClick = { profilePicker.launch(arrayOf("application/json", "text/*", "application/octet-stream")) }, enabled = !saving, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Outlined.FileOpen, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Import connection profile", fontWeight = FontWeight.Medium)
            } }
            item { OutlinedTextField(nodeId, { nodeId = it }, label = { Text("Node ID") }, placeholder = { Text("hex public key") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving) }
            item { OutlinedTextField(token, { token = it }, label = { Text("App token (optional in testing)") }, singleLine = true, modifier = Modifier.fillMaxWidth(), enabled = !saving,
                visualTransformation = if (reveal) VisualTransformation.None else PasswordVisualTransformation(),
                trailingIcon = { IconButton(onClick = { reveal = !reveal }) { Icon(if (reveal) Icons.Outlined.VisibilityOff else Icons.Outlined.Visibility, "Toggle token visibility") } }) }
            item {
                Text("The node ID is printed when the server starts. Testing servers do not require an app token.", fontSize = 12.sp, color = TextMuted, lineHeight = 18.sp)
                if (canGoBack) Text("Changing the server or account clears this device’s chat cache. Server history is kept.", modifier = Modifier.padding(top = 8.dp), fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) item { Text(error, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
            item { ChunkyButton(onClick = {
                saving = true; error = ""
                scope.launch {
                    try { onConnect(ConnectionSettings(nodeId.trim(), token.trim())) }
                    catch (e: Exception) { error = e.message ?: "Could not connect" }
                    finally { saving = false }
                }
            }, enabled = !saving && nodeId.isNotBlank(), modifier = Modifier.fillMaxWidth().height(56.dp),
                color = Beep.colors.accent, contentColor = Beep.colors.onAccent) {
                Spacer(Modifier.weight(1f))
                if (saving) { CircularProgressIndicator(Modifier.size(18.dp), color = Beep.colors.onAccent, strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                Text(if (saving) "Connecting…" else "Connect to Beep", fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
                Spacer(Modifier.weight(1f))
            } }
        }
    }
}
