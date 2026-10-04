package com.flashmind.beep

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.view.WindowManager.LayoutParams
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.IntentCompat
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject
import java.lang.ref.WeakReference

/**
 * Screenshots of the app behind Beep's bubble, for the agent's read_screen tool.
 * Android asks the user to allow each one; Beep hides its own windows for the frame.
 */
object ScreenCapture {
    /** A little under the server's 60 s wait, so the user's silence is reported as such. */
    private const val WAIT_MS = 55_000L
    private var pending: CompletableDeferred<JSONObject>? = null
    /** The bubble's chat window, made invisible for the moment of the screenshot. */
    private var chat: WeakReference<Activity>? = null
    private var dimmed = false

    fun chatShown(activity: Activity) { chat = WeakReference(activity) }
    fun chatGone(activity: Activity) { if (chat?.get() === activity) chat = null }

    /** `{"image": data URI}`, or `{"error", "message"}` when the user declines or capture fails. */
    suspend fun capture(context: Context): JSONObject = withContext(Dispatchers.Main) {
        if (pending != null) return@withContext error("busy", "A screenshot is already being taken.")
        val result = CompletableDeferred<JSONObject>().also { pending = it }
        try {
            context.startActivity(Intent(context, ScreenCaptureActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            withTimeoutOrNull(WAIT_MS) { result.await() }
                ?: error("no_answer", "The user did not answer Android's screen capture prompt.")
        } finally {
            pending = null
        }
    }

    internal fun done(result: JSONObject) { pending?.complete(result) }

    internal fun error(code: String, message: String): JSONObject = JSONObject().put("error", code).put("message", message)

    /** Hides Beep's bubble and chat panel, or shows them again, without closing or moving them. */
    internal fun hideBeep(hidden: Boolean) {
        Bubble.screenshotting(hidden)
        val window = chat?.get()?.window ?: return
        if (hidden) {
            dimmed = window.attributes.flags and LayoutParams.FLAG_DIM_BEHIND != 0
            window.clearFlags(LayoutParams.FLAG_DIM_BEHIND)
        } else if (dimmed) {
            window.addFlags(LayoutParams.FLAG_DIM_BEHIND)
        }
        window.attributes = window.attributes.apply { alpha = if (hidden) 0f else 1f }
    }
}

/** Shows Android's screen capture prompt and hands an approval to [ScreenCaptureService]. */
class ScreenCaptureActivity : ComponentActivity() {
    private val consent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data
        if (result.resultCode == RESULT_OK && data != null) {
            val bounds = windowManager.maximumWindowMetrics.bounds
            ScreenCaptureService.start(this, ScreenCaptureService.Grant(result.resultCode, data,
                bounds.width(), bounds.height(), resources.displayMetrics.densityDpi))
        } else {
            ScreenCapture.done(ScreenCapture.error("denied", "The user did not allow the screenshot."))
        }
        finish()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) {
            consent.launch(getSystemService(MediaProjectionManager::class.java).createScreenCaptureIntent())
        }
    }
}

/** Takes one frame of the screen. Android only allows screen capture from a foreground service. */
class ScreenCaptureService : Service() {
    class Grant(val code: Int, val data: Intent, val width: Int, val height: Int, val dpi: Int)

    companion object {
        private const val CHANNEL = "beep_screen_capture"
        private const val NOTIFICATION_ID = 98
        /** Time for the capture prompt to close and Beep's windows to fade before the frame. */
        private const val SETTLE_MS = 450L
        private const val FRAME_TIMEOUT_MS = 3_000L
        private const val MAX_SIZE = 1600

        fun start(context: Context, grant: Grant) {
            context.startForegroundService(Intent(context, ScreenCaptureService::class.java)
                .putExtra("code", grant.code).putExtra("data", grant.data)
                .putExtra("width", grant.width).putExtra("height", grant.height).putExtra("dpi", grant.dpi))
        }
    }

    private val main = Handler(Looper.getMainLooper())
    private var projection: MediaProjection? = null
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private var finished = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        finished = false
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Screen reading", NotificationManager.IMPORTANCE_LOW))
        startForeground(NOTIFICATION_ID, Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle("Beep is reading your screen")
            .build(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION)
        val data = intent?.let { IntentCompat.getParcelableExtra(it, "data", Intent::class.java) }
        if (data == null) {
            finish(ScreenCapture.error("no_grant", "The screen capture was not allowed."))
            return START_NOT_STICKY
        }
        val projection = getSystemService(MediaProjectionManager::class.java)
            .getMediaProjection(intent.getIntExtra("code", 0), data)
        if (projection == null) {
            finish(ScreenCapture.error("no_grant", "The screen capture was not allowed."))
            return START_NOT_STICKY
        }
        this.projection = projection
        projection.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() = finish(ScreenCapture.error("stopped", "The screen capture stopped."))
        }, main)
        ScreenCapture.hideBeep(true)
        main.postDelayed({
            grab(projection, intent.getIntExtra("width", 0), intent.getIntExtra("height", 0), intent.getIntExtra("dpi", 0))
        }, SETTLE_MS)
        main.postDelayed({
            finish(ScreenCapture.error("no_frame", "The screen could not be captured."))
        }, SETTLE_MS + FRAME_TIMEOUT_MS)
        return START_NOT_STICKY
    }

    private fun grab(projection: MediaProjection, width: Int, height: Int, dpi: Int) {
        if (finished) return
        val reader = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, 2).also { this.reader = it }
        reader.setOnImageAvailableListener({ source ->
            val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
            val bitmap = image.use {
                // Rows can be padded past the image width.
                val plane = it.planes[0]
                val stride = plane.rowStride / plane.pixelStride
                val padded = Bitmap.createBitmap(stride, it.height, Bitmap.Config.ARGB_8888)
                padded.copyPixelsFromBuffer(plane.buffer)
                Bitmap.createBitmap(padded, 0, 0, it.width, it.height)
            }
            finish(encodeBitmap(bitmap, MAX_SIZE, quality = 85)?.let { JSONObject().put("image", it) }
                ?: ScreenCapture.error("encode", "The screenshot could not be encoded."))
        }, main)
        display = projection.createVirtualDisplay("Beep screen", width, height, dpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, reader.surface, null, main)
    }

    private fun finish(result: JSONObject) {
        if (finished) return
        finished = true
        main.removeCallbacksAndMessages(null)
        ScreenCapture.hideBeep(false)
        ScreenCapture.done(result)
        display?.release()
        reader?.close()
        projection?.stop()
        display = null; reader = null; projection = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }
}
