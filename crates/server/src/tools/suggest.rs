use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_REPLY_CHARS: usize = 40;
const MIN_REPLIES: usize = 2;
const MAX_REPLIES: usize = 4;
const MAX_QUESTION_CHARS: usize = 120;

/// Offers tappable replies under the assistant's message.
pub(crate) struct SuggestRepliesTool;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SuggestionKind {
    /// Likely next messages from the user.
    #[default]
    Replies,
    /// The assistant needs the user to pick one option.
    Choice,
}

#[derive(Deserialize)]
struct SuggestArgs {
    replies: Vec<String>,
    #[serde(default)]
    kind: SuggestionKind,
    #[serde(default)]
    question: Option<String>,
}

/// What the phone receives in the tool result.
#[derive(Debug, PartialEq, Serialize)]
struct Suggestions {
    #[serde(rename = "type")]
    card: &'static str,
    kind: SuggestionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<String>,
    replies: Vec<String>,
}

/// Cuts `text` to `max` characters, at a word boundary when one is close.
fn shorten(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    match cut.rfind(' ') {
        Some(space) if space >= max / 2 => cut[..space].to_owned(),
        _ => cut,
    }
}

fn build(args: &Value) -> Result<Suggestions, String> {
    let args = SuggestArgs::deserialize(args).map_err(|e| format!("Invalid suggestions: {e}"))?;
    let mut replies: Vec<String> = Vec::new();
    for reply in &args.replies {
        let reply = shorten(reply, MAX_REPLY_CHARS);
        if !reply.is_empty() && !replies.iter().any(|r| r.eq_ignore_ascii_case(&reply)) {
            replies.push(reply);
        }
    }
    replies.truncate(MAX_REPLIES);
    if replies.len() < MIN_REPLIES {
        return Err(format!(
            "Give {MIN_REPLIES}-{MAX_REPLIES} distinct, non-empty replies."
        ));
    }
    let question = args
        .question
        .map(|q| shorten(&q, MAX_QUESTION_CHARS))
        .filter(|q| !q.is_empty());
    Ok(Suggestions {
        card: "suggestions",
        kind: args.kind,
        question,
        replies,
    })
}

#[async_trait]
impl Tool for SuggestRepliesTool {
    fn name(&self) -> &str {
        "suggest_replies"
    }
    fn description(&self) -> &str {
        "Show 2-4 short tappable replies under your message. Call it last, after your text, and only when there are obvious next steps or you need the user to pick between options. Each reply is what the user would send, in their language, at most 40 characters. Use kind \"choice\" when the user must pick one option, with an optional short question."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "replies": {
                    "type": "array",
                    "minItems": MIN_REPLIES,
                    "maxItems": MAX_REPLIES,
                    "items": {"type": "string", "maxLength": MAX_REPLY_CHARS},
                    "description": "Messages the user can send with one tap."
                },
                "kind": {
                    "type": "string",
                    "enum": ["replies", "choice"],
                    "description": "replies (default): likely next messages. choice: the user must pick one."
                },
                "question": {
                    "type": "string",
                    "maxLength": MAX_QUESTION_CHARS,
                    "description": "For choice: the question being answered, shown above the options."
                }
            },
            "required": ["replies"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, args: &Value) -> String {
        match args.get("kind").and_then(Value::as_str) {
            Some("choice") => "Offering choices".into(),
            _ => "Suggesting replies".into(),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        Ok(match build(&ctx.args) {
            Ok(suggestions) => {
                ToolResult::success(ctx.tool_call_id, serde_json::to_string(&suggestions)?)
            }
            Err(error) => ToolResult::failure(ctx.tool_call_id, error),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_shape() {
        let out =
            build(&json!({"replies": ["Yes please", "  No thanks ", "yes PLEASE", ""]})).unwrap();
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            json!({"type": "suggestions", "kind": "replies", "replies": ["Yes please", "No thanks"]})
        );

        let out = build(&json!({
            "kind": "choice",
            "question": "Which one?",
            "replies": ["The red one", "The blue one", "c", "d", "e"]
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            json!({
                "type": "suggestions",
                "kind": "choice",
                "question": "Which one?",
                "replies": ["The red one", "The blue one", "c", "d"]
            })
        );
    }

    #[test]
    fn long_replies_are_cut_and_too_few_rejected() {
        let long = "Book a table for two at the Italian place tonight at eight";
        let out = build(&json!({"replies": [long, "Not now"]})).unwrap();
        assert!(out.replies[0].chars().count() <= MAX_REPLY_CHARS);
        assert!(long.starts_with(&out.replies[0]));
        assert!(build(&json!({"replies": ["Only one"]})).is_err());
        assert!(build(&json!({"replies": ["a", "b"], "kind": "maybe"})).is_err());
    }
}
