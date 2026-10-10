package com.flashmind.beep

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.TextButton
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.launch
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL

private sealed class Loaded {
    data object Loading : Loaded()
    data object Failed : Loaded()
    data class Ok(val bitmap: Bitmap) : Loaded()
}

/** An image in a reply: where it loads from and what it shows. */
internal data class ReplyPicture(val url: String, val alt: String) {
    val web get() = url.startsWith("https://") || url.startsWith("http://")
}

/** A loaded image as the full screen viewer shows it. */
private data class Shown(val picture: ReplyPicture, val bitmap: Bitmap)

/**
 * An image the assistant put in its reply as `![alt](url)`. Tapping it opens it full
 * screen; one that fails to load falls back to a link.
 */
@Composable
internal fun ReplyImage(url: String, alt: String, compact: Boolean) {
    val picture = ReplyPicture(url, alt)
    val state by produceState<Loaded>(Loaded.Loading, url) {
        value = ReplyImages.load(url)?.let { Loaded.Ok(it) } ?: Loaded.Failed
    }
    var viewing by remember(url) { mutableStateOf(false) }
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    when (val s = state) {
        is Loaded.Ok -> {
            Image(s.bitmap.asImageBitmap(), alt.ifBlank { null },
                contentScale = ContentScale.FillWidth,
                modifier = Modifier.fillMaxWidth().heightIn(max = if (compact) 220.dp else 360.dp).clip(shape)
                    .clickable { viewing = true })
            if (viewing) ImageViewer(listOf(Shown(picture, s.bitmap)), 0) { viewing = false }
        }
        Loaded.Loading -> Box(Modifier.fillMaxWidth().heightIn(min = if (compact) 80.dp else 140.dp).clip(shape)
            .background(Beep.colors.code))
        Loaded.Failed -> ImageLink(picture, compact)
    }
}

/**
 * Images on consecutive lines as one row that scrolls sideways, each the same height with
 * its width from its shape. Images that fail drop out of the row; if all of them fail the
 * row turns into their links.
 */
@Composable
internal fun ReplyGallery(pictures: List<ReplyPicture>, compact: Boolean) {
    val states = remember(pictures) { mutableStateMapOf<String, Loaded>() }
    LaunchedEffect(pictures) {
        pictures.forEach { picture ->
            launch { states[picture.url] = ReplyImages.load(picture.url)?.let { Loaded.Ok(it) } ?: Loaded.Failed }
        }
    }
    var viewing by remember(pictures) { mutableStateOf<Int?>(null) }
    if (pictures.all { states[it.url] == Loaded.Failed }) {
        Column { pictures.forEach { ImageLink(it, compact) } }
        return
    }
    val shown = pictures.mapNotNull { p -> (states[p.url] as? Loaded.Ok)?.let { Shown(p, it.bitmap) } }
    val height = if (compact) 140.dp else 200.dp
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        pictures.forEach { picture ->
            when (val s = states[picture.url] ?: Loaded.Loading) {
                Loaded.Failed -> {}
                Loaded.Loading -> Box(Modifier.size(height * 1.3f, height).clip(shape).background(Beep.colors.code))
                is Loaded.Ok -> {
                    val ratio = (s.bitmap.width.toFloat() / s.bitmap.height).coerceIn(0.5f, 1.8f)
                    val width = height * ratio
                    Column(Modifier.width(width)) {
                        Image(s.bitmap.asImageBitmap(), picture.alt.ifBlank { null }, contentScale = ContentScale.Crop,
                            modifier = Modifier.size(width, height).clip(shape)
                                .clickable { viewing = shown.indexOfFirst { it.picture == picture } })
                        if (picture.alt.isNotBlank()) Text(picture.alt, fontSize = 12.sp, lineHeight = 16.sp, color = Beep.colors.muted,
                            maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 4.dp))
                    }
                }
            }
        }
    }
    viewing?.let { start -> if (start >= 0) ImageViewer(shown, start) { viewing = null } }
}

/** What is left of an image that did not load: its description, as a link when it came from the web. */
@Composable
private fun ImageLink(picture: ReplyPicture, compact: Boolean) {
    val uriHandler = LocalUriHandler.current
    Text(picture.alt.ifBlank { if (picture.web) picture.url else "Image" }, fontSize = if (compact) 14.sp else 16.sp,
        color = Beep.colors.brand, textDecoration = if (picture.web) TextDecoration.Underline else null,
        modifier = Modifier.padding(vertical = 2.dp)
            .then(if (picture.web) Modifier.clickable { runCatching { uriHandler.openUri(picture.url) } } else Modifier))
}

/** Full screen images, swiped through in order, with Open for the page an image came from. */
@Composable
private fun ImageViewer(images: List<Shown>, start: Int, onClose: () -> Unit) {
    val uriHandler = LocalUriHandler.current
    val pager = rememberPagerState(initialPage = start) { images.size }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            HorizontalPager(pager, Modifier.fillMaxSize()) { page ->
                Image(images[page].bitmap.asImageBitmap(), images[page].picture.alt.ifBlank { null },
                    contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
            }
            val current = images[pager.currentPage].picture
            Row(Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onClose) { Icon(Icons.Outlined.Close, "Close", tint = Color.White) }
                if (images.size > 1) Text("${pager.currentPage + 1} of ${images.size}", color = Color.White, fontSize = 14.sp)
                Spacer(Modifier.weight(1f))
                if (current.web) TextButton(onClick = { runCatching { uriHandler.openUri(current.url) } }) {
                    Text("Open", color = Color.White, fontWeight = FontWeight.SemiBold)
                }
            }
            if (current.alt.isNotBlank()) Text(current.alt, color = Color.White, fontSize = 14.sp, lineHeight = 20.sp,
                modifier = Modifier.align(Alignment.BottomStart).fillMaxWidth().background(Color.Black.copy(alpha = 0.5f))
                    .navigationBarsPadding().padding(horizontal = 16.dp, vertical = 12.dp))
        }
    }
}

internal object ReplyImages {
    private const val MAX_BYTES = 10 * 1024 * 1024
    private const val MAX_SIDE = 1600

    // Sized in bytes so a few large photos do not crowd out everything else.
    private val cache = object : LruCache<String, Bitmap>(32 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap) = value.byteCount
    }

    suspend fun load(url: String): Bitmap? = cache.get(url) ?: withContext(Dispatchers.IO) {
        runCatching {
            val bytes = when {
                url.startsWith("data:image/") -> android.util.Base64.decode(url.substringAfter(",", ""), android.util.Base64.DEFAULT)
                url.startsWith("https://") -> download(url)
                else -> null
            } ?: return@runCatching null
            decode(bytes)
        }.getOrNull()?.also { cache.put(url, it) }
    }

    private fun download(url: String): ByteArray? {
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = 10000
        connection.readTimeout = 15000
        connection.setRequestProperty("User-Agent", "Beep for Android")
        try {
            if (connection.responseCode !in 200..299) return null
            if (connection.contentLengthLong > MAX_BYTES) return null
            val out = ByteArrayOutputStream()
            connection.inputStream.use { input ->
                val buf = ByteArray(16 * 1024)
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > MAX_BYTES) return null
                }
            }
            return out.toByteArray()
        } finally { connection.disconnect() }
    }

    /** Decodes at a power-of-two scale that keeps the longer side near [MAX_SIDE]. */
    private fun decode(bytes: ByteArray): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= MAX_SIDE) sample *= 2
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
    }
}
