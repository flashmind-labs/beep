//! Text replies shared by the MCP tool handlers.

use beep_proto::node::tools::Reply;
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn done(text: impl Into<String>, failed: bool) -> Reply {
    Reply::Done {
        text: text.into(),
        failed,
    }
}

pub(super) fn fail(text: impl Into<String>) -> Reply {
    done(text, true)
}

pub(super) fn ok(text: impl Into<String>) -> Reply {
    done(text, false)
}

/// `value` as the JSON text of a reply, marked failed when `failed` is set.
pub(super) fn json_done<T: Serialize>(value: &T, failed: bool) -> Reply {
    match serde_json::to_string(value) {
        Ok(text) => done(text, failed),
        Err(e) => fail(format!("Could not write the answer: {e}")),
    }
}

/// `value` as the JSON text of a reply.
pub(super) fn json_of<T: Serialize>(value: &T) -> Reply {
    json_done(value, false)
}

/// A tool's arguments, or the text that refuses them.
pub(super) fn parse<T: DeserializeOwned>(args: &str) -> Result<T, String> {
    serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))
}
