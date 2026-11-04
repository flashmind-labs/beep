//! The bar: one borderless window that shows and hides in place, and the
//! commands its page and the orb's call. Only the user opens it.

pub(crate) mod app;
pub(crate) mod live;
pub(crate) mod pasted_image;
pub(crate) mod window;

pub(crate) use window::{LABEL, dismiss, hide, is_focused, is_open, show, toggle};
