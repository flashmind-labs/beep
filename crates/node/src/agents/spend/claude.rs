//! Claude Code's logs (`line`): each assistant line carries `message.usage`,
//! `message.model`, `message.id` and `requestId`.

use super::{Line, Request, billing::claude_cloud};
use crate::{agents::claude::unix_seconds, agents::prices::Tokens};
use beep_proto::node::usage::Billing;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    request_id: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CacheCreation {
    ephemeral_5m_input_tokens: u64,
    ephemeral_1h_input_tokens: u64,
}

/// Whether `line` may hold a request, before parsing it.
pub(super) fn wanted(line: &str) -> bool {
    line.contains("\"usage\"") && line.contains("\"assistant\"")
}

/// The request one line of a Claude Code log records, if any. One message
/// is written once per content block, each copy with the same usage, so
/// copies are told apart by message and request id.
pub(super) fn line(text: &str) -> Option<Line> {
    let entry: Entry = serde_json::from_str(text).ok()?;
    if entry.kind.as_deref() != Some("assistant") {
        return None;
    }
    let message = entry.message?;
    let usage = message.usage?;
    let model = message.model.filter(|model| !model.starts_with('<'))?;
    let (cache_write, cache_write_long) = match usage.cache_creation {
        Some(split)
            if split.ephemeral_5m_input_tokens + split.ephemeral_1h_input_tokens
                == usage.cache_creation_input_tokens =>
        {
            (
                split.ephemeral_5m_input_tokens,
                split.ephemeral_1h_input_tokens,
            )
        }
        _ => (usage.cache_creation_input_tokens, 0),
    };
    let tokens = Tokens {
        input: usage.input_tokens,
        output: usage.output_tokens,
        cache_read: usage.cache_read_input_tokens,
        cache_write,
        cache_write_long,
    };
    if tokens.total() == 0 {
        return None;
    }
    let key = (message.id.is_some() || entry.request_id.is_some()).then(|| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(message.id.as_deref().unwrap_or_default().as_bytes());
        hasher.update(b"\0");
        hasher.update(entry.request_id.as_deref().unwrap_or_default().as_bytes());
        let hash = hasher.finalize();
        let mut first = [0u8; 8];
        first.copy_from_slice(&hash.as_bytes()[..8]);
        u64::from_le_bytes(first)
    });
    let billing = if claude_cloud(message.id.as_deref(), &model) {
        Billing::Metered
    } else {
        Billing::Unknown
    };
    Some(Line::Request(Request {
        session: entry.session_id,
        cwd: entry.cwd,
        at: entry.timestamp.as_deref().and_then(unix_seconds)?,
        model: Some(model),
        tokens,
        cost_usd: None,
        key,
        total: None,
        provider: None,
        billing,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"type":"assistant","sessionId":"0f3c2a9e-1111-4222-8333-944455556666","cwd":"/srv/project","timestamp":"2026-11-02T10:15:00.000Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":300,"cache_read_input_tokens":30000,"cache_creation_input_tokens":2000,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":2000}}}}"#;

    #[test]
    fn an_assistant_line_gives_its_request() {
        assert!(wanted(LINE));
        let Some(Line::Request(request)) = line(LINE) else {
            panic!("a request");
        };
        assert_eq!(request.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(request.tokens.cache_write_long, 2000);
        assert_eq!(request.tokens.cache_write, 0);
        assert_eq!(request.at, 1_793_614_500);
        assert!(request.key.is_some());
        assert_eq!(request.cwd.as_deref(), Some("/srv/project"));
        assert_eq!(request.billing, Billing::Unknown);
        let bedrock = LINE.replace("msg_1", "msg_bdrk_1");
        let Some(Line::Request(request)) = line(&bedrock) else {
            panic!("a request");
        };
        assert_eq!(request.billing, Billing::Metered);
    }
}
