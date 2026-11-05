//! A saved chat laid out as turns, the way a window draws it: a prompt, what the model did, and
//! its answer.

use crate::{Chat, Message, tools};
use serde::Serialize;

/// A tool call of a turn and how it ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub call_id: String,
    pub name: String,
    pub label: String,
    pub ok: bool,
}

/// A prompt and the reply to it. The answer is markdown, its rounds apart by a blank line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnView {
    pub question: String,
    pub reasoning: String,
    pub steps: Vec<Step>,
    pub answer: String,
}

/// The turns of `chat`, oldest first.
pub fn turns(chat: &Chat) -> Vec<TurnView> {
    let mut out: Vec<TurnView> = Vec::new();
    for message in &chat.messages {
        match message {
            Message::User { text } => out.push(TurnView {
                question: text.clone(),
                reasoning: String::new(),
                steps: Vec::new(),
                answer: String::new(),
            }),
            Message::Assistant {
                text,
                reasoning,
                calls,
            } => {
                let Some(turn) = out.last_mut() else { continue };
                if !text.is_empty() {
                    if !turn.answer.is_empty() {
                        turn.answer.push_str("\n\n");
                    }
                    turn.answer.push_str(text);
                }
                turn.reasoning.push_str(reasoning);
                turn.steps.extend(calls.iter().map(|call| Step {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    label: tools::label(call),
                    ok: false,
                }));
            }
            Message::Tool { call_id, ok, .. } => {
                let step = out
                    .last_mut()
                    .and_then(|turn| turn.steps.iter_mut().find(|step| step.call_id == *call_id));
                if let Some(step) = step {
                    step.ok = *ok;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Call;
    use uuid::Uuid;

    #[test]
    fn a_saved_chat_becomes_turns_with_their_steps() {
        let call = Call {
            id: "c1".into(),
            name: "web_search".into(),
            arguments: r#"{"query":"rust"}"#.into(),
        };
        let chat = Chat {
            id: Uuid::new_v4(),
            title: "t".into(),
            model: "m".into(),
            created_ms: 0,
            updated_ms: 0,
            messages: vec![
                Message::User { text: "q1".into() },
                Message::Assistant {
                    text: "Looking.".into(),
                    reasoning: "think".into(),
                    calls: vec![call],
                },
                Message::Tool {
                    call_id: "c1".into(),
                    name: "web_search".into(),
                    text: "r".into(),
                    ok: true,
                },
                Message::Assistant {
                    text: "Done.".into(),
                    reasoning: String::new(),
                    calls: vec![],
                },
                Message::User { text: "q2".into() },
            ],
        };
        let turns = turns(&chat);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].answer, "Looking.\n\nDone.");
        assert_eq!(turns[0].reasoning, "think");
        assert_eq!(turns[0].steps[0].label, "Searching the web for rust");
        assert!(turns[0].steps[0].ok);
        assert_eq!(turns[1].answer, "");
    }
}
