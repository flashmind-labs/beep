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

/// `value` as the JSON text of a reply.
pub(super) fn json_of<T: Serialize>(value: &T) -> Reply {
    match serde_json::to_string(value) {
        Ok(text) => ok(text),
        Err(e) => fail(format!("Could not write the answer: {e}")),
    }
}

/// A tool's arguments, or the text that refuses them.
pub(super) fn parse<T: DeserializeOwned>(args: &str) -> Result<T, String> {
    serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))
}
