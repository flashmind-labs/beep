//! The Assistant page's local chat: a conversation with an OpenRouter model on this Mac, run
//! with the OpenRouter and Firecrawl keys saved in Settings. The client, the tool loop and the
//! chat files are in `beep_assistant`; this lends it the keys, the folder and read-only questions
//! about the user's work (`work`), and draws what it streams as `BarEvent`s. It needs no server:
//! the server's chat code is untouched and runs only when `SERVER_ENABLED` is on.

pub(crate) mod commands;
mod runs;
mod work;

pub(crate) use runs::Runs;
