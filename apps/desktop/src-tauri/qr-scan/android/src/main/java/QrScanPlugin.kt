package app.beep.qrscan

import android.app.Activity
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.journeyapps.barcodescanner.CaptureActivity
import com.journeyapps.barcodescanner.ScanIntentResult
import com.journeyapps.barcodescanner.ScanOptions

/** zxing's capture screen, pinned to portrait in the manifest. */
class PortraitCaptureActivity : CaptureActivity()

@TauriPlugin
class QrScanPlugin(private val activity: Activity) : Plugin(activity) {
    /** Opens the camera until it reads a QR code; asks for the camera itself. */
    @Command
    fun scan(invoke: Invoke) {
        val intent = ScanOptions()
            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
            .setBeepEnabled(false)
            .setPrompt("Point the camera at the code on your desktop")
            .setCaptureActivity(PortraitCaptureActivity::class.java)
            .setOrientationLocked(true)
            .createScanIntent(activity)

        startActivityForResult(invoke, intent, "scanned")
    }

    @ActivityCallback
    private fun scanned(invoke: Invoke, result: ActivityResult) {
        val text = ScanIntentResult.parseActivityResult(result.resultCode, result.data).contents
        if (text == null) {
            invoke.reject("cancelled")
            return
        }

        val found = JSObject()
        found.put("content", text)
        invoke.resolve(found)
    }
}
