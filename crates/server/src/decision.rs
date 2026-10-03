//! Server-only Jev decisions through OpenRouter. The key and upstream error bodies never reach clients.
use anyhow::{Result, ensure};
use async_trait::async_trait;
use flashmind::llm::{JEV_LATEST_MODEL, JevQuestion, JevRequest, OpenRouterProvider};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use flashmind::types::{LlmError, LlmErrorKind};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_STATE_BYTES: usize = 20_000;
const MAX_QUESTIONS: usize = 16;
const MAX_ID_BYTES: usize = 64;

/// Holds the key only inside the provider, which has no `Debug` and never logs it.
pub struct DecisionTool {
    provider: OpenRouterProvider,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionArgs {
    state: String,
    questions: BTreeMap<String, JevQuestion>,
}

impl DecisionTool {
    pub fn new(key: SecretString) -> Result<Self> {
        let raw = key.expose_secret();
        ensure!(
            !raw.is_empty() && !raw.chars().any(char::is_whitespace),
            "invalid OpenRouter key"
        );
        let provider = OpenRouterProvider::new(raw.to_owned()).with_app_name("Beep".into());
        Ok(Self { provider })
    }

    #[cfg(test)]
    fn with_system_one_url(self, url: String) -> Self {
        Self {
            provider: self.provider.with_system_one_url(url),
        }
    }

    /// Jev's probability that the answer to `instructions` about `state` is yes.
    pub async fn probability(&self, state: &str, instructions: &str) -> Result<f64> {
        let question = JevQuestion::Noul {
            instructions: instructions.into(),
            criteria: None,
        };
        let request = JevRequest {
            model: JEV_LATEST_MODEL.into(),
            state: state[..state.floor_char_boundary(MAX_STATE_BYTES)].into(),
            questions: BTreeMap::from([("yes".to_owned(), question)]),
        };
        let response = self.provider.decide(request).await?;
        response
            .answers
            .get("yes")
            .and_then(|answer| answer["noul"].as_f64())
            .ok_or_else(|| anyhow::anyhow!("Jev gave no probability"))
    }

    async fn decide(&self, args: &Value) -> Result<Value, String> {
        let request = parse(args)?;
        let response = self.provider.decide(request).await.map_err(describe)?;
        Ok(json!({"answers": response.answers}))
    }
}

/// Turns a provider error into a client-safe message. The provider already
/// leaves upstream bodies out of its errors; transport errors are not echoed.
fn describe(error: anyhow::Error) -> String {
    match error.downcast_ref::<LlmError>() {
        Some(error) if error.kind == LlmErrorKind::RateLimited => {
            "Decision is rate limited. Try again shortly.".into()
        }
        Some(error) => format!("Decision failed: {}.", error.message),
        None => "Decision could not reach Jev. Try again shortly.".into(),
    }
}

fn parse(args: &Value) -> Result<JevRequest, String> {
    let args = DecisionArgs::deserialize(args).map_err(|e| format!("Invalid decision: {e}"))?;
    let state = args.state.trim();
    if state.is_empty() || state.len() > MAX_STATE_BYTES {
        return Err(format!(
            "Decision state must contain 1-{MAX_STATE_BYTES} bytes."
        ));
    }
    if args.questions.is_empty() || args.questions.len() > MAX_QUESTIONS {
        return Err(format!("Decision needs 1-{MAX_QUESTIONS} questions."));
    }
    if let Some(id) = args
        .questions
        .keys()
        .find(|id| id.is_empty() || id.len() > MAX_ID_BYTES)
    {
        return Err(format!(
            "Question ID {id:?} must contain 1-{MAX_ID_BYTES} bytes."
        ));
    }
    Ok(JevRequest {
        model: JEV_LATEST_MODEL.into(),
        state: state.into(),
        questions: args.questions,
    })
}

#[async_trait]
impl Tool for DecisionTool {
    fn name(&self) -> &str {
        "decision"
    }
    fn description(&self) -> &str {
        "Make fast, calibrated judgments over some text with Jev. Use it to categorize, route, filter or answer yes/no questions about a message, email, note or other item, instead of guessing. Ask several related questions in one call. Types: noul gives the probability that the answer is yes; choice picks one key from criteria; score rates the text against an ordered list of criteria. Treat answers as recommendations. Keep secrets and unnecessary personal data out of state."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "state": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": MAX_STATE_BYTES,
                    "description": "The text to judge."
                },
                "questions": {
                    "type": "object",
                    "minProperties": 1,
                    "maxProperties": MAX_QUESTIONS,
                    "description": "Questions keyed by short IDs such as is_spam or team.",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "type": {"type": "string", "enum": ["noul", "choice", "score"]},
                            "instructions": {"type": "string", "description": "The question to answer about the state."},
                            "criteria": {
                                "description": "choice: object mapping each option key to its description (required). score: ordered array of level descriptions (required). noul: optional object of hints.",
                                "anyOf": [
                                    {"type": "object", "additionalProperties": {"type": "string"}},
                                    {"type": "array", "items": {"type": "string"}}
                                ]
                            }
                        },
                        "required": ["type", "instructions"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["state", "questions"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, _args: &Value) -> String {
        "Deciding…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        Ok(match self.decide(&ctx.args).await {
            Ok(output) => ToolResult::success(ctx.tool_call_id, output.to_string()),
            Err(error) => ToolResult::failure(ctx.tool_call_id, error),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    fn args() -> Value {
        json!({
            "state": "Customer was charged twice and wants a refund.",
            "questions": {
                "needs_reply": {"type": "noul", "instructions": "Does this need a reply?"},
                "team": {
                    "type": "choice",
                    "instructions": "Which team should handle this?",
                    "criteria": {"billing": "Charges and refunds", "technical": "Product defects"}
                }
            }
        })
    }

    async fn read_request(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        loop {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
            if data.ends_with(b"}") {
                break;
            }
        }
        String::from_utf8(data).unwrap()
    }

    /// Answers every connection with the same response, so client retries see it too.
    async fn mock(
        status: &'static str,
        body: &'static str,
    ) -> (DecisionTool, UnboundedReceiver<String>) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/v1/systemone", listener.local_addr().unwrap());
        let tool = DecisionTool::new("private-test-key".into())
            .unwrap()
            .with_system_one_url(url);
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let request = read_request(&mut stream).await;
                stream.write_all(response.as_bytes()).await.unwrap();
                let _ = tx.send(request);
            }
        });
        (tool, rx)
    }

    #[tokio::test]
    async fn returns_answers_from_jev() {
        let body = r#"{"id":"r1","model":"jev-1.13","answers":{"needs_reply":{"p":0.93},"team":{"choice":"billing"}},"usage":{"input_tokens":12}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let output = tool.decide(&args()).await.unwrap();
        let request = requests.recv().await.unwrap();
        assert!(request.contains("Bearer private-test-key"));
        assert!(request.contains("\"model\":\"jev-latest\""));
        assert!(request.contains("\"type\":\"choice\""));
        assert_eq!(output["answers"]["team"]["choice"], "billing");
        assert_eq!(output["answers"]["needs_reply"]["p"], 0.93);
        assert!(output.get("usage").is_none());
    }

    #[tokio::test]
    async fn reads_one_probability() {
        let body = r#"{"model":"jev-1.13","answers":{"yes":{"type":"noul","noul":0.81}}}"#;
        let (tool, mut requests) = mock("200 OK", body).await;
        let p = tool.probability("User: I moved to Porto", "Worth remembering?");
        assert_eq!(p.await.unwrap(), 0.81);
        let request = requests.recv().await.unwrap();
        assert!(request.contains("\"type\":\"noul\""));
        assert!(request.contains("I moved to Porto"));
    }

    #[tokio::test]
    async fn redacts_upstream_errors() {
        let (tool, mut requests) = mock("429 Too Many Requests", "private-test-key").await;
        let error = tool.decide(&args()).await.unwrap_err();
        requests.recv().await.unwrap();
        assert!(error.contains("rate limited"));
        assert!(!error.contains("private-test-key"));
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(parse(&json!({"state": " ", "questions": args()["questions"]})).is_err());
        assert!(parse(&json!({"state": "x", "questions": {}})).is_err());
        assert!(
            parse(
                &json!({"state": "x", "questions": {"q": {"type": "choice", "instructions": "?"}}})
            )
            .is_err()
        );
        assert!(
            parse(
                &json!({"state": "x", "questions": {"q": {"type": "maybe", "instructions": "?"}}})
            )
            .is_err()
        );
    }
}
