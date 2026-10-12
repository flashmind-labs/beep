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
import androidx.compose.ui.platform.LocalUriHandler
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
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Logout
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
import androidx.compose.foundation.layout.WindowInsets as ComposeInsets
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
        // The repository keeps the lambda for the life of the process, so it holds the application, not this activity.
        val app = applicationContext
        val locationManager = app.getSystemService(LocationManager::class.java) ?: return
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) != PackageManager.PERMISSION_GRANTED) return
        val provider = when {
            Build.VERSION.SDK_INT >= 31 && locationManager.isProviderEnabled(LocationManager.FUSED_PROVIDER) -> LocationManager.FUSED_PROVIDER
            locationManager.isProviderEnabled(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
            locationManager.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
            else -> return
        }
        val places = PlaceNames(app)
        // No listener: the last known fix is read only when a report is due.
        repository.startLocationUpdates {
            val loc = try {
                locationManager.getLastKnownLocation(provider)
                    ?: locationManager.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?: locationManager.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            } catch (_: Exception) { null }
            loc?.let { ChatRepository.Fix(it.latitude, it.longitude, it.accuracy, places.name(it.latitude, it.longitude)) }
        }
    }
    /** A notice about an app, or the `beep://apps` link Composio returns to after sign-in. */
    private fun opensApps(intent: Intent?): Boolean =
        intent?.getBooleanExtra("open_apps", false) == true ||
            (intent?.data?.scheme == "beep" && intent.data?.host == "apps")

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        takeIntent(intent)
    }
    /** Picks up what [intent] asks to open: a chat, a group, a link or a screen. */
    private fun takeIntent(intent: Intent?) {
        intent ?: return
        intent.getStringExtra("conversation_id")?.let { (application as BeepApplication).repository.select(it); pendingChat.value = it }
        if (intent.getBooleanExtra("focus_input", false)) pendingFocusInput.value = true
        if (opensApps(intent)) pendingOpenApps.value = true
        if (intent.getBooleanExtra("open_read", false)) pendingOpenRead.value = true
        intent.data?.let { shareToken(it.toString()) }?.let { pendingShareToken.value = it }
        intent.data?.let { groupToken(it.toString()) }?.let { pendingGroupToken.value = it }
        intent.getStringExtra("group_id")?.let { pendingGroup.value = it }
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
        takeIntent(intent)
        setContent {
            BeepTheme { CompositionLocalProvider(LocalUriHandler provides OutsideUriHandler(this), LocalOpenLink provides { url: String ->
                // The bubble has no room for the browser, so its links go to the phone's own.
                if (bubble) ExternalBrowser.open(this, url) else repository.openBrowser(url)
            }) {
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
        (application as BeepApplication).repository.chatScreenShown(true, bubble)
    }
    override fun onStop() {
        super.onStop()
        if (!bubble) Bubble.appShown(false)
        (application as BeepApplication).repository.chatScreenShown(false, bubble)
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
    override fun onStart() {
        // The bubble's intent is only read when it first opens, so a reply in another chat is picked up here.
        ChatNotifications.announced?.let { (application as BeepApplication).repository.select(it); ChatNotifications.announced = null }
        super.onStart()
    }
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

internal enum class Screen { Chat, Places, LocationAlways, Memories, Crons, Skills, Integrations, Settings, Devices, Shared, GroupJoin, GroupInfo }

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun BeepApp(repository: ChatRepository, bubble: Boolean,
                    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>?,
                    focusInput: MutableState<Boolean>,
                    openApps: MutableState<Boolean>,
                    openRead: MutableState<Boolean>,
                    shareToken: MutableState<String?>,
                    groupToken: MutableState<String?>,
                    groupToOpen: MutableState<String?>,
                    chatToOpen: MutableState<String?>,
                    clipboardContent: MutableState<String?>,
                    onFloat: () -> Unit, onExpand: (groupId: String?) -> Unit, onBubbleSettings: () -> Unit) {
    val state by repository.state.collectAsStateWithLifecycle()
    val config by repository.settings.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    val pendingAppIntent by repository.pendingIntent.collectAsStateWithLifecycle()
    val pendingAlarm by repository.pendingAlarm.collectAsStateWithLifecycle()
    val browserOpen by repository.browserOpen.collectAsStateWithLifecycle()
    var drawerOpen by remember { mutableStateOf(false) }
    var bubbleMenu by remember { mutableStateOf(false) }
    // The outside screen of a flip phone, about 350 dp square, gets the bubble's slim chat.
    val cover = !bubble && LocalConfiguration.current.let { it.screenWidthDp < 480 && it.screenHeightDp < 480 }
    var historyQuery by rememberSaveable { mutableStateOf("") }
    var screen by rememberSaveable { mutableStateOf(Screen.Chat) }
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
    val activeScreen = if (bubble) Screen.Chat else screen
    BackHandler(enabled = activeScreen != Screen.Chat) {
        screen = when (activeScreen) {
            Screen.Devices -> Screen.Settings
            Screen.LocationAlways -> Screen.Places
            else -> Screen.Chat
        }
    }
    fun openGroupChat(id: String) {
        openGroup = id
        screen = Screen.Chat
    }
    Crossfade(targetState = activeScreen, animationSpec = tween(300), label = "screen-transition") { currentScreen ->
        when (currentScreen) {
            Screen.Memories -> MemoriesScreen(repository, latestRead = showLatestRead, onBack = { showLatestRead = false; screen = Screen.Chat })
            Screen.Crons -> CronsScreen(repository, onBack = { screen = Screen.Chat },
                onAskBeep = { repository.newChatWithDraft("Set up a routine: "); screen = Screen.Chat },
                onOpenChat = { id -> repository.select(id); screen = Screen.Chat })
            Screen.Skills -> SkillsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Integrations -> AppsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Settings -> SettingsScreen(repository, onBack = { screen = Screen.Chat },
                onBubbleSettings = onBubbleSettings, onLinkedDevices = { screen = Screen.Devices })
            Screen.Devices -> LinkedDevicesScreen(repository, onBack = { screen = Screen.Settings })
            Screen.GroupJoin -> joinToken?.let { token ->
                GroupJoinScreen(repository, token, onJoined = { joinToken = null; openGroupChat(it) },
                    onBack = { joinToken = null; screen = Screen.Chat })
            }
            Screen.GroupInfo -> openGroup?.let { id ->
                GroupInfoScreen(repository, id, onBack = { screen = Screen.Chat }, onInvite = { inviting = it },
                    onLeft = { openGroup = null; screen = Screen.Chat })
            }
            Screen.Shared -> sharedToken?.let { token ->
                SharedChatScreen(repository, token, onBack = { screen = Screen.Chat }, onContinued = { sharedToken = null; screen = Screen.Chat })
            }
            Screen.Places -> PlacesScreen(places, onBack = { screen = Screen.Chat },
                onLocationAlways = { beforeLocationAlways = Screen.Places; screen = Screen.LocationAlways })
            Screen.LocationAlways -> BackgroundLocationScreen(onDone = { screen = beforeLocationAlways }, onPlaces = { screen = Screen.Places })
            Screen.Chat -> {
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(drawerOpen) {
        if (drawerOpen) {
            // The composer keeps focus behind the drawer otherwise, and the keyboard covers the list.
            focusManager.clearFocus()
            keyboard?.hide()
            repository.refreshHistory(); repository.refreshGroups(); drawerState.open()
        } else drawerState.close()
    }
    LaunchedEffect(drawerState.currentValue) { drawerOpen = drawerState.currentValue == DrawerValue.Open }
    // Unfolding resizes the window without a restart, and the drawer keeps the closed offset of the
    // narrower sheet, so it shows half open. Snap it again once the new width is laid out.
    val screenWidth = LocalConfiguration.current.screenWidthDp
    LaunchedEffect(screenWidth) { withFrameNanos {}; drawerState.snapTo(drawerState.currentValue) }
    BackHandler(enabled = drawerOpen) { drawerOpen = false }
    // Closed, horizontal swipes belong to the chat (new chat or back), so the drawer only takes them to close.
    ModalNavigationDrawer(drawerState = drawerState, gesturesEnabled = drawerOpen, drawerContent = {
        ModalDrawerSheet(modifier = Modifier.widthIn(max = 480.dp).fillMaxWidth(0.9f), drawerContainerColor = Ink,
            drawerShape = RoundedCornerShape(topEnd = Beep.shapes.radius, bottomEnd = Beep.shapes.radius)) {
            Spacer(Modifier.height(if (bubble) 8.dp else 20.dp))
            Row(Modifier.padding(start = 20.dp, end = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(displayText("Your chats"), Modifier.weight(1f), style = displayStyle(26.sp))
                if (!bubble) ChunkyButton(onClick = { newGroup = true }, shape = RoundedCornerShape(Beep.shapes.smallRadius),
                    modifier = Modifier.size(40.dp), contentPadding = PaddingValues(0.dp)) {
                    Icon(Icons.Outlined.GroupAdd, "New group", Modifier.size(20.dp))
                }
            }
            OutlinedTextField(historyQuery, { historyQuery = it }, placeholder = { Text("Find a conversation", fontSize = 13.sp) },
                leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Surface, focusedContainerColor = Surface,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss),
                shape = RoundedCornerShape(Beep.shapes.radius), modifier = Modifier.padding(horizontal = 20.dp, vertical = 14.dp).fillMaxWidth())
            var contextMenuChat by remember { mutableStateOf<String?>(null) }
            var contextMenuGroup by remember { mutableStateOf<String?>(null) }
            var leavingGroup by remember { mutableStateOf<Group?>(null) }
            var showRenameDialog by remember { mutableStateOf<Conversation?>(null) }
            var showDeleteDialog by remember { mutableStateOf<Conversation?>(null) }
            var sharingChat by remember { mutableStateOf<Conversation?>(null) }
            // Chats and groups (keyed by groupKey) picked for bulk delete; selection mode is on while it is not null.
            var picked by remember { mutableStateOf<Set<String>?>(null) }
            var confirmBulkDelete by remember { mutableStateOf(false) }
            LaunchedEffect(drawerOpen) { if (!drawerOpen) picked = null }
            BackHandler(enabled = picked != null) { picked = null }
            val visibleChats = state.conversations.filter { !it.isUnused && it.title.contains(historyQuery, ignoreCase = true) }
                .sortedWith(compareByDescending<Conversation> { it.pinned }.thenByDescending { it.lastActive })
            // Titles match at once; what was written comes from the server after a pause in typing.
            var contentHits by remember { mutableStateOf<List<SessionMatch>>(emptyList()) }
            LaunchedEffect(historyQuery) {
                contentHits = emptyList()
                val query = historyQuery.trim()
                if (query.length < 2) return@LaunchedEffect
                delay(350)
                contentHits = runCatching { withContext(Dispatchers.IO) { repository.searchChats(query) } }.getOrDefault(emptyList())
            }
            // Groups sit among the chats by when they were last active, pinned ones with the pinned chats.
            val visibleGroups = groups.filter { it.name.contains(historyQuery, ignoreCase = true) }
            val rows: List<DrawerRow> = run {
                val all = (visibleChats.map(DrawerRow::Chat) + visibleGroups.map(DrawerRow::GroupRow)).sortedByDescending { it.lastActive }
                val (pinned, rest) = all.partition { it.pinned }
                pinned + rest
            }
            val known = state.conversations.associateBy { it.id }
            val titled = visibleChats.map { it.id }.toSet()
            val messageHits = if (picked != null) emptyList()
                else contentHits.filter { it.session.id !in titled && it.session.id in known }
            picked?.let { ids ->
                Row(Modifier.padding(start = 20.dp, end = 8.dp, bottom = 4.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text("${ids.size} selected", Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                    val all = visibleChats.map { it.id }.toSet() + visibleGroups.map { groupKey(it.id) }
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
                if (messageHits.isNotEmpty() && visibleChats.isNotEmpty()) item(key = "titles-label") { DrawerLabel("Titles") }
                items(rows, key = { it.key }) { drawerRow ->
                    if (drawerRow is DrawerRow.GroupRow) Box {
                        val row = drawerRow.group
                        val selecting = picked
                        val key = groupKey(row.id)
                        val toggle = { selecting?.let { picked = if (key in it) it - key else it + key } }
                        GroupRow(row, highlighted = if (selecting != null) key in selecting else openGroup == row.id,
                            selected = selecting?.let { key in it },
                            onClick = { if (selecting != null) toggle() else { openGroupChat(row.id); drawerOpen = false } },
                            onLongClick = { if (selecting != null) toggle() else contextMenuGroup = row.id })
                        DropdownMenu(expanded = contextMenuGroup == row.id, onDismissRequest = { contextMenuGroup = null },
                            containerColor = Surface) {
                            DropdownMenuItem(text = { Text("Select") },
                                leadingIcon = { Icon(Icons.Outlined.CheckCircle, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuGroup = null; picked = setOf(key) })
                            DropdownMenuItem(text = { Text(if (row.pinned) "Unpin" else "Pin") },
                                leadingIcon = { Icon(Icons.Outlined.PushPin, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuGroup = null; repository.pinGroup(row.id, !row.pinned) })
                            DropdownMenuItem(text = { Text("Invite") },
                                leadingIcon = { Icon(Icons.Outlined.Link, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuGroup = null; inviting = row })
                            DropdownMenuItem(text = { Text("Leave", color = Coral) },
                                leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Logout, null, Modifier.size(18.dp), tint = Coral) },
                                onClick = { contextMenuGroup = null; leavingGroup = row })
                        }
                        return@items
                    }
                    val chat = (drawerRow as DrawerRow.Chat).chat
                    Box(Modifier.padding(horizontal = 12.dp, vertical = 2.dp)) {
                        val selecting = picked
                        val toggle = { selecting?.let { picked = if (chat.id in it) it - chat.id else it + chat.id } }
                        val highlighted = if (selecting != null) chat.id in selecting else openGroup == null && state.current.id == chat.id
                        // NavigationDrawerItem consumes the press itself, so it never reports a long press.
                        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius))
                            .background(if (highlighted) Beep.colors.user else Color.Transparent)
                            .combinedClickable(
                                onClick = { if (selecting != null) toggle() else { openChat(chat.id); drawerOpen = false } },
                                onLongClick = { if (selecting != null) toggle() else contextMenuChat = chat.id })
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            if (selecting != null) Icon(if (chat.id in selecting) Icons.Outlined.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                                null, Modifier.size(18.dp), tint = if (chat.id in selecting) Coral else TextMuted)
                            Column(Modifier.weight(1f)) {
                                Text(chat.title, maxLines = 2, fontSize = 14.sp, overflow = TextOverflow.Ellipsis,
                                    color = if (highlighted) Beep.colors.onUser else Color.Unspecified)
                                Text(chatTime(chat.lastActive), fontSize = 12.sp, maxLines = 1,
                                    color = if (highlighted) Beep.colors.onUser.copy(alpha = 0.7f) else TextMuted)
                            }
                            if (chat.pinned && selecting == null) Icon(Icons.Outlined.PushPin, "Pinned", Modifier.size(16.dp), tint = Moss)
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
                                onClick = { contextMenuChat = null; repository.pinSession(chat.id, !chat.pinned) })
                            DropdownMenuItem(text = { Text("Share") },
                                leadingIcon = { Icon(Icons.Outlined.Share, null, Modifier.size(18.dp)) },
                                onClick = { contextMenuChat = null; sharingChat = chat })
                            DropdownMenuItem(text = { Text("Delete", color = Coral) },
                                leadingIcon = { Icon(Icons.Outlined.Delete, null, Modifier.size(18.dp), tint = Coral) },
                                onClick = { contextMenuChat = null; showDeleteDialog = chat })
                        }
                    }
                }
                if (messageHits.isNotEmpty()) item(key = "messages-label") { DrawerLabel("In messages") }
                items(messageHits, key = { "hit-" + it.session.id }) { hit ->
                    val chat = known.getValue(hit.session.id)
                    MessageHitRow(chat.title, hit, historyQuery.trim()) { openChat(chat.id); drawerOpen = false }
                }
            }
            sharingChat?.let { chat -> ShareSheet(repository, chat, onDismiss = { sharingChat = null }) }
            // Rename dialog
            showRenameDialog?.let { chat ->
                var renameText by remember(chat.id) { mutableStateOf(chat.title) }
                AlertDialog(
                    onDismissRequest = { showRenameDialog = null },
                    confirmButton = { TextButton(onClick = {
                        val title = renameText.trim()
                        if (title.isNotEmpty()) { repository.renameSession(chat.id, title); showRenameDialog = null }
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
                        repository.deleteSession(chat.id); showDeleteDialog = null
                    }) { Text("Delete", color = Coral) } },
                    dismissButton = { TextButton(onClick = { showDeleteDialog = null }) { Text("Cancel") } },
                    title = { Text("Delete conversation?") },
                    text = { Text("This will permanently delete this conversation and all its messages.") },
                    containerColor = Surface)
            }
            leavingGroup?.let { g ->
                AlertDialog(
                    onDismissRequest = { leavingGroup = null },
                    confirmButton = { TextButton(onClick = {
                        leavingGroup = null
                        if (openGroup == g.id) openGroup = null
                        repository.leaveGroups(setOf(g.id))
                    }) { Text("Leave", color = Coral) } },
                    dismissButton = { TextButton(onClick = { leavingGroup = null }) { Text("Cancel") } },
                    title = { Text("Leave ${g.name}?") },
                    text = { Text(if (g.members.size <= 1) "You are the last member, so the group and its messages are deleted."
                        else "You stop getting its messages. Someone in it can invite you again.") },
                    containerColor = Surface)
            }
            if (confirmBulkDelete) {
                val (groupKeys, chatIds) = picked.orEmpty().partition { it.startsWith(GROUP_KEY) }
                val groupIds = groupKeys.map { it.removePrefix(GROUP_KEY) }.toSet()
                val chats = when (chatIds.size) { 0 -> null; 1 -> "1 conversation"; else -> "${chatIds.size} conversations" }
                val leave = when (groupIds.size) { 0 -> null; 1 -> "1 group"; else -> "${groupIds.size} groups" }
                AlertDialog(
                    onDismissRequest = { confirmBulkDelete = false },
                    confirmButton = { TextButton(onClick = {
                        confirmBulkDelete = false; picked = null
                        repository.deleteSessions(chatIds.toSet())
                        if (openGroup in groupIds) openGroup = null
                        repository.leaveGroups(groupIds)
                    }) { Text(if (chats == null) "Leave" else "Delete", color = Coral) } },
                    dismissButton = { TextButton(onClick = { confirmBulkDelete = false }) { Text("Cancel") } },
                    title = { Text(listOfNotNull(chats?.let { "Delete $it" }, leave?.let { "leave $it" })
                        .joinToString(" and ").replaceFirstChar { it.uppercase() } + "?") },
                    text = { Text(listOfNotNull(chats?.let { "The conversations and their messages are deleted for good." },
                        leave?.let { "You stop getting the groups' messages." }).joinToString(" ")) },
                    containerColor = Surface)
            }
            if (!bubble && !cover) {
                HorizontalDivider(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), color = Border)
                Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    DrawerTile("Memories", Icons.Outlined.Psychology, Beep.colors.tints[0], Modifier.weight(1f)) { screen = Screen.Memories; drawerOpen = false }
                    DrawerTile("Routines", Icons.Outlined.Schedule, Beep.colors.tints[1], Modifier.weight(1f)) { screen = Screen.Crons; drawerOpen = false }
                    DrawerTile("Skills", Icons.Outlined.AutoAwesome, Beep.colors.tints[2], Modifier.weight(1f)) { screen = Screen.Skills; drawerOpen = false }
                }
                Spacer(Modifier.height(8.dp))
                Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    DrawerTile("Places", Icons.Outlined.Place, Beep.colors.tints[1], Modifier.weight(1f)) { screen = Screen.Places; drawerOpen = false }
                    DrawerTile("Apps", Icons.Outlined.Extension, Beep.colors.tints[0], Modifier.weight(1f)) { screen = Screen.Integrations; drawerOpen = false }
                    Spacer(Modifier.weight(1f))
                }
                Spacer(Modifier.height(8.dp))
            }
            Row(Modifier.padding(horizontal = 10.dp).padding(bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                if (!bubble) TextButton(onClick = { screen = Screen.Settings; drawerOpen = false }) {
                    Icon(Icons.Outlined.Settings, null, Modifier.size(18.dp), tint = TextMuted); Spacer(Modifier.width(8.dp))
                    Text("Settings", color = TextMuted)
                }
                Spacer(Modifier.weight(1f))
                ConnectionDot(connected, status); Spacer(Modifier.width(6.dp))
                Text(status, fontSize = 11.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(end = 12.dp).widthIn(max = 140.dp))
            }
        }
    }) {
        val group = openGroup
        // The bubble has no room for the group screen, so its info opens the app.
        if (group != null) GroupChatPane(repository, group, onMenu = { drawerOpen = true }, onInfo = { if (bubble) onExpand(group) else screen = Screen.GroupInfo },
            onInvite = { inviting = it }, onGone = { openGroup = null },
            onNewChat = { startNewChat() }, onPrevious = { openGroup = null },
            onExpand = if (bubble) { { onExpand(group) } } else null)
        else Scaffold(containerColor = Ink, topBar = {
            // With the keyboard up the cover screen has room for the input alone, so the header steps aside.
            if (bubble || cover && !ComposeInsets.isImeVisible) {
                Row(Modifier.fillMaxWidth().then(if (cover) Modifier.windowInsetsPadding(ComposeInsets.safeDrawing
                    .only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)) else Modifier)
                    .height(48.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(displayText("Beep"), style = displayStyle(18.sp))
                    Spacer(Modifier.width(8.dp))
                    ConnectionDot(connected, status)
                    Spacer(Modifier.weight(1f))
                    Box {
                        IconButton(onClick = { bubbleMenu = true }) { Icon(Icons.Outlined.MoreVert, "Chat options", Modifier.size(20.dp)) }
                        DropdownMenu(expanded = bubbleMenu, onDismissRequest = { bubbleMenu = false }) {
                            DropdownMenuItem(text = { Text("New conversation") }, onClick = { startNewChat(); bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Conversations") }, onClick = { drawerOpen = true; bubbleMenu = false })
                        }
                    }
                }
            } else TopAppBar(title = {
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
                    // The cover screen's cameras cut into its bottom edge, so the input sits above them.
                    ChatScreen(state.current, repository, Modifier.fillMaxSize().then(if (cover) Modifier.consumeWindowInsets(padding)
                        .windowInsetsPadding(ComposeInsets.safeDrawing.only(WindowInsetsSides.Bottom)) else Modifier), compact = bubble || cover, focusInput = focusInput, clipboardContent = clipboardContent,
                        onExpand = if (bubble) { { onExpand(null) } } else null)
                }
            }
        }
    } // end ModalNavigationDrawer
    } // end Screen.Chat
    } // end when
    } // end Crossfade
    if (newGroup) NewGroupSheet(repository, onDismiss = { newGroup = false }, onCreated = {
        newGroup = false
        drawerOpen = false
        openGroupChat(it.id)
        inviting = it
    }, onJoinCode = { newGroup = false; joiningByCode = true })
    if (joiningByCode) JoinCodeSheet(repository, onDismiss = { joiningByCode = false }, onFound = {
        joiningByCode = false
        drawerOpen = false
        joinToken = it
        screen = Screen.GroupJoin
    })
    inviting?.let { invite ->
        GroupInviteSheet(repository, invite.id, invite.name, onDismiss = { inviting = null })
    }
    // Browser overlay, never in the bubble
    AnimatedVisibility(visible = browserOpen && !bubble,
        enter = fadeIn() + slideInVertically { it },
        exit = fadeOut() + slideOutVertically { it }) {
        BrowserScreen(onClose = { repository.closeBrowser() }) { modifier ->
            ChatScreen(state.current, repository, modifier, compact = true)
        }
    }
}

@Composable
private fun DrawerLabel(text: String) =
    Text(text, Modifier.padding(start = 28.dp, end = 20.dp, top = 10.dp, bottom = 4.dp),
        fontSize = 12.sp, fontWeight = FontWeight.SemiBold, color = TextMuted)

/** A chat found by what was written in it, with the matching words highlighted. */
@Composable
private fun MessageHitRow(title: String, hit: SessionMatch, query: String, onClick: () -> Unit) {
    val highlight = Beep.colors.brand.copy(alpha = 0.25f)
    val snippet = buildAnnotatedString {
        if (hit.fromUser) append("You: ")
        var from = 0
        while (query.isNotEmpty()) {
            val at = hit.snippet.indexOf(query, from, ignoreCase = true)
            if (at < 0) break
            append(hit.snippet.substring(from, at))
            withStyle(SpanStyle(background = highlight, color = Beep.colors.ink)) { append(hit.snippet.substring(at, at + query.length)) }
            from = at + query.length
        }
        append(hit.snippet.substring(from))
    }
    Column(Modifier.padding(horizontal = 12.dp, vertical = 2.dp).fillMaxWidth().clip(RoundedCornerShape(Beep.shapes.smallRadius))
        .clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(title, maxLines = 1, fontSize = 14.sp, overflow = TextOverflow.Ellipsis)
        Text(snippet, maxLines = 2, fontSize = 12.sp, lineHeight = 16.sp, overflow = TextOverflow.Ellipsis, color = TextMuted)
    }
}

/** A chat or a group in the drawer, which lists both by when they were last active. */
private sealed interface DrawerRow {
    val key: String
    val lastActive: Long
    val pinned: Boolean
    data class Chat(val chat: Conversation) : DrawerRow {
        override val key get() = chat.id
        override val lastActive get() = chat.lastActive
        override val pinned get() = chat.pinned
    }
    data class GroupRow(val group: Group) : DrawerRow {
        override val key get() = groupKey(group.id)
        override val lastActive get() = group.updatedAt
        override val pinned get() = group.pinned
    }
}

/** "Shinjuku, Tokyo, Japan" from the phone's geocoder, reused within about 100 m. */
private class PlaceNames(private val context: Context) {
    private var last: Pair<String, String?>? = null

    @Suppress("DEPRECATION")
    suspend fun name(latitude: Double, longitude: Double): String? {
        val key = "%.3f,%.3f".format(java.util.Locale.ROOT, latitude, longitude)
        last?.takeIf { it.first == key }?.let { return it.second }
        if (!Geocoder.isPresent()) return null
        val address = withContext(Dispatchers.IO) {
            runCatching { Geocoder(context).getFromLocation(latitude, longitude, 1)?.firstOrNull() }.getOrNull()
        } ?: return null
        val name = listOfNotNull(address.locality ?: address.subAdminArea ?: address.subLocality, address.adminArea, address.countryName)
            .filter { it.isNotBlank() }.distinct().joinToString(", ").ifEmpty { null }
        last = key to name
        return name
    }
}

/** A group's key in the drawer list and in the picked set, apart from chat ids. */
private const val GROUP_KEY = "group-"
private fun groupKey(id: String) = GROUP_KEY + id
