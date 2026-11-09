//! A session's turns as the apps see them.

use crate::{
    protocol::{AutoPick, HistoryEntry, HistoryTool},
    routines::cron,
    routines::wake,
    store::{Step, StepBody, StoredBody, StoredMessage, TurnRecord},
    tools,
};
use chrono::{DateTime, Utc};
use flashmind::core::{ConversationEntry, EntryKind};
use std::{
    collections::{HashMap, HashSet},
    mem,
};

/// Tool outputs are cut to this many bytes.
pub const MAX_TOOL_OUTPUT: usize = 16 * 1024;
/// A turn keeps the last this many bytes of its reasoning.
pub const MAX_REASONING: usize = 16 * 1024;

/// What each turn ran with, by the millisecond time of its opening message.
pub type TurnsByTime = HashMap<i64, TurnRecord>;

/// What a run knew of its tool calls that the conversation does not keep, by call id.
#[derive(Debug, Default)]
pub struct CallNotes {
    pub failed: HashSet<String>,
    /// What each call showed while it ran, such as "Recalling memories".
    pub descriptions: HashMap<String, String>,
}

/// The steps of some entries.
#[derive(Debug, Default, PartialEq)]
pub struct Steps {
    pub steps: Vec<Step>,
    /// Reasoning before the first opener, carried on to a turn saved earlier.
    pub carried_reasoning: String,
}

/// The steps of `entries` from `since` on; earlier entries only name the tools their calls ran.
pub fn steps(
    entries: &[ConversationEntry],
    since: Option<DateTime<Utc>>,
    calls: &CallNotes,
    turns: &TurnsByTime,
) -> Steps {
    let mut out = Steps::default();
    let mut names: HashMap<&str, &str> = HashMap::new();
    // A routine's or an event's label, from the context saved just before
    // its message.
    let mut scheduled = None;
    // Where the open turn's record is in `out.steps`.
    let mut opener: Option<usize> = None;
    let mut reasoning = String::new();

    for entry in entries {
        if let EntryKind::Assistant {
            tool_calls: Some(calls),
            ..
        } = &entry.kind
        {
            for call in calls {
                names.insert(&call.id, &call.name);
            }
        }
        if since.is_some_and(|since| entry.timestamp < since) || entry.is_summary() {
            continue;
        }

        let sent_at = entry.timestamp;

        match &entry.kind {
            EntryKind::User { content, .. } => {
                finish(&mut out, opener, &mut reasoning);
                let turn = turns
                    .get(&sent_at.timestamp_millis())
                    .cloned()
                    .unwrap_or_default();
                let label = scheduled
                    .take()
                    .or_else(|| cron::legacy_scheduled_label(content));
                let body = match label {
                    Some(label) => StepBody::Scheduled {
                        label: label.to_owned(),
                        turn,
                    },
                    None => StepBody::User {
                        content: content.clone(),
                        turn,
                    },
                };
                opener = Some(out.steps.len());
                out.steps.push(Step { sent_at, body });
            }
            EntryKind::Assistant {
                content,
                reasoning: said,
                ..
            } => {
                if let Some(said) = said.as_deref().map(str::trim)
                    && !said.is_empty()
                {
                    if !reasoning.is_empty() {
                        reasoning.push_str("\n\n");
                    }

                    reasoning.push_str(said);
                }

                if !content.is_empty() {
                    out.steps.push(Step {
                        sent_at,
                        body: StepBody::Assistant {
                            content: content.clone(),
                        },
                    });
                }
            }
            EntryKind::Tool {
                call_id, output, ..
            } => out.steps.push(Step {
                sent_at,
                body: StepBody::Tool {
                    name: names
                        .get(call_id.as_str())
                        .copied()
                        .unwrap_or_default()
                        .to_owned(),
                    call_id: call_id.clone(),
                    output: cap(output, MAX_TOOL_OUTPUT),
                    success: !calls.failed.contains(call_id),
                    description: calls.descriptions.get(call_id).cloned().unwrap_or_default(),
                },
            }),
            EntryKind::Developer { content, .. } => {
                scheduled = cron::scheduled_label(content).or_else(|| wake::woken_label(content));
            }
            _ => {}
        }
    }

    finish(&mut out, opener, &mut reasoning);
    out
}

/// Tools whose results the apps show as what Beep sent.
const SENT_TOOLS: &[&str] = &[
    tools::SEND_IMAGE,
    tools::file::NAME,
    "image_gen",
    "video_gen",
    "3d_gen",
];

/// What a routine told the user: its `notify_user` text, else its last reply.
pub fn routine_reply(entries: &[ConversationEntry], since: Option<DateTime<Utc>>) -> Option<Step> {
    let turn: Vec<&ConversationEntry> = entries
        .iter()
        .filter(|entry| !since.is_some_and(|since| entry.timestamp < since) && !entry.is_summary())
        .collect();
    let said = |entry: &&ConversationEntry| match &entry.kind {
        EntryKind::Assistant { content, .. } if !content.trim().is_empty() => Some(Step {
            sent_at: entry.timestamp,
            body: StepBody::Assistant {
                content: content.clone(),
            },
        }),
        _ => None,
    };
    let notified = turn.iter().rposition(|entry| {
        matches!(&entry.kind, EntryKind::Assistant { tool_calls: Some(calls), .. }
            if calls.iter().any(|call| call.name == tools::NOTIFY_USER))
    });
    notified
        .and_then(|at| turn[at..].iter().find_map(said))
        .or_else(|| turn.iter().rev().find_map(said))
}

/// A routine's turn as the apps show it: [`routine_reply`] and the files it sent.
pub fn routine_notice(
    entries: &[ConversationEntry],
    since: Option<DateTime<Utc>>,
    steps: Steps,
) -> Steps {
    let sent = steps.steps.into_iter().filter(|step| {
        matches!(&step.body, StepBody::Tool { name, success: true, .. }
            if SENT_TOOLS.contains(&name.as_str()))
    });
    Steps {
        steps: routine_reply(entries, since)
            .into_iter()
            .chain(sent)
            .collect(),
        carried_reasoning: String::new(),
    }
}

/// Gives the gathered reasoning to the turn at `opener`, or to the carried turn.
fn finish(out: &mut Steps, opener: Option<usize>, reasoning: &mut String) {
    let reasoning = cap_tail(&mem::take(reasoning), MAX_REASONING);

    match opener.and_then(|at| out.steps.get_mut(at)) {
        Some(Step {
            body: StepBody::User { turn, .. } | StepBody::Scheduled { turn, .. },
            ..
        }) => turn.reasoning = reasoning,
        _ => out.carried_reasoning = reasoning,
    }
}

/// The history old requests answer with; a turn with tools but no reply gets an empty one.
pub fn history(messages: &[StoredMessage]) -> Vec<HistoryEntry> {
    let mut history = Vec::new();
    let mut turn = Turn::default();

    for message in messages {
        let timestamp = message.timestamp();

        match &message.body {
            StoredBody::User { content, turn: run } => {
                turn.finish(&mut history);
                turn.start(run);
                history.push(HistoryEntry::User {
                    content: content.clone(),
                    timestamp,
                    auto: run.auto_thinking.map(|thinking| AutoPick {
                        model: run.model.clone(),
                        thinking,
                    }),
                });
            }
            StoredBody::Scheduled { label, turn: run } => {
                turn.finish(&mut history);
                turn.start(run);
                history.push(HistoryEntry::Scheduled {
                    label: label.clone(),
                    timestamp,
                });
            }
            StoredBody::Assistant { content } => {
                // A routine's notice stands outside any turn, so the turn
                // before keeps its own reply and tools.
                if message.turn_id.is_none() {
                    turn.finish(&mut history);
                }

                turn.last_reply = Some(history.len());
                history.push(HistoryEntry::Assistant {
                    content: content.clone(),
                    timestamp,
                    tools: Vec::new(),
                    memory_score: None,
                    reasoning: String::new(),
                    model: String::new(),
                });
            }
            StoredBody::Tool {
                call_id,
                name,
                output,
                success,
                ..
            } => {
                turn.last_tool = timestamp;
                turn.tools.push(HistoryTool {
                    name: name.clone(),
                    call_id: call_id.clone(),
                    output: output.clone(),
                    success: *success,
                });
            }
            StoredBody::Approval { query_id, approval } => {
                turn.approvals.push(HistoryEntry::Approval {
                    query_id: query_id.clone(),
                    approval: approval.clone(),
                });
            }
            StoredBody::Member { .. } | StoredBody::Beep { .. } => {}
        }
    }

    turn.finish(&mut history);
    history
}

#[derive(Default)]
struct Turn {
    tools: Vec<HistoryTool>,
    /// Index in the history of this turn's latest reply.
    last_reply: Option<usize>,
    last_tool: i64,
    record: TurnRecord,
    approvals: Vec<HistoryEntry>,
}

impl Turn {
    fn start(&mut self, record: &TurnRecord) {
        self.record = record.clone();
    }

    fn finish(&mut self, history: &mut Vec<HistoryEntry>) {
        let turn = mem::take(self);

        match turn.last_reply.and_then(|index| history.get_mut(index)) {
            Some(HistoryEntry::Assistant {
                tools,
                memory_score,
                reasoning,
                model,
                ..
            }) => {
                *tools = turn.tools;
                *model = turn.record.model;
                *memory_score = turn.record.memory_score;
                *reasoning = turn.record.reasoning;
            }
            _ if turn.tools.is_empty() => {}
            _ => history.push(HistoryEntry::Assistant {
                content: String::new(),
                timestamp: turn.last_tool,
                tools: turn.tools,
                memory_score: turn.record.memory_score,
                reasoning: turn.record.reasoning,
                model: turn.record.model,
            }),
        }

        history.extend(turn.approvals);
    }
}

/// The end of `text`, at most `max` bytes on a character boundary.
pub fn cap_tail(text: &str, max: usize) -> String {
    text[text.ceil_char_boundary(text.len().saturating_sub(max))..].to_owned()
}

/// `text` cut to at most `max` bytes on a character boundary.
pub fn cap(text: &str, max: usize) -> String {
    text[..text.floor_char_boundary(max)].to_owned()
}

#[cfg(test)]
#[path = "../tests/agent_steps.rs"]
mod tests;
