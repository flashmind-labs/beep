//! The types `Live` holds as fields, apart from the code that works on them.

use crate::calls::Tool as BeepTool;
use crate::logs::subagent::Subagent;
use agent_client_protocol::{v1::schema::{ToolCallId, ToolCallStatus, ToolKind}, v2::schema as v2};
use beep_proto::AgentToolCall;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(super) struct ToolCall {
    /// Its entry, counted from the start of the session.
    pub(super) entry: u64,
    pub(super) title: String,
    pub(super) kind: ToolKind,
    pub(super) status: ToolCallStatus,
    pub(super) paths: Vec<String>,
    /// What it showed, already cut to an entry's length.
    pub(super) output: String,
    /// The image lines of the pictures it showed.
    pub(super) pictures: Vec<String>,
    /// The subagent's tool call this one ran in, whose entry shows it.
    pub(super) parent: Option<ToolCallId>,
    /// The calls a subagent this tool started ran, in order.
    pub(super) steps: Vec<ToolCallId>,
    /// The subagent it started, when the call says it starts one.
    pub(super) subagent: Option<Subagent>,
    /// A call to the node's own `beep` server, titled from its arguments.
    pub(super) beep: Option<BeepCall>,
    /// Its command went to the background and still runs, though the call
    /// itself completed.
    pub(super) backgrounded: bool,
    /// The skill a Claude Code `Skill` call loads.
    pub(super) skill: Option<String>,
    /// How far the files a `send_files` call sends have gone.
    pub(super) sent: Option<Sent>,
    pub(super) details: AgentToolCall,
}

/// The bytes a `send_files` call has sent, of all it sends. Its entry
/// shows them as `[sent] <done> <total>` while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sent {
    pub(crate) done: u64,
    pub(crate) total: u64,
}

/// A `beep` server call and its arguments as far as they have come.
#[derive(Debug)]
pub(super) struct BeepCall {
    pub(super) tool: BeepTool,
    pub(super) input: Option<Value>,
}

#[derive(Debug, Default)]
pub(super) struct Projection {
    pub(super) messages: HashMap<v2::MessageId, Message>,
    pub(super) tools: HashMap<v2::ToolCallId, v2::ToolCallUpdate>,
    pub(super) terminals: HashMap<v2::TerminalId, (u64, v2::TerminalUpdate)>,
    pub(super) pending: Option<u64>,
    pub(super) last_agent: Option<u64>,
    pub(super) pruned: u64,
    pub(super) deferred: Vec<v2::SessionUpdate>,
}

#[derive(Debug)]
pub(super) struct Message {
    pub(super) entry: u64,
    pub(super) optimistic: bool,
}
