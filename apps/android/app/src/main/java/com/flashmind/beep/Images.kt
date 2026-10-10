package com.flashmind.beep

import android.content.Context
import android.net.Uri
import android.os.Build
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.util.Base64
import androidx.compose.ui.draw.scale

/** Most images one message may carry; the server refuses more. */
const val MAX_IMAGES = 8

/** The prompt sent with [count] images and no text. */
internal fun imagePrompt(count: Int) = if (count > 1) "What are these?" else "What is this?"

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
        encodeBitmap(bitmap, maxSize)
    } catch (_: Exception) { null }
}

/** Resize to at most [maxSize] px on the longest side and JPEG-encode as a base64 data URI. */
internal fun encodeBitmap(bitmap: Bitmap, maxSize: Int, quality: Int = 80): String? {
    return try {
        val scale = maxSize.toFloat() / maxOf(bitmap.width, bitmap.height).coerceAtLeast(1)
        val resized = if (scale < 1f) {
            Bitmap.createScaledBitmap(bitmap, (bitmap.width * scale).toInt(), (bitmap.height * scale).toInt(), true)
        } else bitmap
        val stream = java.io.ByteArrayOutputStream()
        resized.compress(Bitmap.CompressFormat.JPEG, quality, stream)
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
