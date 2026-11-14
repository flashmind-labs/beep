//! Scans a QR code on Android with zxing's capture screen, which reads codes
//! off a desktop's screen where the CameraX preview of the barcode plugin
//! could not. The page calls `plugin:qr-scan|scan` and gets `{ content }`.

use tauri::{
    Runtime,
    plugin::{Builder, TauriPlugin},
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("qr-scan")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("app.beep.qrscan", "QrScanPlugin")?;
            Ok(())
        })
        .build()
}
