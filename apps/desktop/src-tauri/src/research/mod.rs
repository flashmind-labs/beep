//! Research windows: one per research chat, showing the lead's chat, the
//! team's workspace and its agents. The bar starts researches and opens
//! them; the window polls the server for what the team did.

pub(crate) mod commands;
pub(crate) mod view;
mod window;

pub(crate) use commands::Pending;
pub(crate) use view::Cache;
pub(crate) use window::closed;
