//! Keep the Space UUID across restarts. WindowServer may reassign numeric Space IDs.
use std::ffi::c_void;
use tauri::{Runtime, Window};
use uuid::Uuid;

pub(super) type Desktop = Uuid;

unsafe extern "C" {
    fn beep_window_space(window: *mut c_void, uuid: *mut u8) -> bool;
    fn beep_place_window(window: *mut c_void, uuid: *const u8);
}

pub(super) fn current<R: Runtime>(window: &Window<R>) -> Option<Desktop> {
    let native = window.ns_window().ok()?;
    let mut bytes = [0; 16];
    // SAFETY: Tauri owns this live NSWindow on the UI thread; the output holds a UUID.
    unsafe { beep_window_space(native, bytes.as_mut_ptr()) }.then(|| Uuid::from_bytes(bytes))
}

pub(super) fn place<R: Runtime>(window: &Window<R>, desktop: &Desktop) {
    if let Ok(native) = window.ns_window() {
        // SAFETY: the NSWindow and the 16 input bytes are live for this synchronous call.
        unsafe { beep_place_window(native, desktop.as_bytes().as_ptr()) };
    }
}
