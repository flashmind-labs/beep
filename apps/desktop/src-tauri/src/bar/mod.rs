//! The bar: one borderless window that shows and hides in place on two taps
//! of Option, and the commands its page calls. Only the user opens it.

pub(crate) mod app;
#[cfg(desktop)]
pub(crate) mod hotkey;
pub(crate) mod pasted_image;
pub(crate) mod window;
pub(crate) mod zoom;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use window::toggle;
pub(crate) use window::{LABEL, hide, is_open, show};
