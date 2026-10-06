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
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
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

private enum class Screen { Chat, Memories, Crons, Integrations, Settings }

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
        val accountName by repository.accountName.collectAsState()
        ConnectionScreen(config, accountName, canGoBack = config.configured, onBack = { settingsOpen = false }, onConnect = { settings, identity ->
            withContext(Dispatchers.IO) { repository.connect(settings, identity) }; settingsOpen = false
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
        screen = Screen.Chat
    }
    Crossfade(targetState = activeScreen, animationSpec = tween(300), label = "screen-transition") { currentScreen ->
        when (currentScreen) {
            Screen.Memories -> MemoriesScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Crons -> CronsScreen(repository, onBack = { screen = Screen.Chat },
                onAskBeep = { repository.newChatWithDraft("Set up a routine: "); screen = Screen.Chat },
                onOpenChat = { id -> repository.select(id); screen = Screen.Chat })
            Screen.Integrations -> AppsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Settings -> SettingsScreen(status, repository, onBack = { screen = Screen.Chat },
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
                            if (selecting != null) Icon(if (chat.id in selecting) Icons.Outlined.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                                null, Modifier.size(18.dp), tint = if (chat.id in selecting) Coral else TextMuted)
                            Text(chat.title, maxLines = 2, fontSize = 14.sp, overflow = TextOverflow.Ellipsis,
                                color = if (highlighted) Beep.colors.onUser else Color.Unspecified, modifier = Modifier.weight(1f))
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
            if (confirmBulkDelete) {
                val ids = picked.orEmpty()
                AlertDialog(
                    onDismissRequest = { confirmBulkDelete = false },
                    confirmButton = { TextButton(onClick = {
                        confirmBulkDelete = false; picked = null
                        repository.deleteSessions(ids)
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
