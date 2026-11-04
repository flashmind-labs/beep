package com.flashmind.beep

import android.Manifest
import android.content.ContentResolver
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Matrix
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.util.Size
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

private fun cameraAllowed(context: Context) =
    ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

private suspend fun cameraProvider(context: Context): ProcessCameraProvider = suspendCancellableCoroutine { cont ->
    val future = ProcessCameraProvider.getInstance(context)
    future.addListener({
        runCatching { future.get() }.onSuccess { cont.resume(it) }.onFailure { cont.resumeWithException(it) }
    }, ContextCompat.getMainExecutor(context))
}

/**
 * The camera's live picture. Binds only its own use cases, so the sheet's tile closing
 * cannot stop the full screen camera that opens after it.
 */
@Composable
private fun CameraFeed(lens: Int, capture: ImageCapture?, modifier: Modifier, onCamera: (Camera?) -> Unit = {}) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = remember { PreviewView(context).apply {
        scaleType = PreviewView.ScaleType.FILL_CENTER
        implementationMode = PreviewView.ImplementationMode.COMPATIBLE
    } }
    val preview = remember { Preview.Builder().build() }
    var provider by remember { mutableStateOf<ProcessCameraProvider?>(null) }
    LaunchedEffect(lens) {
        val cameras = runCatching { cameraProvider(context) }.getOrNull() ?: return@LaunchedEffect
        provider = cameras
        cameras.unbind(*listOfNotNull(preview, capture).toTypedArray())
        preview.setSurfaceProvider(view.surfaceProvider)
        val selector = CameraSelector.Builder().requireLensFacing(lens).build()
        onCamera(runCatching { cameras.bindToLifecycle(owner, selector, *listOfNotNull(preview, capture).toTypedArray()) }.getOrNull())
    }
    DisposableEffect(Unit) { onDispose { provider?.unbind(*listOfNotNull(preview, capture).toTypedArray()) } }
    AndroidView({ view }, modifier)
}

/** The picture the camera took, turned upright. */
private fun ImageProxy.upright(): Bitmap {
    val bitmap = toBitmap()
    val degrees = imageInfo.rotationDegrees
    if (degrees == 0) return bitmap
    return Bitmap.createBitmap(bitmap, 0, 0, bitmap.width, bitmap.height, Matrix().apply { postRotate(degrees.toFloat()) }, true)
}

/** Read access to the phone's photos, all of them or the ones the user chose (Android 14). */
private fun photosAllowed(context: Context) = photoPermissions().any {
    ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
}

private fun photoPermissions() = when {
    Build.VERSION.SDK_INT >= 34 -> arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED)
    Build.VERSION.SDK_INT >= 33 -> arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
    else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
}

/** The newest [limit] photos on the phone, newest first. */
private fun recentPhotos(context: Context, limit: Int): List<Uri> {
    val images = MediaStore.Images.Media.EXTERNAL_CONTENT_URI
    val args = Bundle().apply {
        putStringArray(ContentResolver.QUERY_ARG_SORT_COLUMNS, arrayOf(MediaStore.Images.Media.DATE_ADDED))
        putInt(ContentResolver.QUERY_ARG_SORT_DIRECTION, ContentResolver.QUERY_SORT_DIRECTION_DESCENDING)
        putInt(ContentResolver.QUERY_ARG_LIMIT, limit)
    }
    return runCatching {
        context.contentResolver.query(images, arrayOf(MediaStore.Images.Media._ID), args, null)?.use { rows ->
            buildList { while (rows.moveToNext()) add(ContentUris.withAppendedId(images, rows.getLong(0))) }
        }
    }.getOrNull().orEmpty()
}

/** A photo library thumbnail filling its tile. */
@Composable
private fun PhotoThumb(uri: Uri, modifier: Modifier) {
    val context = LocalContext.current
    val thumb by produceState<Bitmap?>(null, uri) {
        value = withContext(Dispatchers.IO) { runCatching { context.contentResolver.loadThumbnail(uri, Size(300, 300), null) }.getOrNull() }
    }
    Box(modifier.background(Beep.colors.code)) {
        thumb?.let { Image(it.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
    }
}

/**
 * The image button's sheet: the camera first, then the newest photos on the phone to pick from,
 * and the system photo picker for the rest. Without the camera permission the tile asks for it;
 * photo access is asked for once, the first time the sheet opens. [room] is how many more images
 * the message can take.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AttachSheet(room: Int, onDismiss: () -> Unit, onAllPhotos: () -> Unit, onFiles: () -> Unit, onImages: (List<String>) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var allowed by remember { mutableStateOf(cameraAllowed(context)) }
    var camera by remember { mutableStateOf(false) }
    var recent by remember { mutableStateOf(listOf<Uri>()) }
    var picked by remember { mutableStateOf(listOf<Uri>()) }
    var adding by remember { mutableStateOf(false) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        allowed = granted
        if (granted) camera = true
    }
    suspend fun loadRecent() {
        if (photosAllowed(context)) recent = withContext(Dispatchers.IO) { recentPhotos(context, MAX_IMAGES) }
    }
    val askPhotos = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        scope.launch { loadRecent() }
    }
    LaunchedEffect(Unit) {
        val prefs = context.getSharedPreferences("photos", Context.MODE_PRIVATE)
        if (!photosAllowed(context) && !prefs.getBoolean("asked", false)) {
            prefs.edit().putBoolean("asked", true).apply()
            askPhotos.launch(photoPermissions())
        } else loadRecent()
    }
    if (camera) {
        ChatCamera(room, onClose = { camera = false }, onDone = { onImages(it); onDismiss() })
        return
    }
    fun addPicked() {
        adding = true
        scope.launch {
            val encoded = withContext(Dispatchers.IO) { picked.mapNotNull { encodeImageUri(context, it) } }
            onImages(encoded)
            onDismiss()
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, containerColor = Beep.colors.page) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            val cameraTile: @Composable (Modifier) -> Unit = { tile ->
                if (allowed) Box(tile.clickable(onClickLabel = "Take a photo") { camera = true }) {
                    CameraFeed(CameraSelector.LENS_FACING_BACK, null, Modifier.fillMaxSize())
                    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.4f)), contentAlignment = Alignment.Center) {
                        Icon(Icons.Outlined.PhotoCamera, "Take a photo", tint = Color.White)
                    }
                } else Column(tile.background(Beep.colors.tints[1]).border(1.5.dp, Beep.colors.ledge, RoundedCornerShape(Beep.shapes.smallRadius))
                    .clickable { ask.launch(Manifest.permission.CAMERA) }.padding(6.dp),
                    verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(Icons.Outlined.PhotoCamera, null, tint = Beep.colors.ink)
                    Text("Allow camera", fontSize = 11.sp, color = Beep.colors.ink, textAlign = TextAlign.Center)
                }
            }
            // On top, so a short window like the bubble still shows them; the photos scroll under.
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                ChunkyButton(onClick = { onAllPhotos(); onDismiss() }, modifier = Modifier.weight(1f)) {
                    Icon(Icons.Outlined.Image, null, Modifier.size(20.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("All photos", fontWeight = FontWeight.SemiBold)
                }
                if (picked.isNotEmpty()) ChunkyButton(onClick = { addPicked() }, modifier = Modifier.weight(1f),
                    color = Beep.colors.accent, contentColor = Beep.colors.ink, enabled = !adding) {
                    Text(if (adding) "Adding…" else "Add ${picked.size}", fontWeight = FontWeight.SemiBold)
                } else ChunkyButton(onClick = { onFiles(); onDismiss() }, modifier = Modifier.weight(1f)) {
                    Icon(Icons.Outlined.Description, null, Modifier.size(20.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Files", fontWeight = FontWeight.SemiBold)
                }
            }
            val tiles = listOf<Uri?>(null) + recent
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                tiles.chunked(3).forEach { row ->
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        row.forEach { uri ->
                            val tile = Modifier.weight(1f).aspectRatio(1f).clip(RoundedCornerShape(Beep.shapes.smallRadius))
                            if (uri == null) cameraTile(tile) else {
                                val number = picked.indexOf(uri).takeIf { it >= 0 }?.plus(1)
                                Box(tile.clickable(onClickLabel = if (number == null) "Pick photo" else "Unpick photo") {
                                    picked = if (number != null) picked - uri else if (picked.size < room) picked + uri else picked
                                }) {
                                    PhotoThumb(uri, Modifier.fillMaxSize())
                                    if (number != null) {
                                        Box(Modifier.fillMaxSize().border(3.dp, Beep.colors.brand, RoundedCornerShape(Beep.shapes.smallRadius)))
                                        Box(Modifier.align(Alignment.TopEnd).padding(5.dp).size(20.dp).background(Beep.colors.brand, CircleShape),
                                            contentAlignment = Alignment.Center) {
                                            Text("$number", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Color.White)
                                        }
                                    }
                                }
                            }
                        }
                        repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
                if (!allowed) Text("Beep only uses the camera while it's open here. Photos you take are sent with your message and not saved to the phone.",
                    fontSize = 12.5.sp, lineHeight = 18.sp, color = Beep.colors.muted)
            }
        }
    }
}

/**
 * The camera inside the chat, full screen. Each shot adds to the count; the last one can be
 * looked at and retaken, and Done puts them all in the composer. Nothing is saved to the phone.
 */
@Composable
private fun ChatCamera(room: Int, onClose: () -> Unit, onDone: (List<String>) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var lens by remember { mutableIntStateOf(CameraSelector.LENS_FACING_BACK) }
    var flash by remember { mutableStateOf(false) }
    var zoom by remember { mutableFloatStateOf(1f) }
    var camera by remember { mutableStateOf<Camera?>(null) }
    var shots by remember { mutableStateOf(listOf<Bitmap>()) }
    var reviewing by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val capture = remember(lens) { ImageCapture.Builder().setCaptureMode(ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY).build() }
    LaunchedEffect(flash, capture) { capture.flashMode = if (flash) ImageCapture.FLASH_MODE_ON else ImageCapture.FLASH_MODE_OFF }
    LaunchedEffect(zoom, camera) { camera?.cameraControl?.setZoomRatio(zoom) }
    fun finish() {
        busy = true
        scope.launch {
            val encoded = withContext(Dispatchers.Default) { shots.mapNotNull { encodeBitmap(it, 1024) } }
            onDone(encoded)
        }
    }
    fun shoot() {
        if (busy || shots.size >= room) return
        busy = true
        capture.takePicture(ContextCompat.getMainExecutor(context), object : ImageCapture.OnImageCapturedCallback() {
            override fun onCaptureSuccess(image: ImageProxy) {
                val picture = runCatching { image.upright() }.getOrNull()
                image.close()
                busy = false
                if (picture != null) shots = shots + picture
            }
            override fun onError(exception: ImageCaptureException) { busy = false }
        })
    }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        BackHandler(enabled = reviewing) { reviewing = false }
        val last = shots.lastOrNull()
        if (reviewing && last != null) {
            Column(Modifier.fillMaxSize().background(Color.Black).systemBarsPadding()) {
                Row(Modifier.fillMaxWidth().height(56.dp).padding(start = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = { reviewing = false }) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, "Back to the camera", tint = Color.White) }
                    Text("Photo ${shots.size}", fontSize = 14.sp, color = Color(0xFFCCCCCC))
                }
                Image(last.asImageBitmap(), "Photo ${shots.size}", Modifier.weight(1f).fillMaxWidth().padding(bottom = 8.dp), contentScale = ContentScale.Fit)
                Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 22.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(onClick = { shots = shots.dropLast(1); reviewing = false }, Modifier.weight(1f).height(48.dp),
                        border = androidx.compose.foundation.BorderStroke(1.5.dp, Color.White)) { Text("Retake", color = Color.White) }
                    ChunkyButton(onClick = { finish() }, Modifier.weight(1f), color = Beep.colors.accent, contentColor = Beep.colors.ink, enabled = !busy) {
                        Text(if (shots.size == 1) "Use photo" else "Use ${shots.size} photos", fontWeight = FontWeight.SemiBold)
                    }
                }
            }
            return@Dialog
        }
        Column(Modifier.fillMaxSize().background(Color.Black).systemBarsPadding()) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                key(lens) { CameraFeed(lens, capture, Modifier.fillMaxSize()) { camera = it; zoom = 1f } }
                Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onClose) { Icon(Icons.Outlined.Close, "Close camera", tint = Color.White) }
                    Spacer(Modifier.weight(1f))
                    if (camera?.cameraInfo?.hasFlashUnit() == true) IconButton(onClick = { flash = !flash }) {
                        Icon(if (flash) Icons.Outlined.FlashOn else Icons.Outlined.FlashOff, if (flash) "Flash on" else "Flash off", tint = Color.White)
                    }
                }
                val range = camera?.cameraInfo?.zoomState?.value
                val stops = listOf(0.5f, 1f, 2f).filter { range == null || it in range.minZoomRatio..range.maxZoomRatio }
                if (stops.size > 1) Row(Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    stops.forEach { stop ->
                        Text(if (stop == 0.5f) "0.5×" else "${stop.toInt()}×", Modifier.clip(CircleShape).background(Color.Black.copy(alpha = 0.35f))
                            .clickable { zoom = stop }.padding(horizontal = 10.dp, vertical = 6.dp),
                            fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = if (zoom == stop) Beep.colors.accent else Color.White.copy(alpha = 0.6f))
                    }
                }
                if (shots.isNotEmpty()) TextButton(onClick = { finish() }, enabled = !busy,
                    modifier = Modifier.align(Alignment.BottomEnd).padding(end = 16.dp, bottom = 10.dp),
                    colors = ButtonDefaults.textButtonColors(containerColor = Color.Black.copy(alpha = 0.55f), contentColor = Color.White)) { Text("Done") }
            }
            Row(Modifier.fillMaxWidth().padding(start = 28.dp, end = 28.dp, top = 18.dp, bottom = 26.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Box(Modifier.size(48.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius)).border(2.dp, Color.White, RoundedCornerShape(Beep.shapes.smallRadius))
                    .clickable(enabled = last != null, onClickLabel = "Look at the last photo") { reviewing = true }) {
                    last?.let { Image(it.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
                    if (shots.isNotEmpty()) Box(Modifier.align(Alignment.BottomEnd).padding(2.dp).sizeIn(minWidth = 18.dp).height(18.dp)
                        .background(Beep.colors.accent, CircleShape), contentAlignment = Alignment.Center) {
                        Text("${shots.size}", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = Beep.colors.ink)
                    }
                }
                Box(Modifier.size(72.dp).border(4.dp, Color.White, CircleShape).clip(CircleShape)
                    .clickable(enabled = !busy && shots.size < room, onClickLabel = "Take photo") { shoot() },
                    contentAlignment = Alignment.Center) {
                    Box(Modifier.size(58.dp).background(if (shots.size < room) Color.White else Color.White.copy(alpha = 0.4f), CircleShape))
                }
                IconButton(onClick = {
                    lens = if (lens == CameraSelector.LENS_FACING_BACK) CameraSelector.LENS_FACING_FRONT else CameraSelector.LENS_FACING_BACK
                }, Modifier.size(48.dp).background(Color.White.copy(alpha = 0.15f), CircleShape)) {
                    Icon(Icons.Outlined.Cameraswitch, "Switch camera", tint = Color.White)
                }
            }
        }
    }
}
