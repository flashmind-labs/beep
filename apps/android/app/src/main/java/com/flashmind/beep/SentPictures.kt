package com.flashmind.beep

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.flashmind.beep.ffi.BeepException
import com.flashmind.beep.proto.ImageRef
import com.flashmind.beep.proto.SentImages
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/** Pictures the user sent, read from the server once and kept in the cache under their id. */
object SentPictures {
    private const val FOLDER = "pictures"
    /** Wide enough for a bubble on any phone; larger pictures decode smaller. */
    private const val MAX_SIDE = 1280

    suspend fun load(context: Context, picture: ImageRef): Bitmap? = withContext(Dispatchers.IO) {
        val file = fetch(context, picture)
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= MAX_SIDE) sample *= 2
        BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
    }

    /** [picture] as the data URI it was sent as, so a rerun can send it again. */
    suspend fun dataUri(context: Context, picture: ImageRef): String = withContext(Dispatchers.IO) {
        "data:${picture.mime};base64," + Base64.encodeToString(fetch(context, picture).readBytes(), Base64.NO_WRAP)
    }

    private suspend fun fetch(context: Context, picture: ImageRef): File {
        val dir = File(context.cacheDir, FOLDER).apply { mkdirs() }
        val file = File(dir, picture.id)
        if (!file.exists()) {
            val client = BeepClients.current() ?: throw BeepException.Connection("not connected to your server")
            client.saveFile(picture.id, file.path)
        }
        return file
    }
}

/** The refs by the message they came with: a user turn's timestamp, or a group message's seq. */
internal fun picturesByMessage(sent: List<SentImages>): Map<Long, List<ImageRef>> =
    sent.associate { it.message to it.images }

private sealed interface PictureState {
    data object Loading : PictureState
    data object Gone : PictureState
    data class Loaded(val bitmap: Bitmap) : PictureState
}

/** One sent picture: a tile while it loads, and a note when the server no longer has it. */
@Composable
private fun SentPicture(picture: ImageRef, description: String, modifier: Modifier, fill: Boolean) {
    val context = LocalContext.current
    var shown by remember(picture.id) { mutableStateOf<PictureState>(PictureState.Loading) }
    LaunchedEffect(picture.id) {
        shown = runCatching { SentPictures.load(context, picture) }.getOrNull()?.let { PictureState.Loaded(it) } ?: PictureState.Gone
    }
    val shape = RoundedCornerShape(Beep.shapes.smallRadius)
    when (val now = shown) {
        is PictureState.Loaded -> Image(now.bitmap.asImageBitmap(), description,
            contentScale = if (fill) ContentScale.FillWidth else ContentScale.Crop, modifier = modifier)
        PictureState.Loading -> Box(modifier.clip(shape).background(Beep.colors.surface))
        PictureState.Gone -> Box(modifier.clip(shape).border(1.dp, Beep.colors.muted, shape).alpha(0.5f).padding(6.dp),
            contentAlignment = Alignment.Center) {
            Text("Picture no longer available", fontSize = 11.sp, color = Beep.colors.muted, textAlign = TextAlign.Center)
        }
    }
}

/** A message's sent pictures, laid out as attached images are: one fills the bubble, several sit in a row. */
@Composable
internal fun SentPictureRow(pictures: List<ImageRef>) {
    if (pictures.size == 1) {
        val picture = pictures[0]
        val ratio = if (picture.width > 0u && picture.height > 0u) picture.width.toFloat() / picture.height.toFloat() else 1f
        SentPicture(picture, "Attached image", Modifier.fillMaxWidth().heightIn(max = 200.dp).aspectRatio(ratio), fill = true)
    } else if (pictures.size > 1) {
        Row(Modifier.horizontalScroll(rememberScrollState()).padding(6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            pictures.forEachIndexed { index, picture ->
                SentPicture(picture, "Attached image ${index + 1}", Modifier.size(110.dp).clip(RoundedCornerShape(Beep.shapes.smallRadius)), fill = false)
            }
        }
    }
}
