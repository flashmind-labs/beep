package com.flashmind.beep

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.content.pm.PackageManager
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
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
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
import android.util.Base64
import androidx.compose.foundation.Image
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import android.provider.Settings as SystemSettings
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

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

private val Ink = Color(0xFF0A0E0C)
private val Surface = Color(0xFF121916)
private val Elevated = Color(0xFF1A231E)
private val Border = Color(0xFF243028)
private val Moss = Color(0xFF3B9B7A)
private val Coral = Color(0xFFF07A6A)
private val TextPrimary = Color(0xFFE4EAE2)
private val TextMuted = Color(0xFF7A8A80)
private val TextDim = Color(0xFF4A5A50)

/** Load, resize to at most [maxSize] px on the longest side, and JPEG-encode as a base64 data URI. */
private fun encodeImageUri(context: Context, uri: Uri, maxSize: Int = 1024): String? {
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
private fun decodeImageBase64(dataUri: String): Bitmap? {
    return try {
        val b64 = dataUri.substringAfter(",", "")
        if (b64.isEmpty()) return null
        val bytes = Base64.decode(b64, Base64.DEFAULT)
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
    } catch (_: Exception) { null }
}

/** Returns true when the system "Remove animations" / "Animator duration scale = 0" setting is on. */
@Composable
private fun reduceMotion(): Boolean {
    val context = androidx.compose.ui.platform.LocalContext.current
    return remember(context) {
        try {
            SystemSettings.Global.getFloat(context.contentResolver, SystemSettings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
        } catch (_: Exception) { false }
    }
}

open class MainActivity : ComponentActivity() {
    protected open val bubble = false
    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (it) { Bubbles.show(this); moveTaskToBack(true) } else Bubbles.settings(this)
    }
    private val locationPermission = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { grants ->
        if (grants.values.any { it }) startLocationUpdates()
    }
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private val pendingFocusInput = mutableStateOf(false)
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
            locationManager.requestLocationUpdates(provider, 5 * 60 * 1000L, 50f, object : LocationListener {
                override fun onLocationChanged(location: Location) {}
            })
        } catch (_: Exception) {}
        repository.startLocationUpdates {
            try {
                val loc = locationManager.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                    ?: locationManager.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
                if (loc != null) Triple(loc.latitude, loc.longitude, loc.accuracy) else null
            } catch (_: Exception) { null }
        }
    }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra("conversation_id")?.let { (application as BeepApplication).repository.select(it) }
        if (intent.getBooleanExtra("focus_input", false)) pendingFocusInput.value = true
    }
    override fun onResume() {
        super.onResume()
        val repository = (application as BeepApplication).repository
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
        enableEdgeToEdge()
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
        setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Moss, onPrimary = Ink,
                background = Ink, surface = Surface, onSurface = TextPrimary, secondaryContainer = Elevated,
                surfaceVariant = Elevated, onSurfaceVariant = TextMuted,
                secondary = Moss, onSecondaryContainer = TextPrimary, outline = Border,
                surfaceContainer = Surface, surfaceContainerHigh = Elevated,
                inverseSurface = TextPrimary, inverseOnSurface = Ink)) {
                BeepApp(repository, bubble, locationPermission = locationPermission, focusInput = pendingFocusInput, clipboardContent = clipboardContent, onFloat = {
                    if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    } else if (Bubbles.show(this)) {
                        if (!bubble) moveTaskToBack(true)
                    } else Bubbles.settings(this)
                }, onExpand = {
                    startActivity(Intent(this, MainActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                    })
                }, onBubbleSettings = { Bubbles.settings(this) })
            }
        }
    }
}
class BubbleActivity : MainActivity() { override val bubble = true }

private enum class Screen { Chat, Memories, Crons, Integrations }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BeepApp(repository: ChatRepository, bubble: Boolean,
                    locationPermission: androidx.activity.result.ActivityResultLauncher<Array<String>>? = null,
                    focusInput: MutableState<Boolean> = mutableStateOf(false),
                    clipboardContent: MutableState<String?> = mutableStateOf(null),
                    onFloat: () -> Unit, onExpand: () -> Unit, onBubbleSettings: () -> Unit) {
    val state by repository.state.collectAsStateWithLifecycle()
    val config by repository.settings.collectAsStateWithLifecycle()
    val status by repository.connection.collectAsStateWithLifecycle()
    val connected by repository.connected.collectAsStateWithLifecycle()
    val pendingAppIntent by repository.pendingIntent.collectAsStateWithLifecycle()
    val browserUrlState by repository.browserUrl.collectAsStateWithLifecycle()
    var settingsOpen by rememberSaveable { mutableStateOf(false) }
    var drawerOpen by remember { mutableStateOf(false) }
    var bubbleMenu by remember { mutableStateOf(false) }
    var historyQuery by rememberSaveable { mutableStateOf("") }
    var screen by rememberSaveable { mutableStateOf(Screen.Chat) }
    val context = androidx.compose.ui.platform.LocalContext.current
    // Handle pending app launch intents
    LaunchedEffect(pendingAppIntent) {
        pendingAppIntent?.let { intent ->
            AppLauncher.launch(context, intent.app, intent.uri, intent.fallbackUrl)
            repository.pendingIntent.value = null
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
    Crossfade(targetState = activeScreen, animationSpec = tween(300), label = "screen-transition") { currentScreen ->
        when (currentScreen) {
            Screen.Memories -> MemoriesScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Crons -> CronsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Integrations -> IntegrationsScreen(repository, onBack = { screen = Screen.Chat })
            Screen.Chat -> {
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    LaunchedEffect(drawerOpen) { if (drawerOpen) { repository.refreshHistory(); drawerState.open() } else drawerState.close() }
    LaunchedEffect(drawerState.currentValue) { drawerOpen = drawerState.currentValue == DrawerValue.Open }
    ModalNavigationDrawer(drawerState = drawerState, drawerContent = {
        ModalDrawerSheet(modifier = Modifier.widthIn(max = 320.dp).fillMaxWidth(0.9f), drawerContainerColor = Surface) {
            Spacer(Modifier.height(if (bubble) 8.dp else 20.dp))
            Text("Your conversations", Modifier.padding(horizontal = 20.dp, vertical = 12.dp), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            FilledTonalButton(onClick = { repository.newChat(); drawerOpen = false }, modifier = Modifier.padding(horizontal = 24.dp).fillMaxWidth()) {
                Icon(Icons.Outlined.Add, null); Spacer(Modifier.width(8.dp)); Text("New conversation")
            }
            OutlinedTextField(historyQuery, { historyQuery = it }, placeholder = { Text("Find a conversation", fontSize = 13.sp) },
                leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                shape = RoundedCornerShape(16.dp), modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp).fillMaxWidth())
            var contextMenuChat by remember { mutableStateOf<String?>(null) }
            var showRenameDialog by remember { mutableStateOf<Conversation?>(null) }
            var showDeleteDialog by remember { mutableStateOf<Conversation?>(null) }
            val drawerScope = rememberCoroutineScope()
            LazyColumn(Modifier.weight(1f)) {
                items(state.conversations.filter { it.title.contains(historyQuery, ignoreCase = true) }, key = { it.id }) { chat ->
                    Box(Modifier.padding(horizontal = 12.dp, vertical = 2.dp)) {
                        NavigationDrawerItem(
                            label = { Text(chat.title, maxLines = 2, fontSize = 14.sp, overflow = TextOverflow.Ellipsis) },
                            icon = { Icon(if (chat.pinned) Icons.Outlined.PushPin else Icons.Outlined.ChatBubbleOutline, null, Modifier.size(18.dp), tint = if (chat.pinned) Moss else TextMuted) },
                            colors = NavigationDrawerItemDefaults.colors(selectedContainerColor = Elevated, unselectedContainerColor = Surface),
                            selected = state.current.id == chat.id,
                            onClick = { repository.select(chat.id); drawerOpen = false },
                            modifier = Modifier.combinedClickable(
                                onClick = { repository.select(chat.id); drawerOpen = false },
                                onLongClick = { contextMenuChat = chat.id }
                            ))
                        DropdownMenu(expanded = contextMenuChat == chat.id, onDismissRequest = { contextMenuChat = null },
                            containerColor = Elevated) {
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
                    containerColor = Elevated)
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
                    containerColor = Elevated)
            }
            if (!bubble) {
                HorizontalDivider(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = Border)
                NavigationDrawerItem(label = { Text("Memories", fontSize = 14.sp) },
                    icon = { Icon(Icons.Outlined.Psychology, null, Modifier.size(18.dp)) },
                    colors = NavigationDrawerItemDefaults.colors(unselectedContainerColor = Surface),
                    selected = false, onClick = { screen = Screen.Memories; drawerOpen = false },
                    modifier = Modifier.padding(horizontal = 12.dp))
                NavigationDrawerItem(label = { Text("Automations", fontSize = 14.sp) },
                    icon = { Icon(Icons.Outlined.Schedule, null, Modifier.size(18.dp)) },
                    colors = NavigationDrawerItemDefaults.colors(unselectedContainerColor = Surface),
                    selected = false, onClick = { screen = Screen.Crons; drawerOpen = false },
                    modifier = Modifier.padding(horizontal = 12.dp))
                NavigationDrawerItem(label = { Text("Integrations", fontSize = 14.sp) },
                    icon = { Icon(Icons.Outlined.Extension, null, Modifier.size(18.dp)) },
                    colors = NavigationDrawerItemDefaults.colors(unselectedContainerColor = Surface),
                    selected = false, onClick = { screen = Screen.Integrations; drawerOpen = false },
                    modifier = Modifier.padding(horizontal = 12.dp))
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = onBubbleSettings, modifier = Modifier.padding(horizontal = 16.dp)) {
                    Icon(Icons.Outlined.BubbleChart, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Floating chat settings")
                }
            }
            Row(Modifier.padding(horizontal = 24.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                ConnectionDot(connected, status); Spacer(Modifier.width(6.dp))
                Text(status, fontSize = 11.sp, color = Moss)
            }
            TextButton(onClick = { settingsOpen = true; drawerOpen = false }, modifier = Modifier.padding(16.dp)) {
                Icon(Icons.Outlined.Settings, null); Spacer(Modifier.width(8.dp)); Text("Connection settings")
            }
        }
    }) {
        Scaffold(containerColor = Ink, floatingActionButton = {
            if (!bubble && state.current.messages.isNotEmpty()) {
                SmallFloatingActionButton(
                    onClick = { repository.newChat() },
                    containerColor = Elevated,
                    contentColor = Coral,
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier.padding(bottom = 72.dp)
                ) { Icon(Icons.Outlined.Add, "New conversation", Modifier.size(20.dp)) }
            }
        }, topBar = {
            if (bubble) {
                Row(Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Beep", fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.width(8.dp))
                    Box(Modifier.size(5.dp).background(if (connected) Moss else Color(0xFFE85A4A), CircleShape))
                    Spacer(Modifier.weight(1f))
                    IconButton(onClick = onExpand) { Icon(Icons.Outlined.OpenInFull, "Open full app", Modifier.size(18.dp)) }
                    Box {
                        IconButton(onClick = { bubbleMenu = true }) { Icon(Icons.Outlined.MoreVert, "Chat options", Modifier.size(20.dp)) }
                        DropdownMenu(expanded = bubbleMenu, onDismissRequest = { bubbleMenu = false }) {
                            DropdownMenuItem(text = { Text("New conversation") }, onClick = { repository.newChat(); bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Conversations") }, onClick = { drawerOpen = true; bubbleMenu = false })
                            DropdownMenuItem(text = { Text("Connection settings") }, onClick = { settingsOpen = true; bubbleMenu = false })
                        }
                    }
                }
            } else TopAppBar(title = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(38.dp))
                    Spacer(Modifier.width(10.dp))
                    Column {
                        Text("Beep", fontWeight = FontWeight.Bold, fontSize = 21.sp)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            ConnectionDot(connected, status)
                            Spacer(Modifier.width(5.dp))
                            Text(if (connected) "Here to help" else "Tap to reconnect", fontSize = 11.sp, color = Moss,
                                maxLines = 1, overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.clickable { repository.reconnect() })
                        }
                    }
                }
            }, navigationIcon = { IconButton(onClick = { drawerOpen = true }) { Icon(Icons.Outlined.Menu, "Conversations") } },
                actions = {
                    if (bubble) IconButton(onClick = onExpand) { Icon(Icons.Outlined.OpenInFull, "Open full app") }
                    else IconButton(onClick = onFloat) { Icon(Icons.Outlined.BubbleChart, "Float chat") }
                    IconButton(onClick = { settingsOpen = true }) { Icon(Icons.Outlined.Settings, "Connection settings") }
                }, colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
        }) { padding ->
            ChatScreen(state.current, repository, Modifier.padding(padding).fillMaxSize(), compact = bubble, focusInput = focusInput, clipboardContent = clipboardContent)
        }
    } // end ModalNavigationDrawer
    } // end Screen.Chat
    } // end when
    } // end Crossfade
    // Browser overlay
    AnimatedVisibility(visible = browserUrlState != null,
        enter = fadeIn() + slideInVertically { it },
        exit = fadeOut() + slideOutVertically { it }) {
        browserUrlState?.let { url ->
            BrowserScreen(url = url, onClose = { repository.browserUrl.value = null },
                onContentExtracted = { /* content available for future use */ })
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
    var followLatest by remember(chat.id) { mutableStateOf(true) }
    LaunchedEffect(list, chat.id) {
        snapshotFlow { list.isScrollInProgress to list.canScrollForward }.collect { (scrolling, canForward) ->
            if (scrolling) followLatest = !canForward
        }
    }
    LaunchedEffect(chat.id) { if (chat.messages.isNotEmpty()) list.scrollToItem(chat.messages.lastIndex) }
    LaunchedEffect(chat.messages.size, chat.messages.lastOrNull()?.text?.length, chat.status) {
        if (chat.messages.isNotEmpty() && followLatest) list.animateScrollToItem(chat.messages.size - 1)
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
        if (chat.messages.isEmpty() && compact) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.Center) {
                Text("Hey. What can I help with?", fontSize = 18.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.height(6.dp))
                Text("Ask a question or search the web.", fontSize = 13.sp, color = TextMuted)
            }
        } else if (chat.messages.isEmpty()) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 28.dp, vertical = 24.dp), verticalArrangement = Arrangement.Center) {
                Box(contentAlignment = Alignment.Center) {
                    Box(Modifier.size(96.dp).background(
                        Brush.radialGradient(listOf(Moss.copy(alpha = 0.25f), Color.Transparent)),
                        shape = CircleShape))
                    Surface(shape = RoundedCornerShape(18.dp), color = Elevated) {
                        Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
                    }
                }
                Spacer(Modifier.height(24.dp))
                Text("A little help.\nRight here.", fontSize = 36.sp, lineHeight = 42.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp)
                Spacer(Modifier.height(12.dp))
                Text("Find an answer. Untangle a thought.\nKeep something worth remembering.", color = TextMuted, lineHeight = 23.sp)
                Spacer(Modifier.height(30.dp))
                val suggestionsVisible = remember { mutableStateOf(false) }
                LaunchedEffect(Unit) { delay(300); suggestionsVisible.value = true }
                listOf(
                    Triple("Remember something", "Remember that I prefer short, practical answers.", Icons.Outlined.BookmarkBorder),
                    Triple("Search the web", "Search the web for ", Icons.Outlined.Search),
                    Triple("Set a reminder", "Remind me to ", Icons.Outlined.Alarm),
                ).forEachIndexed { index, (title, prompt, icon) ->
                    AnimatedVisibility(visible = suggestionsVisible.value,
                        enter = fadeIn(tween(400, delayMillis = index * 120)) + slideInVertically(tween(400, delayMillis = index * 120)) { it / 3 }) {
                        Suggestion(title, prompt, icon) { draft = it }
                    }
                    if (index < 2) Spacer(Modifier.height(10.dp))
                }
            }
        } else {
            val noMotion = reduceMotion()
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(if (compact) 12.dp else 20.dp), verticalArrangement = Arrangement.spacedBy(if (compact) 12.dp else 22.dp)) {
                items(chat.messages, key = { it.id }) { message ->
                    val appeared = remember { mutableStateOf(false) }
                    LaunchedEffect(Unit) { appeared.value = true }
                    AnimatedVisibility(visible = appeared.value,
                        enter = if (noMotion) EnterTransition.None else fadeIn(tween(250)) + slideInVertically(spring(dampingRatio = Spring.DampingRatioLowBouncy, stiffness = Spring.StiffnessMediumLow)) { it / 3 }
                    ) {
                    Column(Modifier.fillMaxWidth().animateItem(), horizontalAlignment = if (message.role == "user") Alignment.End else Alignment.Start) {
                        if (message.role == "assistant" && !compact) {
                            val isLastAssistant = chat.messages.lastOrNull { it.role == "assistant" }?.id == message.id
                            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(bottom = 7.dp)) {
                                BeepAvatar(isThinking = isLastAssistant && chat.busy)
                                Spacer(Modifier.width(8.dp))
                                Text("BEEP", fontSize = 10.sp, fontWeight = FontWeight.Bold, letterSpacing = 2.sp, color = Coral)
                            }
                        }
                        Surface(color = if (message.role == "user") Color.Transparent else Color.Transparent,
                            shape = RoundedCornerShape(20.dp, 20.dp, if (message.role == "user") 5.dp else 20.dp, 20.dp),
                            modifier = if (message.role == "user") Modifier.fillMaxWidth(0.85f).background(
                                Brush.linearGradient(listOf(Color(0xFF182A22), Color(0xFF14201A))),
                                RoundedCornerShape(20.dp, 20.dp, 5.dp, 20.dp))
                            else Modifier.fillMaxWidth()) {
                            Column {
                                // Show attached image thumbnail in user messages
                                if (message.imageBase64 != null) {
                                    val msgBitmap = remember(message.imageBase64) { decodeImageBase64(message.imageBase64) }
                                    if (msgBitmap != null) {
                                        Image(msgBitmap.asImageBitmap(), "Attached image",
                                            contentScale = ContentScale.FillWidth,
                                            modifier = Modifier.fillMaxWidth()
                                                .clip(RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
                                                .heightIn(max = 200.dp))
                                    }
                                }
                                val isStreaming = message.role == "assistant" && chat.busy && chat.messages.lastOrNull { it.role == "assistant" }?.id == message.id
                                val textLen = message.text.length
                                val displayedLen by animateIntAsState(textLen, tween(if (isStreaming) 150 else 0), label = "stream-text")
                                val displayText = if (isStreaming && displayedLen < textLen) message.text.substring(0, displayedLen) else message.text
                                MessageContent(displayText, message.role, compact)
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
                        if (message.role == "assistant" && message.text.isNotBlank() && !chat.busy && !compact) CopyReply(message.text, compact)
                    }
                    } // end AnimatedVisibility
                }
            }
        }
        AnimatedVisibility(visible = !followLatest && chat.messages.isNotEmpty(),
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically(),
            modifier = Modifier.align(Alignment.CenterHorizontally)) {
            TextButton(onClick = { scope.launch { list.animateScrollToItem(chat.messages.lastIndex); followLatest = true } }) {
                Icon(Icons.Outlined.ArrowDownward, null, Modifier.size(16.dp)); Spacer(Modifier.width(6.dp)); Text("Latest reply", fontSize = 12.sp)
            }
        }
        AnimatedVisibility(visible = chat.status.isNotEmpty() || chat.tool.isNotEmpty(),
            enter = fadeIn(tween(200)) + expandVertically(tween(200)), exit = fadeOut(tween(150)) + shrinkVertically(tween(150))) {
            Row(Modifier.padding(horizontal = 22.dp, vertical = 8.dp).animateContentSize(spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium)),
                verticalAlignment = Alignment.CenterVertically) {
                BeepAvatar(isThinking = true, modifier = Modifier)
                Spacer(Modifier.width(10.dp))
                if (chat.tool.isNotEmpty()) {
                    val toolDone = chat.tool == "Done" || chat.tool.contains("didn't complete")
                    val toolTransition = rememberInfiniteTransition(label = "tool-pulse")
                    val toolAlpha by if (toolDone || reduceMotion()) remember { mutableStateOf(1f) } else toolTransition.animateFloat(
                        initialValue = 0.6f, targetValue = 1f,
                        animationSpec = infiniteRepeatable(tween(700, easing = EaseInOutCubic), RepeatMode.Reverse),
                        label = "tool-alpha"
                    )
                    Surface(shape = RoundedCornerShape(6.dp), color = Elevated,
                        modifier = Modifier.graphicsLayer { alpha = toolAlpha }.animateContentSize()) {
                        Text(chat.tool, modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp), color = Coral, fontSize = 11.sp, fontWeight = FontWeight.Medium, fontFamily = FontFamily.Monospace)
                    }
                    Spacer(Modifier.width(8.dp))
                }
                if (chat.status.isNotEmpty() && chat.tool.isEmpty()) {
                    PulsingDots()
                    Spacer(Modifier.width(8.dp))
                }
                Text(chat.status, color = TextMuted, fontSize = 12.sp, maxLines = 2)
            }
        }
        AnimatedVisibility(visible = chat.error.isNotEmpty(),
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Row(Modifier.padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(chat.error, fontSize = 12.sp, color = MaterialTheme.colorScheme.error, modifier = Modifier.weight(1f), maxLines = 4)
                if (chat.busy) TextButton(onClick = repository::retry) { Text("Retry") }
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
            Row(Modifier.padding(horizontal = if (compact) 12.dp else 18.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                val pulseAlpha by rememberInfiniteTransition(label = "listen-pulse").animateFloat(
                    initialValue = 0.4f, targetValue = 1f,
                    animationSpec = infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "listen-alpha")
                Box(Modifier.size(6.dp).background(Coral.copy(alpha = pulseAlpha), CircleShape))
                Spacer(Modifier.width(6.dp))
                Text("Listening...", fontSize = 11.sp, color = Coral.copy(alpha = 0.8f))
            }
        }

        // Image attachment preview
        AnimatedVisibility(visible = attachedImage != null,
            enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            attachedImage?.let { img ->
                val previewBitmap = remember(img) { decodeImageBase64(img) }
                if (previewBitmap != null) {
                    Box(Modifier.padding(horizontal = if (compact) 12.dp else 18.dp, vertical = 4.dp)) {
                        Surface(shape = RoundedCornerShape(12.dp), color = Elevated,
                            modifier = Modifier.height(72.dp).width(72.dp)) {
                            Image(previewBitmap.asImageBitmap(), "Attached image",
                                contentScale = ContentScale.Crop,
                                modifier = Modifier.fillMaxSize().clip(RoundedCornerShape(12.dp)))
                        }
                        // Remove button
                        Surface(onClick = { attachedImage = null },
                            shape = CircleShape, color = Color(0xFF2A1A1A),
                            modifier = Modifier.size(22.dp).align(Alignment.TopEnd).offset(x = 4.dp, y = (-4).dp)) {
                            Icon(Icons.Outlined.Close, "Remove image",
                                modifier = Modifier.padding(3.dp).size(16.dp), tint = Color(0xFFE85A4A))
                        }
                    }
                }
            }
        }

        Row(Modifier.padding(horizontal = if (compact) 8.dp else 14.dp, vertical = if (compact) 4.dp else 8.dp).fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            // Image picker button
            if (!chat.busy && draft.isBlank() && attachedImage == null) {
                FilledIconButton(onClick = {
                    photoPicker.launch(androidx.activity.result.PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                },
                    modifier = Modifier.size(48.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(containerColor = Elevated, contentColor = Moss)) {
                    Icon(Icons.Outlined.Image, "Attach image", modifier = Modifier.size(22.dp))
                }
                Spacer(Modifier.width(4.dp))
            }
            OutlinedTextField(value = draft, onValueChange = { if (it.toByteArray().size <= 32768) draft = it },
                modifier = Modifier.weight(1f).focusRequester(inputFocusRequester), placeholder = { Text(if (listening) "Speak now..." else if (attachedImage != null) "Describe the image..." else "Ask anything...", fontSize = 14.sp, color = TextDim) },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Color.Transparent, focusedBorderColor = Moss.copy(alpha = 0.5f)),
                shape = RoundedCornerShape(if (compact) 20.dp else 24.dp), maxLines = if (compact) 3 else 6,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send), keyboardActions = KeyboardActions(onSend = { submit() }))
            Spacer(Modifier.width(8.dp))
            if (chat.busy) {
                FilledIconButton(onClick = repository::cancel,
                    modifier = Modifier.size(48.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(containerColor = Color(0xFF2A1A1A), contentColor = Color(0xFFE85A4A))) {
                    Icon(Icons.Outlined.StopCircle, "Stop reply", modifier = Modifier.size(22.dp))
                }
            } else if (draft.isNotBlank() || attachedImage != null) {
                var sendPulse by remember { mutableStateOf(false) }
                val sendScale by animateFloatAsState(if (sendPulse) 1.15f else 1f, spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMedium), label = "send-pulse")
                LaunchedEffect(sendPulse) { if (sendPulse) { delay(150); sendPulse = false } }
                FilledIconButton(onClick = { sendPulse = true; submit() },
                    modifier = Modifier.size(48.dp).graphicsLayer { scaleX = sendScale; scaleY = sendScale },
                    colors = IconButtonDefaults.filledIconButtonColors(
                        containerColor = Coral, contentColor = Ink)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send message", modifier = Modifier.size(19.dp))
                }
            } else if (speechAvailable) {
                val micPulse by rememberInfiniteTransition(label = "mic-pulse").animateFloat(
                    initialValue = 1f, targetValue = 1.15f,
                    animationSpec = infiniteRepeatable(tween(500), RepeatMode.Reverse), label = "mic-scale")
                FilledIconButton(onClick = { startVoice() },
                    modifier = Modifier.size(48.dp).graphicsLayer {
                        if (listening) { scaleX = micPulse; scaleY = micPulse }
                    },
                    colors = IconButtonDefaults.filledIconButtonColors(
                        containerColor = if (listening) Coral else Elevated,
                        contentColor = if (listening) Ink else Moss)) {
                    Icon(Icons.Outlined.Mic, if (listening) "Stop listening" else "Voice input", modifier = Modifier.size(22.dp))
                }
            } else {
                // Speech not available: show disabled send button
                FilledIconButton(onClick = {}, enabled = false,
                    modifier = Modifier.size(48.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(
                        disabledContainerColor = Elevated, disabledContentColor = TextDim)) {
                    Icon(Icons.AutoMirrored.Outlined.Send, "Send message", modifier = Modifier.size(19.dp))
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
        Surface(
            color = Elevated,
            shape = RoundedCornerShape(14.dp),
            border = BorderStroke(1.dp, Border),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 6.dp)
        ) {
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
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
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
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(10.dp),
        color = Color.Transparent,
        border = BorderStroke(1.dp, Moss.copy(alpha = 0.5f))
    ) {
        Text(label, fontSize = 11.sp, color = Moss, fontWeight = FontWeight.Medium,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp))
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
            Modifier.size(14.dp), tint = Color(0xFF71816F))
        if (!compact) { Spacer(Modifier.width(6.dp)); Text(if (copied) "Copied" else "Copy reply", fontSize = 11.sp, color = TextMuted) }
    }
}

// ── Rich message rendering ──────────────────────────────────────────────────

private val CodeBlockBg = Color(0xFF0B100E)
private val CodeBlockHeader = Color(0xFF080C0A)
private val TableHeaderBg = Color(0xFF080C0A)
private val TableAltRow = Color(0xFF0E1510)

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

    Surface(
        shape = RoundedCornerShape(14.dp),
        color = Elevated,
        modifier = Modifier.fillMaxWidth().animateContentSize()
    ) {
        Column(Modifier.padding(14.dp)) {
            Text(name, fontWeight = FontWeight.Bold, fontSize = if (compact) 14.sp else 15.sp, color = TextPrimary)
            if (rating > 0) {
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val fullStars = rating.toInt()
                    repeat(fullStars) {
                        Icon(Icons.Outlined.Star, null, Modifier.size(14.dp), tint = Coral)
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
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = {
                    val uri = if (lat != 0.0 && lng != 0.0) "geo:$lat,$lng?q=${Uri.encode(name)}"
                    else "geo:0,0?q=${Uri.encode(name)}"
                    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
                },
                shape = RoundedCornerShape(10.dp),
                border = BorderStroke(1.dp, Moss.copy(alpha = 0.6f)),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                modifier = Modifier.height(34.dp)
            ) {
                Icon(Icons.Outlined.Map, null, Modifier.size(14.dp), tint = Moss)
                Spacer(Modifier.width(6.dp))
                Text("Open in Maps", fontSize = 12.sp, color = Moss)
            }
        }
    }
}

@Composable
private fun LinkCard(data: org.json.JSONObject, onBrowse: (String) -> Unit, compact: Boolean) {
    val title = data.optString("title", "")
    val description = data.optString("description", "")
    val url = data.optString("url", "")

    Surface(
        shape = RoundedCornerShape(14.dp),
        color = Elevated,
        modifier = Modifier.fillMaxWidth().clickable(enabled = url.isNotBlank()) { onBrowse(url) }.animateContentSize()
    ) {
        Column(Modifier.padding(14.dp)) {
            if (title.isNotBlank()) {
                Text(title, fontWeight = FontWeight.SemiBold, fontSize = if (compact) 13.sp else 14.sp, color = TextPrimary, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (description.isNotBlank()) {
                if (title.isNotBlank()) Spacer(Modifier.height(3.dp))
                Text(description, fontSize = if (compact) 12.sp else 13.sp, color = TextMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (url.isNotBlank()) {
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.Link, null, Modifier.size(12.dp), tint = Moss)
                    Spacer(Modifier.width(4.dp))
                    Text(url.removePrefix("https://").removePrefix("http://").take(40), fontSize = 11.sp, color = Moss, maxLines = 1, overflow = TextOverflow.Ellipsis)
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

    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        buttons.forEachIndexed { index, button ->
            val label = button.optString("label", "Action")
            val action = button.optString("action", "")
            val isPrimary = index == 0
            OutlinedButton(
                onClick = {
                    when (action) {
                        "open_browser" -> repository.browserUrl.value = button.optString("url", "")
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
                shape = RoundedCornerShape(10.dp),
                border = BorderStroke(1.dp, if (isPrimary) Coral.copy(alpha = 0.7f) else Moss.copy(alpha = 0.5f)),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp)
            ) {
                Text(label, fontSize = 13.sp, color = if (isPrimary) Coral else Moss)
            }
        }
    }
}

@Composable
private fun TaskProgressCard(steps: List<TaskStep>) {
    val noMotion = reduceMotion()
    Surface(
        shape = RoundedCornerShape(14.dp),
        color = Elevated,
        modifier = Modifier.fillMaxWidth().animateContentSize()
    ) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            steps.forEach { step ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    when (step.status) {
                        "pending" -> {
                            Box(
                                Modifier.size(18.dp),
                                contentAlignment = Alignment.Center
                            ) {
                                Canvas(Modifier.size(14.dp)) {
                                    drawCircle(
                                        color = Color(0xFF7A8A80),
                                        radius = size.minDimension / 2,
                                        style = androidx.compose.ui.graphics.drawscope.Stroke(width = 1.5.dp.toPx())
                                    )
                                }
                            }
                        }
                        "in_progress" -> {
                            val pulse = rememberInfiniteTransition(label = "step-pulse")
                            val alpha by if (noMotion) remember { mutableStateOf(1f) } else pulse.animateFloat(
                                initialValue = 0.4f, targetValue = 1f,
                                animationSpec = infiniteRepeatable(tween(800, easing = EaseInOutCubic), RepeatMode.Reverse),
                                label = "step-alpha"
                            )
                            Box(
                                Modifier.size(18.dp),
                                contentAlignment = Alignment.Center
                            ) {
                                Canvas(Modifier.size(10.dp)) {
                                    drawCircle(
                                        color = Color(0xFF3B9B7A).copy(alpha = alpha),
                                        radius = size.minDimension / 2
                                    )
                                }
                            }
                        }
                        "completed" -> {
                            Icon(
                                Icons.Outlined.Check,
                                contentDescription = "Completed",
                                tint = Moss,
                                modifier = Modifier.size(18.dp)
                            )
                        }
                        "failed" -> {
                            Icon(
                                Icons.Outlined.Close,
                                contentDescription = "Failed",
                                tint = Coral,
                                modifier = Modifier.size(18.dp)
                            )
                        }
                    }
                    Spacer(Modifier.width(10.dp))
                    Text(
                        step.label,
                        fontSize = 13.sp,
                        color = when (step.status) {
                            "completed" -> TextPrimary
                            "failed" -> Coral
                            "in_progress" -> TextPrimary
                            else -> TextMuted
                        }
                    )
                }
            }
        }
    }
}

@Composable
private fun CardView(card: Card, repository: ChatRepository, compact: Boolean) {
    when (card.type) {
        "place" -> PlaceCard(card.data, compact)
        "link" -> LinkCard(card.data, onBrowse = { repository.browserUrl.value = it }, compact)
        "action_buttons" -> ActionButtonRow(card.data, repository)
    }
}

@Composable
private fun MessageContent(text: String, role: String, compact: Boolean) {
    val blocks = remember(text) { parseBlocks(text) }
    val padding = if (role == "user") (if (compact) 10.dp else 16.dp) else if (compact) 4.dp else 6.dp
    Column(Modifier.padding(padding), verticalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 10.dp)) {
        blocks.forEach { block ->
            when (block) {
                is MsgBlock.Text -> SelectionContainer {
                    Text(remember(block.content) { formattedMessage(block.content) },
                        fontSize = if (compact) 14.sp else 15.5.sp, lineHeight = if (compact) 21.sp else 24.sp)
                }
                is MsgBlock.Code -> CodeBlockView(block.language, block.code, compact)
                is MsgBlock.Table -> TableView(block.headers, block.rows, compact)
                is MsgBlock.Quote -> BlockQuoteView(block.content, compact)
                MsgBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = 6.dp), color = Border.copy(alpha = 0.5f))
            }
        }
    }
}

@Composable
private fun CodeBlockView(language: String, code: String, compact: Boolean) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1800); copied = false } }
    Surface(shape = RoundedCornerShape(12.dp), color = CodeBlockBg,
        modifier = Modifier.fillMaxWidth(), shadowElevation = 2.dp) {
        Column {
            Row(Modifier.fillMaxWidth().background(CodeBlockHeader, RoundedCornerShape(topStart = 12.dp, topEnd = 12.dp))
                .padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (language.isNotBlank()) {
                    Surface(shape = RoundedCornerShape(4.dp), color = Border.copy(alpha = 0.4f)) {
                        Text(language, Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
                            fontSize = 10.sp, color = TextMuted, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Medium, letterSpacing = 0.5.sp)
                    }
                }
                Spacer(Modifier.weight(1f))
                Row(Modifier.clickable {
                    (context as? MainActivity)?.suppressNextClipboard = true
                    context.getSystemService(android.content.ClipboardManager::class.java)
                        .setPrimaryClip(android.content.ClipData.newPlainText("Code", code))
                    copied = true
                }.padding(horizontal = 6.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy, null,
                        Modifier.size(13.dp), tint = if (copied) Moss else TextDim)
                    Spacer(Modifier.width(5.dp))
                    Text(if (copied) "Copied" else "Copy", fontSize = 11.sp, color = if (copied) Moss else TextDim)
                }
            }
            SelectionContainer {
                Text(code, modifier = Modifier.horizontalScroll(rememberScrollState()).padding(14.dp),
                    fontFamily = FontFamily.Monospace, fontSize = if (compact) 12.sp else 13.sp,
                    lineHeight = if (compact) 18.sp else 20.sp, color = Color(0xFFD4E0D8))
            }
        }
    }
}

@Composable
private fun TableView(headers: List<String>, rows: List<List<String>>, compact: Boolean) {
    Surface(shape = RoundedCornerShape(10.dp), color = CodeBlockBg,
        modifier = Modifier.fillMaxWidth(), shadowElevation = 1.dp) {
        Column(Modifier.horizontalScroll(rememberScrollState())) {
            Row(Modifier.background(TableHeaderBg, RoundedCornerShape(topStart = 10.dp, topEnd = 10.dp)).padding(vertical = 10.dp, horizontal = 4.dp)) {
                headers.forEach { header ->
                    SelectionContainer {
                        Text(header, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                            fontSize = if (compact) 11.sp else 12.sp, fontWeight = FontWeight.SemiBold,
                            color = Coral.copy(alpha = 0.85f), maxLines = 2, letterSpacing = 0.3.sp)
                    }
                }
            }
            HorizontalDivider(color = Border.copy(alpha = 0.4f), thickness = 0.5.dp)
            rows.forEachIndexed { index, row ->
                Row(Modifier.then(if (index % 2 == 1) Modifier.background(TableAltRow) else Modifier).padding(vertical = 9.dp, horizontal = 4.dp)) {
                    row.forEach { cell ->
                        SelectionContainer {
                            Text(cell, modifier = Modifier.widthIn(min = 90.dp).padding(horizontal = 12.dp),
                                fontSize = if (compact) 11.sp else 12.sp, color = TextPrimary,
                                lineHeight = if (compact) 16.sp else 18.sp, maxLines = 4)
                        }
                    }
                }
                if (index < rows.lastIndex) HorizontalDivider(color = Border.copy(alpha = 0.15f), thickness = 0.5.dp)
            }
        }
    }
}

@Composable
private fun BlockQuoteView(content: String, compact: Boolean) {
    Surface(shape = RoundedCornerShape(6.dp), color = Elevated.copy(alpha = 0.4f),
        modifier = Modifier.fillMaxWidth().drawBehind {
            drawRoundRect(Moss.copy(alpha = 0.6f), size = androidx.compose.ui.geometry.Size(3.dp.toPx(), size.height),
                cornerRadius = androidx.compose.ui.geometry.CornerRadius(2.dp.toPx()))
        }) {
        SelectionContainer {
            Text(remember(content) { formattedMessage(content) },
                modifier = Modifier.padding(start = 14.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
                fontSize = if (compact) 13.sp else 14.sp, lineHeight = if (compact) 19.sp else 22.sp,
                color = TextMuted)
        }
    }
}

// Inline markdown: links, bold, strikethrough, inline code, headings, bullets, numbered lists.
private fun formattedMessage(text: String) = buildAnnotatedString {
    val inlineTokens = Regex("""\[([^\]]+)\]\((https?://[^\s)]+)\)|(https?://[^\s<>]+)|\*\*(.+?)\*\*|~~(.+?)~~|`([^`\n]+)`""")
    val lines = text.split('\n')
    var firstLine = true
    for (line in lines) {
        if (!firstLine) append("\n")
        firstLine = false
        val heading = Regex("^#{1,4} +").find(line)
        val content = line.removePrefix(heading?.value.orEmpty())
            .replace(Regex("^[-*] +"), "\u2022 ")
        pushStyle(if (heading != null) SpanStyle(fontWeight = FontWeight.Bold, fontSize = 17.sp, letterSpacing = (-0.3).sp) else SpanStyle())
        var offset = 0
        for (match in inlineTokens.findAll(content)) {
            append(content.substring(offset, match.range.first))
            when {
                match.groups[4] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(match.groups[4]!!.value) }
                match.groups[5] != null -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough, color = TextMuted)) { append(match.groups[5]!!.value) }
                match.groups[6] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = Color(0xFF1A1614), color = Color(0xFFF09888), fontSize = 13.sp)) { append("\u200A${match.groups[6]!!.value}\u200A") }
                else -> {
                    val url = (match.groups[2]?.value ?: match.groups[3]!!.value).trimEnd('.', ',', ';')
                    withLink(LinkAnnotation.Url(url, TextLinkStyles(style = SpanStyle(color = Moss, textDecoration = TextDecoration.Underline)))) {
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

@Composable
private fun PulsingDots() {
    val noMotion = reduceMotion()
    if (noMotion) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            repeat(3) { Box(Modifier.size(6.dp).background(Moss, CircleShape)) }
        }
        return
    }
    val infiniteTransition = rememberInfiniteTransition(label = "dots")
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        repeat(3) { index ->
            val alpha by infiniteTransition.animateFloat(
                initialValue = 0.25f, targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = tween(450, easing = EaseInOutCubic, delayMillis = index * 160),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-alpha-$index"
            )
            val scale by infiniteTransition.animateFloat(
                initialValue = 0.7f, targetValue = 1.15f,
                animationSpec = infiniteRepeatable(
                    animation = tween(450, easing = EaseInOutCubic, delayMillis = index * 160),
                    repeatMode = RepeatMode.Reverse
                ), label = "dot-scale-$index"
            )
            Box(Modifier.size(6.dp).graphicsLayer { scaleX = scale; scaleY = scale }
                .background(Moss.copy(alpha = alpha), CircleShape))
        }
    }
}

@Composable
private fun ConnectionDot(connected: Boolean, status: String) {
    val reconnecting = !connected && status.contains("Reconnecting", ignoreCase = true)
    if (reconnecting) {
        val infiniteTransition = rememberInfiniteTransition(label = "reconnect-dot")
        val alpha by infiniteTransition.animateFloat(0.3f, 1f, infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "reconnect-alpha")
        Box(Modifier.size(6.dp).background(Color(0xFFE8B84A).copy(alpha = alpha), CircleShape))
    } else {
        Box(Modifier.size(6.dp).background(if (connected) Moss else Color(0xFFE85A4A), CircleShape))
    }
}

@Composable
fun BeepAvatar(isThinking: Boolean, modifier: Modifier = Modifier) {
    val noMotion = reduceMotion()
    val infiniteTransition = rememberInfiniteTransition(label = "beep-avatar")

    // Idle: gentle breathing scale (0.97 to 1.03). Active: slightly faster pulse.
    val breatheScale by if (noMotion) remember { mutableStateOf(1f) } else infiniteTransition.animateFloat(
        initialValue = 0.97f, targetValue = 1.03f,
        animationSpec = infiniteRepeatable(
            animation = tween(if (isThinking) 800 else 2400, easing = EaseInOutCubic),
            repeatMode = RepeatMode.Reverse
        ), label = "avatar-breathe"
    )

    // Emerald glow alpha when thinking.
    val glowAlpha by if (noMotion || !isThinking) remember { mutableStateOf(0f) } else infiniteTransition.animateFloat(
        initialValue = 0.15f, targetValue = 0.45f,
        animationSpec = infiniteRepeatable(
            animation = tween(900, easing = EaseInOutCubic),
            repeatMode = RepeatMode.Reverse
        ), label = "avatar-glow"
    )

    Box(modifier.size(32.dp)
        .graphicsLayer { scaleX = breatheScale; scaleY = breatheScale }
        .then(if (isThinking && !noMotion) Modifier.drawBehind {
            drawCircle(Moss.copy(alpha = glowAlpha), radius = size.minDimension * 0.75f)
        } else Modifier)
        .background(Elevated, CircleShape),
        contentAlignment = Alignment.Center
    ) {
        if (isThinking && !noMotion) {
            Row(horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.CenterVertically) {
                repeat(3) { index ->
                    val offset by infiniteTransition.animateFloat(
                        initialValue = 0f, targetValue = -4f,
                        animationSpec = infiniteRepeatable(
                            animation = tween(400, delayMillis = index * 120, easing = EaseInOutCubic),
                            repeatMode = RepeatMode.Reverse
                        ), label = "dot-bounce-$index"
                    )
                    Canvas(Modifier.size(5.dp).graphicsLayer { translationY = offset }) {
                        drawCircle(Coral)
                    }
                }
            }
        } else {
            Row(horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.CenterVertically) {
                repeat(3) {
                    Canvas(Modifier.size(5.dp)) { drawCircle(Coral) }
                }
            }
        }
    }
}

@Composable
private fun Suggestion(title: String, prompt: String, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: (String) -> Unit) {
    var pressed by remember { mutableStateOf(false) }
    val scale by animateFloatAsState(if (pressed) 0.96f else 1f, spring(dampingRatio = Spring.DampingRatioMediumBouncy), label = "suggestion-scale")
    Surface(onClick = { pressed = true; onClick(prompt) }, shape = RoundedCornerShape(16.dp), color = Elevated,
        modifier = Modifier.fillMaxWidth().graphicsLayer { scaleX = scale; scaleY = scale }) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = Moss, modifier = Modifier.size(20.dp)); Spacer(Modifier.width(12.dp))
            Text(title, fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
            Icon(Icons.Outlined.NorthEast, null, tint = Moss, modifier = Modifier.size(16.dp))
        }
    }
    LaunchedEffect(pressed) { if (pressed) { delay(200); pressed = false } }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemoriesScreen(repository: ChatRepository, onBack: () -> Unit) {
    var memories by remember { mutableStateOf<List<Pair<String, String>>>(emptyList()) }
    var query by rememberSaveable { mutableStateOf("") }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    fun refresh() { scope.launch {
        loading = true; error = ""
        try { memories = withContext(Dispatchers.IO) { repository.listMemories() } }
        catch (e: Exception) { error = e.message.orEmpty() }
        finally { loading = false }
    } }
    LaunchedEffect(Unit) { refresh() }
    val filtered = memories.filter { (k, v) -> query.isBlank() || k.contains(query, true) || v.contains(query, true) }
    Scaffold(containerColor = Ink, topBar = {
        TopAppBar(title = { Text("Memories", fontSize = 18.sp) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(query, { query = it }, placeholder = { Text("Search memories", fontSize = 13.sp) },
                    leadingIcon = { Icon(Icons.Outlined.Search, null, Modifier.size(18.dp)) }, singleLine = true,
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.weight(1f),
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                Spacer(Modifier.width(12.dp))
                Text("${filtered.size}", fontSize = 12.sp, color = TextMuted)
            }
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (filtered.isEmpty()) Text(if (memories.isEmpty()) "No memories stored yet." else "No matches.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(filtered, key = { it.first }) { (key, content) ->
                    Surface(shape = RoundedCornerShape(10.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(key, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(content, fontSize = 12.sp, color = TextMuted, maxLines = 3, overflow = TextOverflow.Ellipsis, lineHeight = 18.sp)
                            }
                            IconButton(onClick = { scope.launch {
                                try { withContext(Dispatchers.IO) { repository.deleteMemory(key) }; refresh() }
                                catch (e: Exception) { error = e.message.orEmpty() }
                            } }, modifier = Modifier.size(36.dp)) {
                                Icon(Icons.Outlined.Close, "Delete", Modifier.size(16.dp), tint = TextDim)
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
        TopAppBar(title = { Text("Automations", fontSize = 18.sp) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { IconButton(onClick = { showCreate = true }) { Icon(Icons.Outlined.Add, "New automation") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (crons.isEmpty()) Text("No automations yet.\nSchedule tasks for Beep to run on a timer.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(crons, key = { it.id }) { cron ->
                    Surface(shape = RoundedCornerShape(10.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
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
                                }, modifier = Modifier.height(28.dp), colors = SwitchDefaults.colors(checkedTrackColor = Moss, uncheckedTrackColor = Border))
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
        title = { Text("New automation", fontSize = 18.sp, fontWeight = FontWeight.Medium) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(label, { label = it }, label = { Text("Name") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Morning briefing") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(schedule, { schedule = it }, label = { Text("Schedule (cron)") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("0 9 * * *") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
            OutlinedTextField(prompt, { prompt = it }, label = { Text("Prompt") }, maxLines = 4, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Give me a summary of...") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
        } },
        confirmButton = { Button(onClick = { if (label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) onCreate(label, prompt, schedule) },
            enabled = label.isNotBlank() && prompt.isNotBlank() && schedule.isNotBlank()) { Text("Create") } },
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
        TopAppBar(title = { Text("Integrations", fontSize = 18.sp) },
            navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            actions = { IconButton(onClick = { showCreate = true }) { Icon(Icons.Outlined.Add, "New integration") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (loading) Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) }
            else if (integrations.isEmpty()) Text("No integrations yet.\nConnect services like email, calendar, and more.", fontSize = 13.sp, color = TextMuted, modifier = Modifier.padding(40.dp).fillMaxWidth(), textAlign = androidx.compose.ui.text.style.TextAlign.Center, lineHeight = 20.sp)
            else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(integrations, key = { it.id }) { integration ->
                    Surface(shape = RoundedCornerShape(10.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.Top) {
                            Column(Modifier.weight(1f)) {
                                Text(integration.label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(3.dp))
                                Text(integration.provider, fontSize = 12.sp, color = TextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Spacer(Modifier.height(4.dp))
                                Text(integration.status, fontSize = 11.sp,
                                    color = if (integration.status == "connected") Moss else TextDim,
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
        title = { Text("New integration", fontSize = 18.sp, fontWeight = FontWeight.Medium) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }) {
                OutlinedTextField(selectedProvider, {}, readOnly = true, label = { Text("Provider") }, singleLine = true,
                    modifier = Modifier.fillMaxWidth().menuAnchor(),
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                    colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                        unfocusedBorderColor = Border, focusedBorderColor = Moss))
                ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                    providers.forEach { provider ->
                        DropdownMenuItem(text = { Text(provider) }, onClick = { selectedProvider = provider; expanded = false })
                    }
                }
            }
            OutlinedTextField(label, { label = it }, label = { Text("Label") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("My Email") },
                colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = Elevated, focusedContainerColor = Elevated,
                    unfocusedBorderColor = Border, focusedBorderColor = Moss))
        } },
        confirmButton = { Button(onClick = { if (label.isNotBlank()) onCreate(selectedProvider, label) },
            enabled = label.isNotBlank()) { Text("Create") } },
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
            Box(contentAlignment = Alignment.Center) {
                Box(Modifier.size(96.dp).background(
                    Brush.radialGradient(listOf(Moss.copy(alpha = 0.25f), Color.Transparent)),
                    shape = CircleShape))
                Surface(shape = RoundedCornerShape(18.dp), color = Elevated) {
                    Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.padding(12.dp).size(48.dp))
                }
            }
            Spacer(Modifier.height(28.dp))
            Text("Almost ready.", fontSize = 32.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.5).sp)
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
            Button(
                onClick = {
                    if (locationGranted) {
                        // Trigger location updates now that permission is granted
                        (context as? MainActivity)?.startLocationUpdates()
                    }
                    onDone()
                },
                modifier = Modifier.fillMaxWidth().height(54.dp),
                shape = RoundedCornerShape(17.dp)
            ) {
                Text("Continue to Beep", fontWeight = FontWeight.SemiBold)
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
    Surface(shape = RoundedCornerShape(14.dp), color = Elevated, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = if (granted) Moss else TextMuted, modifier = Modifier.size(24.dp))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                Text(description, fontSize = 12.sp, color = TextMuted, lineHeight = 17.sp)
            }
            Spacer(Modifier.width(12.dp))
            if (granted) {
                Icon(Icons.Outlined.CheckCircle, "Granted", tint = Moss, modifier = Modifier.size(24.dp))
            } else {
                FilledTonalButton(onClick = onRequest, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp)) {
                    Text("Allow", fontSize = 13.sp)
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
        TopAppBar(title = { Text(if (canGoBack) "Connection" else "Welcome to Beep", fontSize = 18.sp) },
            navigationIcon = { if (canGoBack) IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back") } },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = Ink))
    }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize().imePadding(), contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            item {
                Icon(painterResource(R.drawable.ic_beep), null, tint = Color.Unspecified, modifier = Modifier.size(66.dp))
                Spacer(Modifier.height(24.dp))
                Text("Your assistant.\nYour space.", fontSize = 36.sp, lineHeight = 41.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp)
                Spacer(Modifier.height(12.dp))
                Text("Connect to your Beep server to bring your conversations and memory along.", color = TextMuted, lineHeight = 23.sp)
            }
            item { OutlinedButton(onClick = { profilePicker.launch(arrayOf("application/json", "text/*", "application/octet-stream")) }, enabled = !saving, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Outlined.FileOpen, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Import connection profile")
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
            item { Button(onClick = {
                saving = true; error = ""
                scope.launch {
                    try { onConnect(ConnectionSettings(nodeId.trim(), token.trim())) }
                    catch (e: Exception) { error = e.message ?: "Could not connect" }
                    finally { saving = false }
                }
            }, enabled = !saving && nodeId.isNotBlank(), modifier = Modifier.fillMaxWidth().height(54.dp), shape = RoundedCornerShape(17.dp)) {
                if (saving) { CircularProgressIndicator(Modifier.size(18.dp), color = Color.White, strokeWidth = 2.dp); Spacer(Modifier.width(10.dp)) }
                Text(if (saving) "Connecting…" else "Connect to Beep", fontWeight = FontWeight.SemiBold)
            } }
        }
    }
}
