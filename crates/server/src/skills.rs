//! Skills: how a user or group wants a recurring task done, learned from their chats.

use crate::{
    memories::clip,
    starters::Generator,
    store::{NewSkill, SkillOwner, SkillPatch, SkillRow, Store},
};
use anyhow::{Context, Result};
use flashmind::core::{ConversationEntry, EntryKind};
use serde::Deserialize;
use std::fmt::Write;

/// A chat is reviewed after this many turns since its last review.
pub const REVIEW_TURNS: i32 = 8;

/// A turn that made this many tool calls is reviewed straight away.
pub const REVIEW_TOOL_CALLS: usize = 15;

pub const NAME_MAX: usize = 60;
pub const DESCRIPTION_MAX: usize = 200;
pub const BODY_MAX: usize = 6000;

/// The most skills the context lists.
const INDEX_MAX: usize = 30;

/// The most of the transcript a review reads, from its end.
const TRANSCRIPT_MAX: usize = 40_000;

/// How much of each tool call's arguments and result a review reads.
const TOOL_CLIP: usize = 400;

/// Starts the context's skill list, so a chat can tell whether it carries the current one.
pub const INDEX_HEADER: &str = "Skills, how this user likes recurring tasks done. Before doing a task one covers, load it with `skill` view:";

/// The same for a group.
pub const GROUP_INDEX_HEADER: &str = "Skills, how this group likes recurring tasks done. Before doing a task one covers, load it with `skill` view:";

const REVIEW_SYSTEM: &str = include_str!("../prompts/skill_review.md");

/// The skill list for the context, most recently used first; `None` when empty.
pub fn index(header: &str, skills: &[SkillRow]) -> Option<String> {
    let mut text = String::from(header);
    let mut any = false;
    for skill in skills.iter().filter(|skill| skill.enabled).take(INDEX_MAX) {
        let _ = write!(text, "\n- {}: {}", skill.name, skill.description);
        any = true;
    }
    any.then_some(text)
}

/// The turns a review reads, written out; keeps the end when long.
pub fn transcript<'a>(entries: impl IntoIterator<Item = &'a ConversationEntry>) -> String {
    let mut text = String::new();
    for entry in entries {
        match &entry.kind {
            EntryKind::User { content, .. } => {
                let _ = write!(text, "\n\nUser: {content}");
            }
            EntryKind::Assistant {
                content,
                tool_calls,
                ..
            } => {
                if !content.is_empty() {
                    let _ = write!(text, "\n\nBeep: {content}");
                }
                for call in tool_calls.iter().flatten() {
                    let arguments = clip(&call.arguments.to_string(), TOOL_CLIP);
                    let _ = write!(text, "\n[call {} {arguments}]", call.name);
                }
            }
            EntryKind::Tool { output, .. } => {
                let _ = write!(text, "\n[result {}]", clip(output, TOOL_CLIP));
            }
            EntryKind::SystemPrompt(_) | EntryKind::Developer { .. } => {}
        }
    }
    tail(text.trim(), TRANSCRIPT_MAX)
}

/// The last `max` bytes of `text`, at a character boundary.
fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("[earlier turns left out]\n{}", &text[start..])
}

/// Tool calls in `entries`.
pub fn tool_calls<'a>(entries: impl IntoIterator<Item = &'a ConversationEntry>) -> usize {
    entries
        .into_iter()
        .map(|entry| match &entry.kind {
            EntryKind::Assistant {
                tool_calls: Some(calls),
                ..
            } => calls.len(),
            _ => 0,
        })
        .sum()
}

/// Who the reviewed chat is with, which changes what may go in a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Personal,
    Group,
}

fn review_prompt(audience: Audience, skills: &[SkillRow], transcript: &str) -> String {
    let mut text = String::new();
    if audience == Audience::Group {
        text.push_str(
            "This is a group chat. Skills belong to the whole group: never put one member's \
             private details in one.\n\n",
        );
    }
    if skills.is_empty() {
        text.push_str("Existing skills: none.\n");
    } else {
        text.push_str("Existing skills:\n");
        for skill in skills {
            let paused = if skill.enabled { "" } else { " (paused)" };
            let _ = write!(
                text,
                "\n## {}{paused}\n{}\n\n{}\n",
                skill.name, skill.description, skill.body
            );
        }
    }
    let _ = write!(text, "\nConversation:\n{transcript}\n");
    text
}

/// A change the review asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum SkillOp {
    Create {
        name: String,
        description: String,
        body: String,
    },
    /// Rewrites the existing skill `name`.
    Update {
        name: String,
        #[serde(default)]
        description: Option<String>,
        body: String,
    },
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    changes: Vec<SkillOp>,
}

/// Reads the review's JSON answer, which may sit in a code fence.
pub fn parse_ops(answer: &str) -> Result<Vec<SkillOp>> {
    let start = answer.find('{').context("no JSON in the skill review")?;
    let end = answer.rfind('}').context("no JSON in the skill review")?;
    let json = answer
        .get(start..=end)
        .context("no JSON in the skill review")?;
    let parsed: Answer = serde_json::from_str(json).context("unreadable skill review")?;
    Ok(parsed.changes)
}

/// Whether the text fits a skill's limits.
pub fn valid(name: &str, description: &str, body: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= NAME_MAX
        && !description.trim().is_empty()
        && description.chars().count() <= DESCRIPTION_MAX
        && !body.trim().is_empty()
        && body.len() <= BODY_MAX
}

/// What a review changed, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reviewed {
    pub created: Vec<String>,
    pub updated: Vec<String>,
}

/// Reviews `transcript` for `owner` and applies what it asks for.
pub async fn review(
    store: &Store,
    generator: Generator<'_>,
    owner: &SkillOwner,
    audience: Audience,
    transcript: &str,
) -> Result<Reviewed> {
    if transcript.is_empty() {
        return Ok(Reviewed::default());
    }
    let skills = store.owned_skills(owner).await?;
    let answer = generator
        .text_up_to(
            REVIEW_SYSTEM,
            review_prompt(audience, &skills, transcript),
            6000,
        )
        .await?;
    let mut reviewed = Reviewed::default();
    for op in parse_ops(&answer)?.into_iter().take(2) {
        match op {
            SkillOp::Create {
                name,
                description,
                body,
            } => {
                if !valid(&name, &description, &body) {
                    continue;
                }
                let skill = NewSkill {
                    name: name.trim(),
                    description: description.trim(),
                    body: body.trim(),
                };
                if store.create_skill(owner, skill).await?.is_some() {
                    reviewed.created.push(name);
                }
            }
            SkillOp::Update {
                name,
                description,
                body,
            } => {
                let Some(skill) = skills
                    .iter()
                    .find(|skill| skill.name.eq_ignore_ascii_case(name.trim()))
                else {
                    continue;
                };
                let description = description.as_deref().map(str::trim);
                if !valid(
                    &skill.name,
                    description.unwrap_or(&skill.description),
                    &body,
                ) {
                    continue;
                }
                let patch = SkillPatch {
                    description,
                    body: Some(body.trim()),
                    ..SkillPatch::default()
                };
                if store
                    .learn_skill_change(owner, skill.id, patch)
                    .await?
                    .is_some()
                {
                    reviewed.updated.push(skill.name.clone());
                }
            }
        }
    }
    Ok(reviewed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fenced_answers() {
        let answer = "```json\n{\"changes\":[{\"op\":\"create\",\"name\":\"Book padel\",\"description\":\"Booking a court\",\"body\":\"1. Ask\"},{\"op\":\"update\",\"name\":\"Plan trips\",\"body\":\"2. Go\"}]}\n```";
        assert_eq!(
            parse_ops(answer).unwrap(),
            [
                SkillOp::Create {
                    name: "Book padel".into(),
                    description: "Booking a court".into(),
                    body: "1. Ask".into(),
                },
                SkillOp::Update {
                    name: "Plan trips".into(),
                    description: None,
                    body: "2. Go".into(),
                },
            ]
        );
        assert!(parse_ops("{\"changes\":[]}").unwrap().is_empty());
        assert!(parse_ops("nothing to learn").is_err());
        assert!(parse_ops("} nothing to learn {").is_err());
    }
}
