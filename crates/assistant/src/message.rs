use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub id: String,
    pub name: String,
    /// The arguments as the model wrote them, a JSON object.
    pub arguments: String,
}

/// One message of a chat, as stored. `wire` turns it into what OpenRouter takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(default)]
        reasoning: String,
        #[serde(default)]
        calls: Vec<Call>,
    },
    Tool {
        call_id: String,
        name: String,
        text: String,
        #[serde(default)]
        ok: bool,
    },
}

impl Message {
    pub(crate) fn wire(&self) -> Value {
        match self {
            Self::User { text } => json!({"role": "user", "content": text}),
            Self::Assistant { text, calls, .. } => {
                let mut message = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    let calls: Vec<Value> = calls
                        .iter()
                        .map(|call| {
                            let arguments = if call.arguments.trim().is_empty() {
                                "{}"
                            } else {
                                &call.arguments
                            };
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {"name": call.name, "arguments": arguments},
                            })
                        })
                        .collect();
                    message["tool_calls"] = Value::Array(calls);
                }
                message
            }
            Self::Tool { call_id, text, .. } => {
                json!({"role": "tool", "tool_call_id": call_id, "content": text})
            }
        }
    }
}
