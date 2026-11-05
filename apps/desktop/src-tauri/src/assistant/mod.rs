//! The local chat: a conversation with an OpenRouter model on this Mac, run with the OpenRouter
//! and Firecrawl keys saved in Settings. The client, the tool loop and the chat files are in
//! `beep_assistant`. `local` is the other data source of the chat commands in `chat::sessions`
//! (the window above them is the same one), `stream` draws a reply as the `BarEvent`s that chat
//! already draws, and `work` answers its read-only questions about the user's work. It needs no
//! server: the server's chat code is untouched and runs when `SERVER_ENABLED` is on.

pub(crate) mod commands;
pub(crate) mod dictation;
pub(crate) mod local;
pub(crate) mod mcp;
pub(crate) mod picker;
mod runs;
mod stream;
mod work;

pub(crate) use runs::Runs;
