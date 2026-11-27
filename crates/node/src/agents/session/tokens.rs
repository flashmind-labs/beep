//! What a turn's response says it used.

use crate::agents::live::TurnTokens;
use crate::text::cut;
use agent_client_protocol::v1::schema::PromptResponse;
use beep_proto::ModelId;
use beep_proto::node::usage::{MAX_MODEL_NAME, MAX_TURN_MODELS, ModelTokens};
use serde_json::Value;

/// The tokens a turn used: `_meta.quota` as claude-agent-acp and
/// codex-acp give it, `token_count.totalTokens` with `model_usage` rows by
/// model, else the response's own `usage`, which any agent may send.
/// Nothing from an agent that says neither.
pub(super) fn turn_tokens(done: &PromptResponse) -> TurnTokens {
    reported_tokens(
        done.meta.as_ref(),
        done.usage.as_ref().map(|usage| usage.total_tokens),
    )
}

pub(super) fn reported_tokens(
    meta: Option<&serde_json::Map<String, Value>>,
    usage: Option<u64>,
) -> TurnTokens {
    let total = |count: &Value| count.get("totalTokens").and_then(Value::as_u64);
    let quota = meta.and_then(|meta| meta.get("quota"));
    let models = quota
        .and_then(|quota| quota.get("model_usage"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(ModelTokens {
                model: ModelId::new(cut(row.get("model")?.as_str()?, MAX_MODEL_NAME)),
                tokens: total(row.get("token_count")?)?,
            })
        })
        .filter(|row| row.tokens > 0)
        .take(MAX_TURN_MODELS)
        .collect();
    let total = quota
        .and_then(|quota| quota.get("token_count"))
        .and_then(total)
        .or(usage)
        .unwrap_or(0);
    TurnTokens { total, models }
}
