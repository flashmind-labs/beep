//! What the windows show of an agent's replies (Markdown), and
//! the files, pictures and clipboard they hand to the user.

pub(crate) mod clipboard;
pub(crate) mod files;
#[cfg(any(mobile, feature = "rich-text"))]
pub(crate) mod markdown;
#[cfg(not(any(mobile, feature = "rich-text")))]
#[path = "plain.rs"]
pub(crate) mod markdown;
