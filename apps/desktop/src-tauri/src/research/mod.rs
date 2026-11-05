//! Research: starting one, listing them and what the team did. A research
//! opens as a page of the command center, which polls the server for it.

pub(crate) mod commands;
pub(crate) mod view;

pub(crate) use commands::Pending;
pub(crate) use view::Cache;
