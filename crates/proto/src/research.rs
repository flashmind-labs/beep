//! Research runs: a lead agent and a team researching in the background.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A research: a lead agent plans it in its chat, scouts research the
/// plan's questions, a critic challenges them and a writer drafts a paper.
/// The chat holds only the plan and the result; the agents' work is here.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Research {
    pub id: Uuid,
    pub topic: String,
    pub status: ResearchStatus,
    /// The team, lead first. Only the lead until the plan is approved.
    pub agents: Vec<ResearchAgent>,
    /// Log entries after the request's `after`, oldest first.
    pub entries: Vec<ResearchEntry>,
    /// Every source the scouts kept, numbered as the notes and draft cite them.
    pub sources: Vec<ResearchSource>,
    /// The writer's paper in Markdown, empty until it starts.
    pub draft: String,
    /// In milliseconds since the Unix epoch.
    pub started_at: i64,
    /// When the agents stopped, for any reason.
    pub finished_at: Option<i64>,
    /// The newest round, from 1: each follow-up the team researches is one.
    pub round: u32,
}

wire_enum! {
    /// `Planning` while the lead and the user agree on a plan; `Interrupted`
    /// when the server restarted while the agents ran.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ResearchStatus {
        0 Planning {}
        1 Researching {}
        2 Reviewing {}
        3 Writing {}
        4 Done {}
        5 Failed {}
        6 Stopped {}
        7 Interrupted {}
    }
}

/// A research as a list shows it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSummary {
    /// The chat the lead talks to the user in.
    pub session_id: Uuid,
    pub topic: String,
    pub status: ResearchStatus,
    /// The newest round, from 1.
    pub round: u32,
    /// How many sources the scouts kept.
    pub sources: u32,
    /// In milliseconds since the Unix epoch.
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

/// One agent of a research.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchAgent {
    /// `lead`, `scout-1`, ..., `critic` or `writer`.
    pub id: String,
    /// Like "Scout 1".
    pub name: String,
    /// What it does, like the question a scout works on.
    pub role: String,
    /// The display name of the model it runs on.
    pub model: String,
    /// The round a scout researched in; 0 for agents that work in every
    /// round.
    pub round: u32,
}

/// One thing an agent did or said.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchEntry {
    /// Grows with each entry of the research.
    pub seq: i64,
    /// The agent's id.
    pub agent: String,
    pub body: ResearchEntryBody,
    /// In milliseconds since the Unix epoch.
    pub timestamp: i64,
}

wire_enum! {
    /// `Said` is a message to the room, to one agent or to all when `to` is
    /// `None`. `Step` is a tool the agent ran, in its own thread. `Note` is a
    /// finding for the notes, citing source numbers.
    #[derive(Debug, Clone, PartialEq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum ResearchEntryBody {
        0 Said { to: Option<String>, content: String }
        1 Step { label: String, success: bool }
        2 Note { content: String, sources: Vec<u32> }
    }
}

/// A source a scout kept.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSource {
    /// From 1, as cited.
    pub number: u32,
    pub url: String,
    pub title: String,
    /// The passage it was kept for.
    pub quote: String,
    /// The agent that found it.
    pub agent: String,
}

impl ResearchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planning {} => "planning",
            Self::Researching {} => "researching",
            Self::Reviewing {} => "reviewing",
            Self::Writing {} => "writing",
            Self::Done {} => "done",
            Self::Failed {} => "failed",
            Self::Stopped {} => "stopped",
            Self::Interrupted {} => "interrupted",
        }
    }
}
