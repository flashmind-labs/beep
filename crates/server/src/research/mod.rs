//! Research: the lead plans a topic with the user, then a team of scouts, a critic and a writer drafts a paper in the background.

mod lead;
#[cfg(feature = "pdf")]
pub(crate) mod pdf;
#[cfg(not(feature = "pdf"))]
#[path = "pdf_off.rs"]
pub(crate) mod pdf;
mod tools;

pub use lead::{AddScoutsTool, ResearchMoreTool, StartResearchTool, StopResearchTool};
pub(crate) use tools::{Seat, TEAM_TOOLS, TeamTools, team_tools};

use crate::protocol::{ResearchEntryBody, ResearchStatus};
use flashmind::types::{Model, ReasoningLevel};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use strum::{EnumString, IntoStaticStr};

/// The fewest and most questions a plan may have, one scout each.
pub const MIN_SCOUTS: usize = 2;
pub const MAX_SCOUTS: usize = 5;
/// The most questions a follow-up round may have.
pub const MAX_FOLLOW_UPS: usize = 3;
/// The most scouts the lead adds to a round to fill its gaps.
pub const MAX_GAP_SCOUTS: usize = 2;
/// The most rounds a research may have.
pub const MAX_ROUNDS: u32 = 6;
/// The most scouts a research may have over all its rounds.
pub const MAX_TEAM_SCOUTS: usize =
    MAX_SCOUTS + MAX_GAP_SCOUTS + (MAX_ROUNDS as usize - 1) * (MAX_FOLLOW_UPS + MAX_GAP_SCOUTS);

/// Where a research stands, as the store keeps it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr, strum::AsRefStr, strum::Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum Status {
    Planning,
    Researching,
    Reviewing,
    Writing,
    Done,
    Failed,
    Stopped,
    Interrupted,
}

impl Status {
    /// The team is at work.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Researching | Self::Reviewing | Self::Writing)
    }
}

impl From<Status> for ResearchStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::Planning => ResearchStatus::Planning {},
            Status::Researching => ResearchStatus::Researching {},
            Status::Reviewing => ResearchStatus::Reviewing {},
            Status::Writing => ResearchStatus::Writing {},
            Status::Done => ResearchStatus::Done {},
            Status::Failed => ResearchStatus::Failed {},
            Status::Stopped => ResearchStatus::Stopped {},
            Status::Interrupted => ResearchStatus::Interrupted {},
        }
    }
}

/// One agent of a research's team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Member {
    Lead,
    /// Numbered from 1, in plan order.
    Scout(usize),
    Critic,
    Writer,
}

impl Member {
    pub fn name(self) -> String {
        match self {
            Self::Lead => "Lead".to_owned(),
            Self::Scout(n) => format!("Scout {n}"),
            Self::Critic => "Critic".to_owned(),
            Self::Writer => "Writer".to_owned(),
        }
    }
}

impl fmt::Display for Member {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Lead => f.write_str("lead"),
            Self::Scout(n) => write!(f, "scout-{n}"),
            Self::Critic => f.write_str("critic"),
            Self::Writer => f.write_str("writer"),
        }
    }
}

impl FromStr for Member {
    type Err = UnknownMember;

    fn from_str(text: &str) -> Result<Self, UnknownMember> {
        Ok(match text {
            "lead" => Self::Lead,
            "critic" => Self::Critic,
            "writer" => Self::Writer,
            _ => {
                let n = text
                    .strip_prefix("scout-")
                    .and_then(|n| n.parse().ok())
                    .filter(|n| (1..=MAX_TEAM_SCOUTS).contains(n))
                    .ok_or(UnknownMember)?;
                Self::Scout(n)
            }
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("no such agent in this research")]
pub struct UnknownMember;

/// A model and how long it thinks, for one role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleModel {
    pub model: Model,
    #[serde(default = "default_thinking")]
    pub thinking: ReasoningLevel,
}

fn default_thinking() -> ReasoningLevel {
    ReasoningLevel::Medium
}

/// The config's `[research]`: each role's model, one of the server's model choices.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchConfig {
    pub lead: RoleModel,
    pub scout: RoleModel,
    pub critic: RoleModel,
    pub writer: RoleModel,
    /// Other models the lead may put a scout on, besides `scout`.
    #[serde(default)]
    pub scout_choices: Vec<Model>,
}

impl ResearchConfig {
    /// Whether a scout may run on `model`.
    pub fn scout_may_use(&self, model: &Model) -> bool {
        &self.scout.model == model || self.scout_choices.contains(model)
    }
}

/// What the user approved: one scout per question, and the models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub scouts: Vec<Scout>,
    pub critic: RoleModel,
    pub writer: RoleModel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scout {
    pub question: String,
    pub model: RoleModel,
    /// The round it researched in, from 1.
    #[serde(default = "first_round")]
    pub round: u32,
}

fn first_round() -> u32 {
    1
}

/// A member as the team lists it.
pub struct TeamMember<'a> {
    pub member: Member,
    /// What it does: a scout's question.
    pub role: &'a str,
    pub model: &'a RoleModel,
    /// A scout's round; 0 for the members that work in every round.
    pub round: u32,
}

impl Plan {
    /// The newest round, from 1.
    pub fn round(&self) -> u32 {
        self.scouts.iter().map(|s| s.round).max().unwrap_or(1)
    }

    /// The scouts of `round`, by their member.
    pub fn scouts_of(&self, round: u32) -> impl Iterator<Item = (Member, &Scout)> {
        self.scouts
            .iter()
            .enumerate()
            .filter(move |(_, scout)| scout.round == round)
            .map(|(i, scout)| (Member::Scout(i + 1), scout))
    }

    /// Every member with what it does and its model, lead first.
    pub fn team<'a>(&'a self, lead: &'a RoleModel) -> Vec<TeamMember<'a>> {
        let member = |member, role, model| TeamMember {
            member,
            role,
            model,
            round: 0,
        };
        let mut team = vec![member(Member::Lead, "Plans and coordinates", lead)];

        for (i, scout) in self.scouts.iter().enumerate() {
            team.push(TeamMember {
                round: scout.round,
                ..member(Member::Scout(i + 1), scout.question.as_str(), &scout.model)
            });
        }

        team.push(member(
            Member::Critic,
            "Challenges weak claims",
            &self.critic,
        ));
        team.push(member(Member::Writer, "Drafts the paper", &self.writer));
        team
    }
}

/// Something a member did or said.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Said { to: Option<Member>, content: String },
    Step { label: String, success: bool },
    Note { content: String, sources: Vec<u32> },
}

impl From<&Entry> for ResearchEntryBody {
    fn from(entry: &Entry) -> Self {
        match entry {
            Entry::Said { to, content } => ResearchEntryBody::Said {
                to: to.map(|to| to.to_string()),
                content: content.clone(),
            },
            Entry::Step { label, success } => ResearchEntryBody::Step {
                label: label.clone(),
                success: *success,
            },
            Entry::Note { content, sources } => ResearchEntryBody::Note {
                content: content.clone(),
                sources: sources.clone(),
            },
        }
    }
}

#[cfg(test)]
#[path = "../tests/research.rs"]
mod tests;
