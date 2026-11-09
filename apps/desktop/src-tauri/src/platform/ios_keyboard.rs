//! iOS: removes the bar WKWebView puts above the keyboard (previous, next and
//! a checkmark). The composer has its own button that puts the keyboard away.

use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
    sel,
};
use std::ffi::{CStr, c_void};

/// The content view's subclass, made once, with no accessory bar.
const NO_BAR: &CStr = c"BeepWKContentViewNoBar";

extern "C-unwind" fn no_bar(_this: *mut AnyObject, _cmd: Sel) -> *mut AnyObject {
    std::ptr::null_mut()
}

/// Gives the content view of `webview`, a `WKWebView`, a class whose
/// `inputAccessoryView` is nil. Call on the main thread.
pub fn hide_bar(webview: *mut c_void) {
    // SAFETY: Tauri hands the window's live WKWebView, and this runs on the
    // main thread. `scrollView`, `subviews`, `count` and `objectAtIndex:` are
    // UIKit API returning what they are declared to; the subclass only adds a
    // method to the class each view already has, so swapping it in keeps the
    // views' layout.
    unsafe {
        let Some(webview) = (webview as *const AnyObject).as_ref() else {
            return;
        };
        let scroll: *mut AnyObject = msg_send![webview, scrollView];
        let Some(scroll) = scroll.as_ref() else {
            return;
        };
        let views: *mut AnyObject = msg_send![scroll, subviews];
        let Some(views) = views.as_ref() else { return };
        let count: usize = msg_send![views, count];

        for i in 0..count {
            let view: *mut AnyObject = msg_send![views, objectAtIndex: i];
            let Some(view) = view.as_ref() else { continue };
            let class = view.class();

            if !class.name().to_bytes().starts_with(b"WKContent") {
                continue;
            }

            let Some(sub) = AnyClass::get(NO_BAR).or_else(|| subclass(class)) else {
                log::warn!("could not hide the keyboard's bar");
                return;
            };
            AnyObject::set_class(view, sub);
        }
    }
}

fn subclass(class: &AnyClass) -> Option<&'static AnyClass> {
    let mut builder = ClassBuilder::new(NO_BAR, class)?;
    // SAFETY: `inputAccessoryView` takes no arguments and returns an object
    // pointer, as `no_bar` does.
    unsafe {
        builder.add_method(
            sel!(inputAccessoryView),
            no_bar as extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject,
        );
    }
    Some(builder.register())
}
