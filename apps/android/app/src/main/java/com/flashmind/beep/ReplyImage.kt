package com.flashmind.beep

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
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

/**
 * An image the assistant put in its reply as `![alt](url)`. Tapping a web image opens it
 * in the browser; one that fails to load falls back to a link.
 */
@Composable
internal fun ReplyImage(url: String, alt: String, compact: Boolean) {
    val uriHandler = LocalUriHandler.current
    val state by produceState<Loaded>(Loaded.Loading, url) {
        value = ReplyImages.load(url)?.let { Loaded.Ok(it) } ?: Loaded.Failed
    }
    val web = url.startsWith("https://") || url.startsWith("http://")
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    when (val s = state) {
        is Loaded.Ok -> Image(s.bitmap.asImageBitmap(), alt.ifBlank { null },
            contentScale = ContentScale.FillWidth,
            modifier = Modifier.fillMaxWidth().heightIn(max = if (compact) 220.dp else 360.dp).clip(shape)
                .then(if (web) Modifier.clickable { runCatching { uriHandler.openUri(url) } } else Modifier))
        Loaded.Loading -> Box(Modifier.fillMaxWidth().heightIn(min = if (compact) 80.dp else 140.dp).clip(shape)
            .background(Beep.colors.code))
        Loaded.Failed -> Text(alt.ifBlank { if (web) url else "Image" }, fontSize = if (compact) 14.sp else 16.sp,
            color = Beep.colors.brand, textDecoration = if (web) TextDecoration.Underline else null,
            modifier = Modifier.padding(vertical = 2.dp)
                .then(if (web) Modifier.clickable { runCatching { uriHandler.openUri(url) } } else Modifier))
    }
}

private object ReplyImages {
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
