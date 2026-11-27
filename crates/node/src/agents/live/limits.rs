//! What a session's live state keeps and cuts.

use agent_client_protocol::v1::schema::ToolKind;

/// The most transcript entries kept; older ones are dropped.
pub(super) const MAX_ENTRIES: usize = 5000;
/// The longest entry kept.
pub(super) const MAX_ENTRY: usize = 16 * 1024;
/// The longest last message sent to the caller.
pub(super) const MAX_LAST_MESSAGE: usize = 4 * 1024;
/// The most files and plan entries reported.
pub(super) const MAX_FILES: usize = 50;
pub(super) const MAX_PLAN: usize = 30;
/// The most commands kept of those the agent advertises.
pub(super) const MAX_COMMANDS: usize = 200;
/// The longest command description kept; skills describe themselves at
/// length.
pub(super) const MAX_DESCRIPTION: usize = 240;
/// The most compactions remembered, so an update of one is shown once.
pub(super) const MAX_COMPACTIONS: usize = 100;
/// The longest summary or question detail.
pub(super) const MAX_LINE: usize = beep_proto::node::agent::MAX_QUESTION_LINE;
pub(in crate::agents) use crate::logs::window::{ARTIFACT_TOOL, SKILL_TOOL};

/// The `_meta` key claude-agent-acp forwards Claude Code's rate limit
/// events under, in a `usage_update`.
pub(super) const CLAUDE_RATE_LIMIT: &str = "_claude/rateLimit";

/// The most background tasks kept.
pub(super) const MAX_TASKS: usize = 20;

/// The most of a thought kept to find its line.
pub(super) const MAX_THOUGHT: usize = 8 * 1024;

/// The most of a subagent's steps its entry names; it counts them all.
pub(super) const MAX_STEPS: usize = 60;

/// ACP tool kinds that change files.
pub(super) const EDIT_KINDS: &[ToolKind] = &[ToolKind::Edit, ToolKind::Delete, ToolKind::Move];
