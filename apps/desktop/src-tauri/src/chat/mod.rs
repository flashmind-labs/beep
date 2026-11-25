//! What the windows show of an agent's replies (Markdown), and
//! the files, pictures and clipboard they hand to the user.

pub(crate) mod clipboard;
#[cfg(desktop)]
pub(crate) mod dictation;
pub(crate) mod files;
#[cfg(desktop)]
pub(crate) mod local_dictation;
pub(crate) mod markdown;
