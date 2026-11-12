//! Background task notifications and stop requests from ACP extensions.

use agent_client_protocol::{
    UntypedMessage,
    v1::schema::{CLIENT_METHOD_NAMES, SessionId},
};
use serde::Deserialize;
use serde_json::json;

/// An `async_task_*` update of `session/update`, which the crate's typed
/// `SessionUpdate` does not know. The names are the adapter's.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "sessionUpdate",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(in crate::agents) enum AsyncTask {
    #[serde(rename = "async_task_spawned")]
    Spawned {
        async_task_id: String,
        #[serde(default)]
        name: String,
        /// `turn` for a turn the agent started on its own, which is not a
        /// command.
        #[serde(default)]
        task_type: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        can_stop: bool,
        #[serde(default)]
        output_file_path: Option<String>,
        #[serde(default)]
        tool_call_id: Option<String>,
    },
    #[serde(rename = "async_task_progress")]
    Progress {
        async_task_id: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        output_file_path: Option<String>,
        #[serde(default)]
        tool_call_id: Option<String>,
    },
    #[serde(rename = "async_task_state_update")]
    StateUpdate {
        async_task_id: String,
        state: String,
        #[serde(default)]
        summary: Option<String>,
        #[serde(default)]
        output_file_path: Option<String>,
        #[serde(default)]
        tool_call_id: Option<String>,
    },
}

/// The `async_task_*` update a notification carries, when it is one.
pub(in crate::agents) fn async_task(message: &UntypedMessage) -> Option<AsyncTask> {
    if message.method != CLIENT_METHOD_NAMES.session_update {
        return None;
    }

    let update = message.params.get("update")?;
    let name = update.get("sessionUpdate")?.as_str()?;

    if !name.starts_with("async_task_") {
        return None;
    }

    match serde_json::from_value(update.clone()) {
        Ok(task) => Some(task),
        Err(e) => {
            log::debug!("an {name} update was not read: {e}");
            None
        }
    }
}

/// `_session/async_task/stop`: stops one background task of the session
/// without cancelling its turn. The answer says whether it stopped.
pub(in crate::agents) fn stop_task(
    session: &SessionId,
    task: &str,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    UntypedMessage::new(
        "_session/async_task/stop",
        json!({"sessionId": session, "asyncTaskId": task}),
    )
}
