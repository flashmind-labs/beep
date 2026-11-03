//! `tools/1`: the Beep MCP server's tools, which coding agents on a node
//! call to work across the user's machines. Unlike the other protocols the
//! node opens these streams, on its connection to the server, and only
//! when the welcome said the server takes them (`Down::Welcome::tools`).
//! Each stream starts with `Open`, then carries one `Request` and its
//! `Reply`. The server runs the call on the machine it names, so the
//! user's rules for their machines apply as for Beep's own tools.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "tools/1";

/// The tools the server runs. `ask_user` is not among them: the node puts
/// that question to the user itself.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Tool {
    ListMachines,
    RunOn,
    SyncProject,
    SendFiles,
    JobStatus,
    Cancel,
    /// A tool a newer node calls that this server does not know.
    #[serde(other)]
    Unknown,
}

/// What the user said to a call that asked first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approved {
    /// Not asked yet: the server checks the user's rules.
    #[default]
    No,
    /// The user allowed this one call.
    Once,
    /// The user allowed it from now on: the server keeps that in the rules.
    Always,
}

/// What the node sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The node's id of the agent session that called the tool.
    pub session: String,
    pub tool: Tool,
    /// The call's arguments as the agent sent them, as JSON text.
    pub args: String,
    #[serde(default)]
    pub approved: Approved,
}

/// One argument an approval shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub label: String,
    pub value: String,
}

/// What the server answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The call ran: `text` is its result for the agent, `failed` when it
    /// did not work.
    Done { text: String, failed: bool },
    /// The user's rules want them asked first. Send the request again with
    /// `approved` once they allow it.
    Ask {
        /// The app the call acts in, such as "Your machines".
        app: String,
        /// What it does, such as "run on".
        action: String,
        fields: Vec<Field>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(value: &T) -> T {
        let bytes = rmp_serde::to_vec_named(value).expect("encodes");
        rmp_serde::from_slice(&bytes).expect("decodes")
    }

    #[test]
    fn requests_and_replies_round_trip() {
        let request = Request {
            session: "a3f9c1".into(),
            tool: Tool::RunOn,
            args: r#"{"machine":"build-box","command":"cargo test"}"#.into(),
            approved: Approved::Once,
        };
        assert_eq!(round_trip(&request), request);
        let ask = Reply::Ask {
            app: "Your machines".into(),
            action: "run on".into(),
            fields: vec![Field {
                label: "Machine".into(),
                value: "build-box".into(),
            }],
        };
        assert_eq!(round_trip(&ask), ask);
        let name: &'static str = Tool::SendFiles.into();
        assert_eq!(name, "send_files");
        assert_eq!("job_status".parse::<Tool>(), Ok(Tool::JobStatus));
    }

    #[test]
    fn an_unknown_tool_and_a_missing_approval_still_read() {
        #[derive(Serialize)]
        struct Newer {
            session: &'static str,
            tool: &'static str,
            args: &'static str,
        }
        let bytes = rmp_serde::to_vec_named(&Newer {
            session: "a3f9c1",
            tool: "schedule",
            args: "{}",
        })
        .expect("encodes");
        let request: Request = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(request.tool, Tool::Unknown);
        assert_eq!(request.approved, Approved::No);
    }
}
