//! iOS: the share sheet for a saved file, so a video or picture can go to
//! Photos, Files or another app.

use objc2::{class, msg_send, runtime::AnyObject};
use std::{ffi::c_void, path::Path};

/// Presents the share sheet for `path` over `controller`, the web view's
/// `UIViewController`. Call on the main thread.
pub fn share(controller: *mut c_void, path: &Path) {
    let Some(path) = path.to_str() else { return };
    let Ok(path) = std::ffi::CString::new(path) else {
        return;
    };

    // SAFETY: Tauri hands the window's live view controller and this runs on
    // the main thread. Each message is UIKit or Foundation API called with
    // the argument types it declares; `alloc` and `init...` return an owned
    // controller that `presentViewController:` retains, so it is released
    // after presenting. The popover anchor keeps an iPad from raising.
    unsafe {
        let Some(controller) = (controller as *const AnyObject).as_ref() else {
            return;
        };
        let text: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: path.as_ptr()];
        let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: text];
        let items: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: url];
        let sheet: *mut AnyObject = msg_send![class!(UIActivityViewController), alloc];
        let nothing: *mut AnyObject = std::ptr::null_mut();
        let sheet: *mut AnyObject =
            msg_send![sheet, initWithActivityItems: items, applicationActivities: nothing];
        let Some(sheet) = sheet.as_ref() else { return };

        let view: *mut AnyObject = msg_send![controller, view];
        let popover: *mut AnyObject = msg_send![sheet, popoverPresentationController];
        if let Some(popover) = popover.as_ref() {
            let _: () = msg_send![popover, setSourceView: view];
        }

        let done: *mut AnyObject = std::ptr::null_mut();
        let _: () =
            msg_send![controller, presentViewController: sheet, animated: true, completion: done];
        let _: () = msg_send![sheet, release];
    }
}
