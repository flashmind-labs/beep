//! A session's turns as the apps see them: the steps the model's entries
//! become when saved, and the history the old requests build from them.

use crate::{
    cron,
    protocol::{AutoPick, HistoryEntry, HistoryTool},
    store::{Step, StepBody, StoredBody, StoredMessage, TurnRecord},
    wake,
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

/// What a turn ran with besides its reasoning, by the millisecond time of
/// the message that opened it.
pub type TurnsByTime = HashMap<i64, TurnRecord>;

/// The steps of some entries.
#[derive(Debug, Default, PartialEq)]
pub struct Steps {
    pub steps: Vec<Step>,
    /// The reasoning of the steps before the first that opens a turn, which
    /// carry on a turn saved earlier.
    pub carried_reasoning: String,
}

/// The steps of the entries of `entries` from `since` on: each user
/// message or scheduled prompt, reply with text, and tool result. Earlier
/// entries only name the tools their calls ran. Tool calls in `failures`
/// failed, and `turns` holds what each turn ran with.
pub fn steps(
    entries: &[ConversationEntry],
    since: Option<DateTime<Utc>>,
    failures: &HashSet<String>,
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
            EntryKind::Tool { call_id, output } => out.steps.push(Step {
                sent_at,
                body: StepBody::Tool {
                    name: names
                        .get(call_id.as_str())
                        .copied()
                        .unwrap_or_default()
                        .to_owned(),
                    call_id: call_id.clone(),
                    output: cap(output, MAX_TOOL_OUTPUT),
                    success: !failures.contains(call_id),
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

/// Gives the reasoning gathered so far to the turn at `opener`, or to the
/// turn carried on when there is none yet.
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

/// The history the old requests answer with: user messages, routine
/// prompts, and replies, with each turn's tool results, reasoning, model and
/// memory score on the reply that ends it. A turn with tools but no reply
/// gets an empty one to carry them. A waiting approval comes after its turn.
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
mod tests {
    use super::*;
    use chrono::Duration;
    use flashmind::types::{ReasoningLevel, ToolCall};
    use serde_json::json;
    use uuid::Uuid;

    /// The history of `entries` once saved as steps.
    fn saved_history(
        entries: &[ConversationEntry],
        failures: &HashSet<String>,
        turns: &TurnsByTime,
    ) -> Vec<HistoryEntry> {
        let messages: Vec<StoredMessage> = steps(entries, None, failures, turns)
            .steps
            .into_iter()
            .enumerate()
            .map(|(id, step)| StoredMessage {
                id: id as i64 + 1,
                chat_id: Uuid::nil(),
                turn_id: None,
                sent_at: step.sent_at,
                author_id: None,
                body: step.body.into(),
                images: Vec::new(),
            })
            .collect();
        history(&messages)
    }

    #[test]
    fn history_attaches_tools_to_the_turn() {
        let call = |id: &str, name: &str| ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: json!({}),
        };
        let mut entries = [
            ConversationEntry::user("Where is lunch?"),
            ConversationEntry::assistant_with_tool_calls(
                "",
                vec![call("a", "open_maps")],
                Some("Look it up.".into()),
            ),
            ConversationEntry::tool("a", "é".repeat(MAX_TOOL_OUTPUT)),
            ConversationEntry::assistant_with_reasoning("Here it is.", Some("Found one.".into())),
            ConversationEntry::user("Remind me"),
            ConversationEntry::assistant_with_tool_calls("", vec![call("b", "cron_create")], None),
            ConversationEntry::tool("b", "failed"),
        ];
        // Each turn is found by its user message's time, so they must differ.
        entries[4].timestamp = entries[0].timestamp + Duration::seconds(1);
        let asked = entries[0].timestamp.timestamp_millis();
        let history = saved_history(
            &entries,
            &HashSet::from(["b".to_owned()]),
            &HashMap::from([(
                asked,
                TurnRecord {
                    model: "z-ai/glm-5.3-flash".to_owned(),
                    auto_thinking: Some(ReasoningLevel::High),
                    reasoning: String::new(),
                    memory_score: Some(0.8),
                },
            )]),
        );
        let [
            HistoryEntry::User {
                auto: first_auto, ..
            },
            HistoryEntry::Assistant {
                content: first,
                tools: first_tools,
                memory_score: first_score,
                reasoning: first_reasoning,
                model: first_model,
                ..
            },
            HistoryEntry::User {
                auto: second_auto, ..
            },
            HistoryEntry::Assistant {
                content: second,
                tools: second_tools,
                memory_score: second_score,
                reasoning: second_reasoning,
                model: second_model,
                ..
            },
        ] = history.as_slice()
        else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(first, "Here it is.");
        assert_eq!(*first_score, Some(0.8));
        assert_eq!(*second_score, None);
        assert_eq!(first_model, "z-ai/glm-5.3-flash");
        assert!(second_model.is_empty());
        assert_eq!(
            first_auto.as_ref().map(|pick| pick.thinking),
            Some(ReasoningLevel::High)
        );
        assert!(second_auto.is_none());
        assert_eq!(first_reasoning, "Look it up.\n\nFound one.");
        assert!(second_reasoning.is_empty());
        assert_eq!(first_tools[0].name, "open_maps");
        assert!(first_tools[0].success);
        assert_eq!(first_tools[0].output.len(), MAX_TOOL_OUTPUT);
        assert!(second.is_empty());
        assert_eq!(second_tools[0].call_id, "b");
        assert!(!second_tools[0].success);
    }

    #[test]
    fn history_marks_a_routine_turn_as_scheduled() {
        let entries = [
            ConversationEntry::system_message(cron::scheduled_context("Plants")),
            ConversationEntry::user("Remind the user to water the plants"),
            ConversationEntry::assistant("Time to water the plants."),
            ConversationEntry::system_message("Time: Monday 2026-10-05 09:00 +04"),
            ConversationEntry::user("Thanks"),
            ConversationEntry::user("[Scheduled: Old] Remind me to stretch"),
        ];
        let history = saved_history(&entries, &HashSet::new(), &HashMap::new());
        let [
            HistoryEntry::Scheduled { label, .. },
            HistoryEntry::Assistant { .. },
            HistoryEntry::User { content, .. },
            HistoryEntry::Scheduled { label: old, .. },
        ] = history.as_slice()
        else {
            panic!("unexpected history: {history:?}");
        };
        assert_eq!(label, "Plants");
        assert_eq!(content, "Thanks");
        assert_eq!(old, "Old");
    }

    #[test]
    fn steps_before_the_first_opener_carry_their_reasoning() {
        let mut entries = [
            ConversationEntry::user("Earlier"),
            ConversationEntry::assistant_with_reasoning("Waiting on you.", None),
            ConversationEntry::assistant_with_reasoning("Done.", Some("Approved.".into())),
        ];
        entries[2].timestamp = entries[0].timestamp + Duration::seconds(1);
        let since = entries[2].timestamp;
        let out = steps(&entries, Some(since), &HashSet::new(), &HashMap::new());
        assert_eq!(out.carried_reasoning, "Approved.");
        assert!(out.steps.iter().all(|step| !step.body.opens_turn()));
    }
}
