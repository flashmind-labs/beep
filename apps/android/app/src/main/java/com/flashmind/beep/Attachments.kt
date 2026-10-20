package com.flashmind.beep

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import com.flashmind.beep.proto.SentFiles as SentFileList
import com.flashmind.beep.proto.Upload
import java.io.File
import java.util.UUID

/** The most the files of one message may hold in all; the server refuses more. */
const val MAX_UPLOAD = 5L * 1024 * 1024

/** The prompt sent with [images] pictures and [files] files and no text. */
internal fun attachedPrompt(images: Int, files: Int) = when {
    files == 0 -> imagePrompt(images)
    files + images > 1 -> "Have a look at these files."
    else -> "Have a look at this file."
}

/**
 * Files the user attaches, copied into the app's files so a queued or retried message can
 * still send them, and so the chat can open them before the server lists them.
 */
object Attachments {
    private const val FOLDER = "attached"

    /** Copies [uri] in, or null when it cannot be read. */
    fun copy(context: Context, uri: Uri): ToolCard.File? = runCatching {
        val resolver = context.contentResolver
        val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        } ?: uri.lastPathSegment?.substringAfterLast('/') ?: "file"
        val mime = resolver.getType(uri) ?: "application/octet-stream"
        val dir = File(context.filesDir, "$FOLDER/${UUID.randomUUID()}").apply { mkdirs() }
        val file = File(dir, name.replace('/', '_'))
        resolver.openInputStream(uri)!!.use { input -> file.outputStream().use { input.copyTo(it) } }
        ToolCard.File("", file.name, mime, file.length(), file.path)
    }.getOrNull()

    /** The bytes to send for [files]. */
    fun uploads(files: List<ToolCard.File>): List<Upload> =
        files.map { Upload(it.name, it.mime, File(it.path).readBytes()) }

    /** Removes [file]'s copy. */
    fun delete(file: ToolCard.File) {
        if (file.path.isNotEmpty()) File(file.path).parentFile?.deleteRecursively()
    }

    /** Deletes every copy no longer in [kept], such as files of a chat since reloaded from the server. */
    fun prune(context: Context, kept: Collection<String>) {
        val keep = kept.mapNotNull { File(it).parentFile?.name }.toSet()
        File(context.filesDir, FOLDER).listFiles()?.filter { it.name !in keep }?.forEach { it.deleteRecursively() }
    }
}

/** The files each message carried, by its server timestamp. */
internal fun filesByMessage(sent: List<SentFileList>): Map<Long, List<ToolCard.File>> =
    sent.associate { m -> m.message to m.files.map { ToolCard.File(it.id, it.name, it.mime, it.size.toLong()) } }
