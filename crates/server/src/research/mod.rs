//! Research: a lead agent plans a topic with the user in an ordinary chat,
//! then a team researches it in the background. Scouts take one question
//! each, a critic challenges what they found, the lead fills the gaps and
//! briefs the writer, and the writer drafts a paper. Follow-up questions in
//! the chat start more rounds, which revise the same paper. The chat only
//! gets the plan and the results; everything the team does is kept as
//! entries that the Mac's research window reads (`GetResearch`). Each role
//! runs on its own model, set in the config's `[research]`.

mod lead;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
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
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// The team is at work.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Researching | Self::Reviewing | Self::Writing)
    }

    /// The team can be started from here, on a new round or the last one
    /// again.
    pub fn can_start(self) -> bool {
        !self.is_running()
    }

    pub fn wire(self) -> ResearchStatus {
        match self {
            Self::Planning => ResearchStatus::Planning {},
            Self::Researching => ResearchStatus::Researching {},
            Self::Reviewing => ResearchStatus::Reviewing {},
            Self::Writing => ResearchStatus::Writing {},
            Self::Done => ResearchStatus::Done {},
            Self::Failed => ResearchStatus::Failed {},
            Self::Stopped => ResearchStatus::Stopped {},
            Self::Interrupted => ResearchStatus::Interrupted {},
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

/// The config's `[research]`: the model each role runs on. Each must be one
/// of the server's model choices.
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

/// What the user approved: one scout per question, and the models. Each
/// round adds its scouts.
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

impl Entry {
    pub fn wire(&self) -> ResearchEntryBody {
        match self {
            Self::Said { to, content } => ResearchEntryBody::Said {
                to: to.map(|to| to.to_string()),
                content: content.clone(),
            },
            Self::Step { label, success } => ResearchEntryBody::Step {
                label: label.clone(),
                success: *success,
            },
            Self::Note { content, sources } => ResearchEntryBody::Note {
                content: content.clone(),
                sources: sources.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_read_back_from_their_ids() {
        for member in [
            Member::Lead,
            Member::Scout(1),
            Member::Scout(5),
            Member::Critic,
            Member::Writer,
        ] {
            assert_eq!(member.to_string().parse::<Member>().ok(), Some(member));
        }
        assert!("scout-0".parse::<Member>().is_err());
        assert!(
            format!("scout-{}", MAX_TEAM_SCOUTS + 1)
                .parse::<Member>()
                .is_err()
        );
        assert!("boss".parse::<Member>().is_err());
    }

    #[test]
    fn statuses_read_back_from_the_store() {
        assert_eq!("researching".parse(), Ok(Status::Researching));
        assert_eq!(Status::Interrupted.as_str(), "interrupted");
        assert!(Status::Stopped.can_start());
        assert!(Status::Done.can_start(), "a follow-up starts a round");
        assert!(!Status::Writing.can_start());
    }

    #[test]
    fn a_plan_reads_its_rounds_scouts() {
        let model = RoleModel {
            model: "openai:test".parse().expect("a model id"),
            thinking: ReasoningLevel::Medium,
        };
        let scout = |question: &str, round| Scout {
            question: question.to_owned(),
            model: model.clone(),
            round,
        };
        let plan = Plan {
            scouts: vec![scout("a", 1), scout("b", 1), scout("c", 2)],
            critic: model.clone(),
            writer: model.clone(),
        };
        assert_eq!(plan.round(), 2);
        let second: Vec<Member> = plan.scouts_of(2).map(|(member, _)| member).collect();
        assert_eq!(second, [Member::Scout(3)]);
        let old: Plan = serde_json::from_value(serde_json::json!({
            "scouts": [{ "question": "a", "model": model }],
            "critic": model,
            "writer": model,
        }))
        .expect("a plan from before rounds");
        assert_eq!(old.round(), 1);
    }

    #[test]
    fn the_config_reads_a_model_and_thinking_per_role() {
        let config: ResearchConfig = toml::from_str(
            r#"
            lead = { model = "openrouter:anthropic/claude-sonnet-5.5", thinking = "high" }
            scout = { model = "openrouter:deepseek/deepseek-v4.1-flash-1017" }
            critic = { model = "openrouter:openai/gpt-6-luna" }
            writer = { model = "openrouter:anthropic/claude-haiku-5.5" }
            scout_choices = ["openrouter:z-ai/glm-5.3-flash"]
            "#,
        )
        .expect("the example parses");
        assert_eq!(config.lead.thinking, ReasoningLevel::High);
        assert_eq!(config.scout.thinking, ReasoningLevel::Medium);
        let glm = "openrouter:z-ai/glm-5.3-flash".parse().expect("a model id");
        assert!(config.scout_may_use(&glm));
        assert!(!config.scout_may_use(&config.lead.model));
    }
}
