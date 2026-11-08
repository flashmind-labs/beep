//! Text replies shared by the MCP tool handlers.

use beep_proto::node::tools::Reply;

pub(super) fn fail(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: true,
    }
}

pub(super) fn ok(text: impl Into<String>) -> Reply {
    Reply::Done {
        text: text.into(),
        failed: false,
    }
}
