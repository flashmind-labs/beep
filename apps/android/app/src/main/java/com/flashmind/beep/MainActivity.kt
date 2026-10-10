package com.flashmind.beep

import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.activity.compose.BackHandler
import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.Geocoder
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.animation.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import com.flashmind.beep.ffi.groupToken
import com.flashmind.beep.ffi.shareToken
import com.flashmind.beep.proto.Group
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import com.flashmind.beep.proto.SessionMatch
import kotlinx.coroutines.delay
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import android.graphics.Outline
import android.view.Gravity
import android.view.View
import android.view.ViewOutlineProvider
import android.view.WindowInsets
import androidx.compose.ui.draw.clip
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.Locale

// Short names for the current theme's colors.
private val Ink: Color @Composable @ReadOnlyComposable get() = Beep.colors.page
private val Surface: Color @Composable @ReadOnlyComposable get() = Beep.colors.surface
private val Border: Color @Composable @ReadOnlyComposable get() = Beep.colors.softLine
private val Moss: Color @Composable @ReadOnlyComposable get() = Beep.colors.brand
private val Coral: Color @Composable @ReadOnlyComposable get() = Beep.colors.error
private val TextMuted: Color @Composable @ReadOnlyComposable get() = Beep.colors.muted

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
    private val pendingOpenRead = mutableStateOf(false)
    /** The token of a `beep://share/` link to open. */
    private val pendingShareToken = mutableStateOf<String?>(null)
    /** The token of a `beep://group/` invite to open. */
    private val pendingGroupToken = mutableStateOf<String?>(null)
    /** A group to open, from its notification. */
    private val pendingGroup = mutableStateOf<String?>(null)
    /** A chat to open, from its notification, a widget or a share. */
    private val pendingChat = mutableStateOf<String?>(null)
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
        // No listener: the last known fix is read only when a report is due.
        repository.startLocationUpdates {
            val loc = try {
                locationManager.getLastKnownLocation(provider)
                    ?: locationManager.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?: locationManager.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            } catch (_: Exception) { null }
            loc?.let { ChatRepository.Fix(it.latitude, it.longitude, it.accuracy, placeName(it.latitude, it.longitude)) }
        }
    }
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
        intent.getStringExtra("conversation_id")?.let { (application as BeepApplication).repository.select(it); pendingChat.value = it }
        if (intent.getBooleanExtra("focus_input", false)) pendingFocusInput.value = true
        if (opensApps(intent)) pendingOpenApps.value = true
        if (intent.getBooleanExtra("open_read", false)) pendingOpenRead.value = true
        takeLinks(intent)
    }
    private fun takeLinks(intent: Intent?) {
        intent?.data?.let { shareToken(it.toString()) }?.let { pendingShareToken.value = it }
        intent?.data?.let { groupToken(it.toString()) }?.let { pendingGroupToken.value = it }
        intent?.getStringExtra("group_id")?.let { pendingGroup.value = it }
    }
    override fun onResume() {
        super.onResume()
        val repository = (application as BeepApplication).repository
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
        intent?.getStringExtra("conversation_id")?.let { repository.select(it); pendingChat.value = it }
        if (intent?.getBooleanExtra("focus_input", false) == true) pendingFocusInput.value = true
        if (opensApps(intent)) pendingOpenApps.value = true
        if (intent?.getBooleanExtra("open_read", false) == true) pendingOpenRead.value = true
        takeLinks(intent)
        setContent {
            BeepTheme { CompositionLocalProvider(LocalOpenLink provides { url: String -> repository.openBrowser(url) }) {
                BeepApp(repository, bubble, locationPermission = locationPermission, focusInput = pendingFocusInput, openApps = pendingOpenApps, openRead = pendingOpenRead, shareToken = pendingShareToken, groupToken = pendingGroupToken, groupToOpen = pendingGroup, chatToOpen = pendingChat, clipboardContent = clipboardContent, onFloat = {
                    if (Bubble.kind(this) == BubbleKind.System || Bubble.allowed(this) || Bubble.asked(this)) float()
                    else { Bubble.markAsked(this); pendingFloat = true; Bubble.settings(this) }
                }, onExpand = { groupId ->
                    startActivity(Intent(this, MainActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                        groupId?.let { putExtra("group_id", it) }
                    })
                    if (panel) finish()
                }, onBubbleSettings = { Bubble.settings(this) })
            } }
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
        // Measured from the top of the screen, like the bubble, not from below the status bar.
        window.attributes = window.attributes.apply { gravity = Gravity.TOP; fitInsetsTypes = 0; y = Bubble.panelTop(this@PanelActivity) }
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

internal enum class Screen { Home, Chat, Places, LocationAlways, Memories, Crons, Integrations, Settings, Devices, Shared, GroupJoin, GroupInfo }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BeepApp(repository: ChatRepository, bubble: Boolean,
                    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>? = null,
                    focusInput: MutableState<Boolean> = mutableStateOf(false),
                    openApps: MutableState<Boolean> = mutableStateOf(false),
                    openRead: MutableState<Boolean> = mutableStateOf(false),
                    shareToken: MutableState<String?> = mutableStateOf(null),
                    groupToken: MutableState<String?> = mutableStateOf(null),
                    groupToOpen: MutableState<String?> = mutableStateOf(null),
                    chatToOpen: MutableState<String?> = mutableStateOf(null),
                    clipboardContent: MutableState<String?> = mutableStateOf(null),
                    onFloat: () -> Unit, onExpand: (groupId: String?) -> Unit, onBubbleSettings: () -> Unit) {
    val state by repository.state.collectAsStateWithLifecycle()
    val config by repository.settings.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    val pendingAppIntent by repository.pendingIntent.collectAsStateWithLifecycle()
    val pendingAlarm by repository.pendingAlarm.collectAsStateWithLifecycle()
    val browserOpen by repository.browserOpen.collectAsStateWithLifecycle()
    var bubbleMenu by remember { mutableStateOf(false) }
    // The app opens on the chat list; the bubble opens on its chat.
    var screen by rememberSaveable { mutableStateOf(if (bubble) Screen.Chat else Screen.Home) }
    val places = remember(repository) { ServerPlaces(repository) }
    // Where "Not now" on the location screen goes back to.
    var beforeLocationAlways by rememberSaveable { mutableStateOf(Screen.Places) }
    // The first place routine made without location all the time opens the screen that asks for it.
    val askLocationAlways by repository.askLocationAlways.collectAsStateWithLifecycle()
    LaunchedEffect(askLocationAlways, bubble) {
        if (askLocationAlways && !bubble) {
            repository.askLocationAlways.value = false
            beforeLocationAlways = screen
            screen = Screen.LocationAlways
        }
    }
    // A notice about an app that lost its login opens Apps to reconnect it.
    LaunchedEffect(openApps.value) {
        if (openApps.value) {
            openApps.value = false
            screen = Screen.Integrations
        }
    }
    // A finished read of mail or a calendar opens Memories on what it found.
    var showLatestRead by remember { mutableStateOf(false) }
    LaunchedEffect(openRead.value) {
        if (openRead.value) {
            openRead.value = false
            showLatestRead = true
            screen = Screen.Memories
        }
    }
    // A shared chat's link opens it read only until the user goes back or continues it.
    var sharedToken by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(shareToken.value) {
        shareToken.value?.let {
            shareToken.value = null
            sharedToken = it
            screen = Screen.Shared
        }
    }
    // The group chat on screen instead of a one to one chat, if any.
    var openGroup by rememberSaveable { mutableStateOf<String?>(null) }
    var joinToken by rememberSaveable { mutableStateOf<String?>(null) }
    var newGroup by remember { mutableStateOf(false) }
    var joiningByCode by remember { mutableStateOf(false) }
    var inviting by remember { mutableStateOf<Group?>(null) }
    val groups by repository.groups.collectAsStateWithLifecycle()
    LaunchedEffect(groupToken.value) {
        groupToken.value?.let {
            groupToken.value = null
            joinToken = it
            screen = Screen.GroupJoin
        }
    }
    LaunchedEffect(groupToOpen.value) {
        groupToOpen.value?.let {
            groupToOpen.value = null
            openGroup = it; screen = Screen.Chat
        }
    }
    LaunchedEffect(chatToOpen.value) {
        chatToOpen.value?.let {
            chatToOpen.value = null
            openGroup = null; screen = Screen.Chat
        }
    }
    // Chats left by starting a new one, newest last, so a swipe right can go back to them.
    // The trail only holds while the user stays on the chat the swipes led to: opening one from the
    // list, a notification or a share starts over, and it lives no longer than this screen.
    var visited by remember { mutableStateOf(listOf<String>()) }
    var trailEnd by remember { mutableStateOf<String?>(null) }
    fun openChat(id: String) {
        visited = emptyList()
        openGroup = null
        repository.select(id)
    }
    fun startNewChat() {
        if (openGroup != null) {
            openGroup = null
            if (state.current.messages.isNotEmpty()) repository.newChat()
            return
        }
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
        val id = previousChat() ?: run { screen = Screen.Home; return }
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
    // Show onboarding permissions screen once after first connection
    val prefs = remember { context.getSharedPreferences("connection", android.content.Context.MODE_PRIVATE) }
    var onboardingDone by rememberSaveable { mutableStateOf(prefs.getBoolean("onboarding_done", false)) }
    if (!onboardingDone && !bubble) {
        OnboardingScreen(repository, locationPermission = locationPermission, onDone = {
            prefs.edit().putBoolean("onboarding_done", true).apply()
            onboardingDone = true
        })
        return
    }
    // The bubble only has room for its chat and the chat list.
    val activeScreen = if (bubble && screen != Screen.Home) Screen.Chat else screen
    BackHandler(enabled = if (bubble) activeScreen == Screen.Home else activeScreen != Screen.Home) {
        screen = when {
            bubble -> Screen.Chat
            activeScreen == Screen.Devices -> Screen.Settings
            activeScreen == Screen.LocationAlways -> beforeLocationAlways
            activeScreen == Screen.GroupInfo -> Screen.Chat
            else -> Screen.Home
        }
    }
    fun openGroupChat(id: String) {
        openGroup = id
        screen = Screen.Chat
    }
    Crossfade(targetState = activeScreen, animationSpec = tween(300), label = "screen-transition") { currentScreen ->
        when (currentScreen) {
            Screen.Memories -> MemoriesScreen(repository, latestRead = showLatestRead, onBack = { showLatestRead = false; screen = Screen.Home })
            Screen.Crons -> CronsScreen(repository, onBack = { screen = Screen.Home },
                onAskBeep = { repository.newChatWithDraft("Set up a routine: "); screen = Screen.Chat },
                onOpenChat = { id -> repository.select(id); screen = Screen.Chat })
            Screen.Integrations -> AppsScreen(repository, onBack = { screen = Screen.Home })
            Screen.Settings -> SettingsScreen(repository, onBack = { screen = Screen.Home },
                onBubbleSettings = onBubbleSettings, onLinkedDevices = { screen = Screen.Devices })
            Screen.Devices -> LinkedDevicesScreen(repository, onBack = { screen = Screen.Settings })
            Screen.GroupJoin -> joinToken?.let { token ->
                GroupJoinScreen(repository, token, onJoined = { joinToken = null; openGroupChat(it) },
                    onBack = { joinToken = null; screen = Screen.Home })
            }
            Screen.GroupInfo -> openGroup?.let { id ->
                GroupInfoScreen(repository, id, onBack = { screen = Screen.Chat }, onInvite = { inviting = it },
                    onLeft = { openGroup = null; screen = Screen.Home })
            }
            Screen.Shared -> sharedToken?.let { token ->
                SharedChatScreen(repository, token, onBack = { screen = Screen.Home }, onContinued = { sharedToken = null; screen = Screen.Chat })
            }
            Screen.Home -> ChatListScreen(repository, places, HomeActions(
                openChat = { openChat(it); screen = Screen.Chat },
                openGroup = { openGroupChat(it) },
                newChat = { openGroup = null; if (state.current.messages.isNotEmpty()) repository.newChat(); screen = Screen.Chat },
                newGroup = { if (bubble) onExpand(null) else newGroup = true },
                joinCode = { if (bubble) onExpand(null) else joiningByCode = true },
                invite = { inviting = it },
                open = { if (bubble) onExpand(null) else screen = it }))
            Screen.Places -> PlacesScreen(places, onBack = { screen = Screen.Home },
                onLocationAlways = { beforeLocationAlways = Screen.Places; screen = Screen.LocationAlways })
            Screen.LocationAlways -> BackgroundLocationScreen(onDone = { screen = beforeLocationAlways }, onPlaces = { screen = Screen.Places })
            Screen.Chat -> {
        val group = openGroup
        // The bubble has no room for the group screen, so its info opens the app.
        if (group != null) GroupChatPane(repository, group, onBack = { screen = Screen.Home }, onInfo = { if (bubble) onExpand(group) else screen = Screen.GroupInfo },
            onInvite = { inviting = it }, onGone = { openGroup = null },
            onNewChat = { startNewChat() }, onPrevious = { openGroup = null },
            onExpand = if (bubble) { { onExpand(group) } } else null)
        else Scaffold(containerColor = Ink, topBar = {
            if (bubble) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(displayText("Beep"), style = displayStyle(18.sp))
                    Spacer(Modifier.width(8.dp))
                    ConnectionDot(connected, status)
                    Spacer(Modifier.weight(1f))
                    Box {
                        IconButton(onClick = { bubbleMenu = true }) { Icon(Icons.Outlined.MoreVert, "Chat options", Modifier.size(20.dp)) }
                        DropdownMenu(expanded = bubbleMenu, onDismissRequest = { bubbleMenu = false }) {
                            DropdownMenuItem(text = { Text("New conversation") }, onClick = { startNewChat(); bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Conversations") }, onClick = { screen = Screen.Home; bubbleMenu = false })
                        }
                    }
                }
            } else TopAppBar(title = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    GroupAvatar("b", Beep.colors.brand, 36.dp, beep = true)
                    Column {
                        Text(state.current.title.ifEmpty { "Beep" }, fontWeight = FontWeight.SemiBold, fontSize = 16.sp,
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ConnectionDot(connected, status)
                            Spacer(Modifier.width(6.dp))
                            if (!connected) Text("Tap to reconnect", fontSize = 12.sp, color = TextMuted,
                                fontFamily = Beep.fonts.body, fontWeight = FontWeight.Medium,
                                maxLines = 1, overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.clickable { repository.reconnect() })
                            else Text(status, fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }, navigationIcon = {
                IconButton(onClick = { screen = Screen.Home }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Chats") }
            },
                actions = {
                    IconButton(onClick = onFloat, Modifier.padding(end = 6.dp)) { Icon(Icons.Outlined.BubbleChart, "Float chat", tint = TextMuted) }
                }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
        }) { padding ->
            SwipeChats(canStartNew = state.current.messages.isNotEmpty(), hasPrevious = previousChat() != null,
                onNew = { startNewChat() }, onBack = { goBack() }, modifier = Modifier.padding(padding).fillMaxSize()) {
                // Each chat gets its own list, so rows of the chat being left never animate under the next one.
                key(state.current.id) {
                    ChatScreen(state.current, repository, Modifier.fillMaxSize(), compact = bubble, focusInput = focusInput, clipboardContent = clipboardContent,
                        onExpand = if (bubble) { { onExpand(null) } } else null)
                }
            }
        }
    } // end Screen.Chat
    } // end when
    } // end Crossfade
    if (newGroup) NewGroupSheet(repository, onDismiss = { newGroup = false }, onCreated = {
        newGroup = false
        openGroupChat(it.id)
        inviting = it
    }, onJoinCode = { newGroup = false; joiningByCode = true })
    if (joiningByCode) JoinCodeSheet(repository, onDismiss = { joiningByCode = false }, onFound = {
        joiningByCode = false
        joinToken = it
        screen = Screen.GroupJoin
    })
    inviting?.let { invite ->
        GroupInviteSheet(repository, invite.id, invite.name, onDismiss = { inviting = null })
    }
    // Browser overlay
    AnimatedVisibility(visible = browserOpen,
        enter = fadeIn() + slideInVertically { it },
        exit = fadeOut() + slideOutVertically { it }) {
        BrowserScreen(onClose = { repository.closeBrowser() }) { modifier ->
            ChatScreen(state.current, repository, modifier, compact = true)
        }
    }
}
